//! Entities: things in the world that aren't blocks and aren't players.
//!
//! Right now that means falling blocks. The server owns them; the client
//! receives a snapshot per tick and interpolates between snapshots, the
//! same arrangement remote players use — and for the same reason, since
//! snapshots arrive at the tick rate rather than once per frame.
//!
//! ## Motion
//!
//! Snapshots arrive at the server's tick rate (20 Hz); frames happen far
//! more often. The drawn position is therefore interpolated **between
//! the last two snapshots**, using the measured interval between them.
//!
//! The obvious-looking alternative -- easing the drawn position toward
//! the newest one at some fixed rate -- is what this did first, and it
//! is wrong for anything that moves steadily. An exponential chase
//! settles at a constant lag of `speed / rate`: sand at terminal
//! velocity (18 blocks/s) chased at 18 per second trails a **whole
//! block** behind where the server says it is, all the way down, and
//! then jumps that block when it lands. Interpolating between two known
//! samples has no such lag; it is one snapshot behind (50 ms), which is
//! not visible.
//!
//! ## Despawning
//!
//! The protocol has no despawn message, so there are three ways an
//! entity goes away, in order of how promptly they fire:
//!
//! * **It lands.** The block update that puts it back into the world
//!   also removes the entity, in the same frame. This is the one that
//!   matters: without it a landed block was drawn hovering above the
//!   real one it had just become, for as long as the timeout ran.
//! * **It is missing from a snapshot.** Each snapshot is the complete
//!   set of entities near the player, so anything absent from one is
//!   gone -- no waiting.
//! * **Nothing is heard for a while.** Only reachable when the *last*
//!   nearby entity disappears, since the server sends no message at all
//!   in that case. A backstop, not the normal path.
//!
//! Rendering reuses the chunk pipeline: a falling block is emitted as a
//! textured cube in world space with the same vertex format as terrain,
//! so it picks up the same lighting, fog and textures with no second
//! pipeline to keep in sync.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use glam::Vec3;

use primitive_shared::lighting::LightMap;
use primitive_shared::protocol::{EntityId, EntityKind, EntityState};
use primitive_shared::types::{BlockId, BLOCK_AIR, MAX_LIGHT};

use crate::mesh::{face_uv, faces, pack_light, Vertex};
use crate::texture::FaceLayers;

/// Drop an entity we haven't heard about for this long. Only the last
/// entity in range ever reaches this -- see the module docs.
const STALE_AFTER: Duration = Duration::from_millis(150);
/// Fallback when two snapshots arrive suspiciously close together or
/// impossibly far apart; roughly one server tick.
const NOMINAL_INTERVAL: Duration = Duration::from_millis(50);

struct Entity {
    kind: EntityKind,
    /// Where the previous snapshot put it, and where the latest one did.
    previous: Vec3,
    current: Vec3,
    /// When `current` arrived, and how long it took to arrive after
    /// `previous`. Measured rather than assumed, so a server running at
    /// a different tick rate interpolates correctly.
    updated_at: Instant,
    interval: Duration,
}

impl Entity {
    /// Position to draw at, `now`.
    fn drawn(&self, now: Instant) -> Vec3 {
        let elapsed = now.duration_since(self.updated_at).as_secs_f32();
        let interval = self.interval.as_secs_f32().max(1e-4);
        // Clamped, not extrapolated: overshooting on a late snapshot
        // would push a block through the floor it is about to land on.
        self.previous
            .lerp(self.current, (elapsed / interval).clamp(0.0, 1.0))
    }
}

#[derive(Default)]
pub struct Entities {
    entities: HashMap<EntityId, Entity>,
    /// The frame's clock, sampled once in `tick`, so every entity in one
    /// frame is drawn at the same instant.
    now: Option<Instant>,
    last_snapshot: Option<Instant>,
}

impl Entities {
    pub fn len(&self) -> usize {
        self.entities.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }

    pub fn apply_snapshot(&mut self, states: &[EntityState]) {
        let now = Instant::now();
        // Measured interval, clamped: a stalled frame or a burst of
        // queued messages must not stretch or collapse the motion.
        let interval = self
            .last_snapshot
            .map(|last| now.duration_since(last))
            .filter(|d| *d >= Duration::from_millis(5) && *d <= Duration::from_millis(500))
            .unwrap_or(NOMINAL_INTERVAL);
        self.last_snapshot = Some(now);

        for state in states {
            let current = Vec3::new(state.x, state.y, state.z);
            self.entities
                .entry(state.id)
                .and_modify(|e| {
                    // Continue from where it is being drawn, not from the
                    // last snapshot's position: if a frame was dropped
                    // the drawn position is somewhere in between, and
                    // restarting from the older sample would step back.
                    e.previous = e.drawn(now);
                    e.current = current;
                    e.kind = state.kind;
                    e.updated_at = now;
                    e.interval = interval;
                })
                .or_insert(Entity {
                    kind: state.kind,
                    // A newly seen entity starts where it is rather than
                    // easing in from wherever the last one happened to
                    // be -- otherwise a block that starts falling appears
                    // to fly in from somewhere else.
                    previous: current,
                    current,
                    updated_at: now,
                    interval,
                });
        }

        // A snapshot is the complete set of nearby entities, so anything
        // it doesn't mention has gone.
        self.entities
            .retain(|id, _| states.iter().any(|state| state.id == *id));
    }

    /// A block has appeared in the world at this cell.
    ///
    /// If a falling entity was occupying it, that entity just landed and
    /// *is* this block -- so it stops being drawn now, rather than
    /// hovering over its own landing site until the stale timeout.
    pub fn on_block_placed(&mut self, gx: i32, gy: i32, gz: i32, block: BlockId) {
        if block == BLOCK_AIR {
            return;
        }
        let now = self.now.unwrap_or_else(Instant::now);
        self.entities.retain(|_, entity| {
            let at = entity.drawn(now);
            !(at.x.floor() as i32 == gx
                && at.z.floor() as i32 == gz
                // A little vertical slack: the entity is drawn one
                // snapshot behind, so at the moment it lands it can
                // still be up to a cell above its resting place.
                && (at.y.floor() as i32 - gy).abs() <= 1)
        });
    }

    pub fn tick(&mut self, _dt: f32) {
        let now = Instant::now();
        self.now = Some(now);
        if let Some(last) = self.last_snapshot {
            if now.duration_since(last) >= STALE_AFTER {
                self.entities.clear();
            }
        }
    }

    /// Builds one mesh for every entity, in the terrain vertex format.
    /// Convenience wrapper that allocates. The frame loop uses
    /// `build_mesh_into` with buffers it keeps; this is for tests, which
    /// care about the geometry and not about the allocation.
    #[cfg(test)]
    pub fn build_mesh(&self, layers: &FaceLayers, light: &LightMap) -> (Vec<Vertex>, Vec<u32>) {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        self.build_mesh_into(layers, light, &mut vertices, &mut indices);
        (vertices, indices)
    }

    /// The same mesh, appended to buffers the caller keeps.
    ///
    /// The frame loop rebuilds this every frame because the entities
    /// move every frame; reusing the storage means it does not also
    /// allocate every frame.
    pub fn build_mesh_into(
        &self,
        layers: &FaceLayers,
        light: &LightMap,
        vertices: &mut Vec<Vertex>,
        indices: &mut Vec<u32>,
    ) {
        let now = self.now.unwrap_or_else(Instant::now);

        for entity in self.entities.values() {
            match entity.kind {
                EntityKind::FallingBlock { block } => {
                    append_cube(vertices, indices, entity.drawn(now), block, layers, light);
                }
            }
        }
    }
}

fn append_cube(
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
    origin: Vec3,
    block: primitive_shared::types::BlockId,
    layers: &FaceLayers,
    light: &LightMap,
) {
    // Light is sampled once at the entity's centre rather than per face.
    // A falling block is in the air and in motion; per-face sampling
    // would make it flicker as it crosses cell boundaries.
    let sample = origin + Vec3::splat(0.5);
    let (cx, cy, cz) = (
        sample.x.floor() as i32,
        sample.y.floor() as i32,
        sample.z.floor() as i32,
    );

    // Take the brightest of the cell it is in and the one above it.
    //
    // Two reasons, both visible. A block only *just* out of the grid can
    // still be sampled in the cell it vacated before the light there has
    // been recomputed, which reads as a black cube for a frame or two.
    // And a chunk whose lighting has not been computed yet answers 0 for
    // every cell -- so sand falling at the streaming frontier rendered
    // as a silhouette. Neither is a case where darkness is the honest
    // answer: the block is in mid-air, and the air above it is lit.
    let sky = light.sky(cx, cy, cz).max(light.sky(cx, cy + 1, cz));
    let block_light = light.block(cx, cy, cz).max(light.block(cx, cy + 1, cz));
    let sky = if light.is_lit(primitive_shared::types::ChunkPos::from_world(sample.x, sample.z)) {
        sky
    } else {
        // Nothing is known about this column at all. Full sky is the
        // better guess than none: the frontier reads slightly bright
        // rather than as a hole.
        MAX_LIGHT
    };

    for (face_index, face) in faces().iter().enumerate() {
        let layer = layers.layer_for_face(block, face_index);
        let base = vertices.len() as u32;
        for corner in face.corners.iter() {
            vertices.push(Vertex {
                position: [
                    origin.x + corner[0],
                    origin.y + corner[1],
                    origin.z + corner[2],
                ],
                uv: face_uv(face_index, *corner),
                tex_layer: layer,
                // Full ambient occlusion (3 = unoccluded): an entity is
                // surrounded by air by definition.
                light: pack_light(sky, block_light, 3, face_index as u8),
            });
        }
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use primitive_shared::types::BLOCK_SAND;


    fn state(id: EntityId, y: f32) -> EntityState {
        EntityState {
            id,
            kind: EntityKind::FallingBlock { block: BLOCK_SAND },
            x: 1.0,
            y,
            z: 2.0,
        }
    }

    /// Lowest vertex of the drawn geometry -- the entity's own y, since
    /// the cube is built upward from its origin.
    fn drawn_y(entities: &Entities) -> f32 {
        let (vertices, _) = entities.build_mesh(&FaceLayers::empty_for_test(), &LightMap::new());
        vertices
            .iter()
            .map(|v| v.position[1])
            .fold(f32::MAX, f32::min)
    }

    #[test]
    fn a_new_entity_appears_where_it_is() {
        // Not eased in from somewhere else -- a block that starts
        // falling must not appear to fly in from the origin.
        let mut entities = Entities::default();
        entities.apply_snapshot(&[state(1, 40.0)]);
        assert_eq!(drawn_y(&entities), 40.0);
    }

    #[test]
    fn movement_is_interpolated_between_snapshots() {
        let mut entities = Entities::default();
        entities.apply_snapshot(&[state(1, 40.0)]);
        entities.apply_snapshot(&[state(1, 30.0)]);

        // Halfway through the interval between the two snapshots.
        let entity = entities.entities.get_mut(&1).expect("entity");
        entity.interval = Duration::from_millis(100);
        entity.updated_at = Instant::now() - Duration::from_millis(50);
        entities.tick(1.0 / 60.0);

        let y = drawn_y(&entities);
        assert!(y < 40.0 && y > 30.0, "expected an eased position, got {y}");
    }

    #[test]
    fn interpolation_reaches_the_target_rather_than_trailing_behind_it() {
        // Regression. The drawn position used to chase the latest
        // snapshot exponentially, which settles at a constant lag
        // proportional to speed: sand at terminal velocity was drawn a
        // whole block above where the server said it was, the whole way
        // down, and jumped that block when it landed.
        let mut entities = Entities::default();
        entities.apply_snapshot(&[state(1, 40.0)]);
        entities.apply_snapshot(&[state(1, 39.1)]);

        // One full interval later, it must be exactly at the sample --
        // not merely approaching it.
        let entity = entities.entities.get_mut(&1).expect("entity");
        entity.interval = Duration::from_millis(50);
        entity.updated_at = Instant::now() - Duration::from_millis(50);
        entities.tick(1.0 / 60.0);

        assert!(
            (drawn_y(&entities) - 39.1).abs() < 1e-3,
            "still lagging at {}",
            drawn_y(&entities)
        );
    }

    #[test]
    fn a_landing_block_update_removes_the_entity_at_once() {
        // This is what stops a landed block being drawn hovering over
        // the real block it just became.
        let mut entities = Entities::default();
        entities.apply_snapshot(&[state(1, 1.4)]);
        assert_eq!(entities.len(), 1);

        // The server places sand in the cell the entity is in.
        entities.on_block_placed(1, 1, 2, BLOCK_SAND);
        assert!(entities.is_empty(), "the landed entity is still drawn");
    }

    #[test]
    fn a_block_update_somewhere_else_leaves_the_entity_alone() {
        let mut entities = Entities::default();
        entities.apply_snapshot(&[state(1, 20.0)]);
        entities.on_block_placed(1, 5, 2, BLOCK_SAND); // far below
        entities.on_block_placed(9, 20, 9, BLOCK_SAND); // another column
        entities.on_block_placed(1, 20, 2, BLOCK_AIR); // a break, not a landing
        assert_eq!(entities.len(), 1);
    }

    #[test]
    fn an_entity_missing_from_a_snapshot_disappears_immediately() {
        // Each snapshot is the complete nearby set, so absence is
        // information -- waiting out a timeout would leave a ghost.
        let mut entities = Entities::default();
        entities.apply_snapshot(&[state(1, 40.0), state(2, 30.0)]);
        assert_eq!(entities.len(), 2);
        entities.apply_snapshot(&[state(2, 29.0)]);
        assert_eq!(entities.len(), 1, "entity 1 should be gone");
    }

    #[test]
    fn silence_eventually_clears_the_last_entity() {
        // The server sends nothing at all once no entity is near, so
        // this timeout is the only thing that clears the final one.
        let mut entities = Entities::default();
        entities.apply_snapshot(&[state(1, 40.0)]);
        entities.last_snapshot = Some(Instant::now() - Duration::from_secs(5));
        entities.tick(0.016);
        assert!(entities.is_empty(), "stale entity was not dropped");
    }

    #[test]
    fn a_falling_block_is_a_closed_cube() {
        let mut entities = Entities::default();
        entities.apply_snapshot(&[state(1, 10.0), state(2, 20.0)]);
        let (vertices, indices) =
            entities.build_mesh(&FaceLayers::empty_for_test(), &LightMap::new());
        assert_eq!(vertices.len(), 2 * 24, "6 faces x 4 corners per entity");
        assert_eq!(indices.len(), 2 * 36);
    }

    #[test]
    fn no_entities_means_no_geometry() {
        let entities = Entities::default();
        let (vertices, indices) =
            entities.build_mesh(&FaceLayers::empty_for_test(), &LightMap::new());
        assert!(vertices.is_empty() && indices.is_empty());
    }
}
