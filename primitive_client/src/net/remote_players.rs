//! Other connected players: interpolated toward the server's snapshots,
//! drawn as boxes, and treated as solid by the local physics.
//!
//! Changed this pass: the server no longer relays movement message by
//! message, it sends one interest-filtered `Snapshot` per tick. Two
//! consequences handled here:
//!
//! - A player can vanish from snapshots simply by walking out of interest
//!   range, with no `PlayerLeft` message. So each player carries a
//!   `last_seen` and is dropped after a timeout -- otherwise their box
//!   would hang frozen in the world forever, and worse, keep acting as a
//!   solid obstacle you'd walk into.
//! - Interpolation now targets the snapshot rate rather than an arbitrary
//!   message rate, which is why `INTERP_RATE_PER_SEC` is tied to it.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use bytemuck::{Pod, Zeroable};
use glam::Vec3;

use primitive_shared::protocol::{PlayerId, PlayerState};

use crate::logic::physics::{PLAYER_HALF_WIDTH, PLAYER_HEIGHT};

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct ActorVertex {
    pub position: [f32; 3],
    pub color: [f32; 3],
    /// Face normal, so actors can be lit by the same sun as the terrain
    /// instead of being flat-shaded slabs.
    pub normal: [f32; 3],
}

impl ActorVertex {
    pub const ATTRS: [wgpu::VertexAttribute; 3] =
        wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x3];

    pub fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<ActorVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRS,
        }
    }
}

const REMOTE_PLAYER_COLOR: [f32; 3] = [0.85, 0.25, 0.25];
/// How quickly the displayed position chases the latest snapshot.
/// Higher = snappier but jerkier, lower = smoother but laggier-looking.
const INTERP_RATE_PER_SEC: f32 = 12.0;
/// Drop a player we haven't heard about for this long.
const STALE_AFTER: Duration = Duration::from_secs(3);

pub struct RemotePlayer {
    /// What we're actually drawing and colliding against this frame.
    pub interpolated_pos: Vec3,
    /// Where the server last said they are; `interpolated_pos` eases
    /// toward this instead of snapping, since snapshots arrive at the
    /// tick rate, not every frame (Этап 4: "используйте интерполяцию для
    /// сглаживания движений других игроков").
    pub target_pos: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub username: Option<String>,
    pub last_seen: Instant,
}

#[derive(Default)]
pub struct RemotePlayers {
    players: HashMap<PlayerId, RemotePlayer>,
    names: HashMap<PlayerId, String>,
}

impl RemotePlayers {
    /// Applies one tick's worth of server truth.
    pub fn apply_snapshot(&mut self, states: &[PlayerState], my_id: Option<PlayerId>) {
        let now = Instant::now();
        for state in states {
            if Some(state.id) == my_id {
                continue;
            }
            let target = Vec3::new(state.x, state.y, state.z);
            // `get_mut` first rather than the entry API: the common case
            // is a known player, and the entry form would clone the
            // username per player per snapshot just to throw it away.
            if let Some(p) = self.players.get_mut(&state.id) {
                p.target_pos = target;
                p.yaw = state.yaw;
                p.pitch = state.pitch;
                p.last_seen = now;
            } else {
                self.players.insert(
                    state.id,
                    RemotePlayer {
                        interpolated_pos: target,
                        target_pos: target,
                        yaw: state.yaw,
                        pitch: state.pitch,
                        username: self.names.get(&state.id).cloned(),
                        last_seen: now,
                    },
                );
            }
        }
    }

    pub fn on_join(&mut self, id: PlayerId, username: String) {
        if let Some(player) = self.players.get_mut(&id) {
            player.username = Some(username.clone());
        }
        self.names.insert(id, username);
    }

    pub fn remove(&mut self, id: PlayerId) {
        self.players.remove(&id);
        self.names.remove(&id);
    }

    pub fn name_of(&self, id: PlayerId) -> Option<&str> {
        self.names.get(&id).map(|s| s.as_str())
    }

    /// Advances interpolation and forgets anyone who's gone quiet.
    pub fn tick(&mut self, dt: f32) {
        let t = (INTERP_RATE_PER_SEC * dt).clamp(0.0, 1.0);
        let now = Instant::now();
        for p in self.players.values_mut() {
            p.interpolated_pos = p.interpolated_pos.lerp(p.target_pos, t);
        }
        self.players
            .retain(|_, p| now.duration_since(p.last_seen) < STALE_AFTER);
    }

    pub fn iter_positions(&self) -> impl Iterator<Item = Vec3> + '_ {
        self.players.values().map(|p| p.interpolated_pos)
    }

    /// Who is under the crosshair, and how far off.
    ///
    /// The nearest one the ray reaches, so someone behind a player being
    /// aimed at is not hit through them. `dir` must be normalised, since
    /// the distance comes back in blocks and is compared against the
    /// distance to whatever block is behind them.
    ///
    /// Drawn against the *interpolated* position rather than the last
    /// snapshot -- the crosshair has to agree with what is on screen,
    /// and the server's tolerance is what covers the difference between
    /// that and where the server thinks they are. See
    /// `primitive_shared::combat`.
    pub fn aimed_at(&self, eye: Vec3, dir: Vec3, range: f32) -> Option<(PlayerId, f32)> {
        self.players
            .iter()
            .filter_map(|(&id, player)| {
                let feet = player.interpolated_pos;
                primitive_shared::geometry::ray_hits_player(
                    (eye.x, eye.y, eye.z),
                    (dir.x, dir.y, dir.z),
                    (feet.x, feet.y, feet.z),
                    range,
                )
                .map(|distance| (id, distance))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
    }

    #[allow(dead_code)] // companion to len(), used by tests
    pub fn is_empty(&self) -> bool {
        self.players.is_empty()
    }

    pub fn len(&self) -> usize {
        self.players.len()
    }
}

/// One combined mesh for every remote player's hitbox, in a single
/// vertex/index buffer -- cheap enough to rebuild every frame at the
/// player counts a single client can actually see.
#[cfg(test)]
pub fn build_actor_mesh(players: &RemotePlayers) -> (Vec<ActorVertex>, Vec<u32>) {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    build_actor_mesh_into(players, &mut vertices, &mut indices);
    (vertices, indices)
}

/// The same mesh, appended to buffers the caller keeps.
///
/// Rebuilt every frame because the players move every frame; reusing the
/// storage means it does not also allocate every frame.
pub fn build_actor_mesh_into(
    players: &RemotePlayers,
    vertices: &mut Vec<ActorVertex>,
    indices: &mut Vec<u32>,
) {
    for feet in players.iter_positions() {
        append_box(
            vertices,
            indices,
            feet,
            PLAYER_HALF_WIDTH,
            PLAYER_HEIGHT,
            REMOTE_PLAYER_COLOR,
        );
    }
}

fn append_box(
    vertices: &mut Vec<ActorVertex>,
    indices: &mut Vec<u32>,
    feet: Vec3,
    half_width: f32,
    height: f32,
    color: [f32; 3],
) {
    let min = feet - Vec3::new(half_width, 0.0, half_width);
    let max = feet + Vec3::new(half_width, height, half_width);

    let faces: [([f32; 3], [[f32; 3]; 4]); 6] = [
        (
            [0.0, 1.0, 0.0],
            [
                [min.x, max.y, min.z],
                [min.x, max.y, max.z],
                [max.x, max.y, max.z],
                [max.x, max.y, min.z],
            ],
        ),
        (
            [0.0, -1.0, 0.0],
            [
                [min.x, min.y, max.z],
                [min.x, min.y, min.z],
                [max.x, min.y, min.z],
                [max.x, min.y, max.z],
            ],
        ),
        (
            [1.0, 0.0, 0.0],
            [
                [max.x, min.y, min.z],
                [max.x, max.y, min.z],
                [max.x, max.y, max.z],
                [max.x, min.y, max.z],
            ],
        ),
        (
            [-1.0, 0.0, 0.0],
            [
                [min.x, min.y, max.z],
                [min.x, max.y, max.z],
                [min.x, max.y, min.z],
                [min.x, min.y, min.z],
            ],
        ),
        (
            [0.0, 0.0, 1.0],
            [
                [max.x, min.y, max.z],
                [max.x, max.y, max.z],
                [min.x, max.y, max.z],
                [min.x, min.y, max.z],
            ],
        ),
        (
            [0.0, 0.0, -1.0],
            [
                [min.x, min.y, min.z],
                [min.x, max.y, min.z],
                [max.x, max.y, min.z],
                [max.x, min.y, min.z],
            ],
        ),
    ];

    for (normal, corners) in faces {
        let base = vertices.len() as u32;
        for corner in corners {
            vertices.push(ActorVertex {
                position: corner,
                color,
                normal,
            });
        }
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(id: PlayerId, x: f32) -> PlayerState {
        PlayerState {
            id,
            x,
            y: 30.0,
            z: 0.0,
            yaw: 0.0,
            pitch: 0.0,
            on_ground: true,
        }
    }

    #[test]
    fn your_own_state_is_ignored() {
        let mut players = RemotePlayers::default();
        players.apply_snapshot(&[state(1, 0.0), state(2, 5.0)], Some(1));
        assert_eq!(players.len(), 1, "should not render yourself as a remote box");
    }

    #[test]
    fn interpolation_eases_rather_than_snapping() {
        let mut players = RemotePlayers::default();
        players.apply_snapshot(&[state(2, 0.0)], Some(1));
        players.apply_snapshot(&[state(2, 10.0)], Some(1));
        players.tick(1.0 / 60.0);
        let x = players.iter_positions().next().unwrap().x;
        assert!(x > 0.0 && x < 10.0, "expected an eased position, got {x}");
    }

    #[test]
    fn a_player_who_walks_out_of_range_is_forgotten() {
        // No PlayerLeft arrives in this case -- they simply stop appearing
        // in snapshots -- so the timeout is the only thing stopping a
        // ghost hitbox from blocking the path forever.
        let mut players = RemotePlayers::default();
        players.apply_snapshot(&[state(2, 0.0)], Some(1));
        assert_eq!(players.len(), 1);

        if let Some(p) = players.players.get_mut(&2) {
            p.last_seen = Instant::now() - Duration::from_secs(10);
        }
        players.tick(0.016);
        assert_eq!(players.len(), 0);
    }

    #[test]
    fn the_hitbox_mesh_is_a_closed_box_per_player() {
        let mut players = RemotePlayers::default();
        players.apply_snapshot(&[state(2, 0.0), state(3, 4.0)], Some(1));
        let (vertices, indices) = build_actor_mesh(&players);
        assert_eq!(vertices.len(), 2 * 24, "6 faces x 4 corners per player");
        assert_eq!(indices.len(), 2 * 36);
    }
}
