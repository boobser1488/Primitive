//! Debug statistics, shown three ways:
//!
//! 1. An **on-screen panel**, toggled with F3. This is the one that
//!    matters: reading numbers off a window title while looking at the
//!    world is not a thing anyone can actually do, and a console behind
//!    a fullscreen game is not visible at all.
//! 2. The window title, updated every frame, for the at-a-glance case.
//! 3. A per-second console dump, for when you want a log to scroll back
//!    through rather than a snapshot.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use glam::Vec3;

use primitive_shared::types::ChunkPos;

const FRAME_HISTORY: usize = 240;

#[derive(Default)]
pub struct DebugStats {
    pub console_enabled: bool,
    frame_times: VecDeque<Duration>,
    last_console_dump: Option<Instant>,

    pub chunks_meshed_this_second: u32,
    pub chunks_integrated_this_second: u32,
    pub chunk_time_ms_this_second: f32,
    pub upload_time_ms_this_second: f32,
    pub stale_meshes_discarded: u32,
    pub mesh_time_ms_this_second: f32,
    pub network_messages_in_this_second: u32,
    pub network_messages_out_this_second: u32,
    pub corrections_received: u32,
}

/// Everything the title bar and F3 dump want to show, gathered in one
/// place so the call sites don't turn into ten-argument functions.
pub struct FrameInfo {
    pub position: Vec3,
    pub chunk: ChunkPos,
    pub grounded: bool,
    pub loaded_chunks: usize,
    pub pending_chunks: usize,
    /// The radius actually in use, which is the player's setting capped
    /// by what the server said it would stream. Worth showing now that
    /// the setting can be changed mid-session: it is the only way to
    /// see that a server refused to go as far as you asked.
    pub render_distance: i32,
    pub queued_meshes: usize,
    pub queued_arrivals: usize,
    pub lighting_jobs: usize,
    pub remote_players: usize,
    pub entities: usize,
    pub clock: String,
    pub sun_intensity: f32,
    /// The world seed the server reported. Shown so a bug report can say
    /// which world it happened in.
    pub seed: u32,
    pub selected_block: &'static str,
    pub draw_calls: u32,
    pub chunks_culled: usize,
    pub underwater: bool,
}

impl DebugStats {
    pub fn record_frame(&mut self, dt: Duration) {
        self.frame_times.push_back(dt);
        if self.frame_times.len() > FRAME_HISTORY {
            self.frame_times.pop_front();
        }
    }

    pub fn toggle_console(&mut self) {
        self.console_enabled = !self.console_enabled;
    }

    fn fps(&self) -> f32 {
        if self.frame_times.is_empty() {
            return 0.0;
        }
        let avg: Duration =
            self.frame_times.iter().sum::<Duration>() / self.frame_times.len() as u32;
        if avg.as_secs_f32() > 0.0 {
            1.0 / avg.as_secs_f32()
        } else {
            0.0
        }
    }

    fn frame_time_percentile(&self, p: f32) -> Duration {
        if self.frame_times.is_empty() {
            return Duration::ZERO;
        }
        let mut sorted: Vec<Duration> = self.frame_times.iter().copied().collect();
        sorted.sort();
        let idx = ((sorted.len() as f32 - 1.0) * p).round() as usize;
        sorted[idx]
    }

    pub fn title(&self, info: &FrameInfo) -> String {
        format!(
            "Primitive | {:.0} FPS | {:.1},{:.1},{:.1} | chunk {},{} | {} | {} chunks | \
             {} players | {} | sun {:.0}% | [{}]{} (F3 stats)",
            self.fps(),
            info.position.x,
            info.position.y,
            info.position.z,
            info.chunk.x,
            info.chunk.z,
            if info.grounded { "grounded" } else { "airborne" },
            info.loaded_chunks,
            info.remote_players,
            info.clock,
            info.sun_intensity * 100.0,
            info.selected_block,
            if info.underwater { " underwater" } else { "" },
        )
    }

    /// The lines of the on-screen F3 panel.
    ///
    /// Ordered so the first few answer the questions asked most often --
    /// where am I, is it running smoothly, is the world still streaming
    /// -- because those are readable even when the panel is glanced at
    /// rather than read.
    pub fn overlay_lines(&self, info: &FrameInfo) -> Vec<String> {
        let frames = self.frame_times.len().max(1) as f32;
        let average = self.frame_times.iter().sum::<Duration>().as_secs_f32() * 1000.0 / frames;
        vec![
            format!(
                "{:.0} fps   {:.1} ms avg   p95 {:.1}   p99 {:.1}",
                self.fps(),
                average,
                self.frame_time_percentile(0.95).as_secs_f32() * 1000.0,
                self.frame_time_percentile(0.99).as_secs_f32() * 1000.0,
            ),
            format!(
                "xyz {:.1} {:.1} {:.1}   chunk {} {}   {}",
                info.position.x,
                info.position.y,
                info.position.z,
                info.chunk.x,
                info.chunk.z,
                if info.grounded { "grounded" } else { "airborne" },
            ),
            format!(
                "chunks {} loaded   {} pending   {} culled   r{}",
                info.loaded_chunks, info.pending_chunks, info.chunks_culled, info.render_distance,
            ),
            format!(
                "queues  mesh {}   arrivals {}   lighting {}",
                info.queued_meshes, info.queued_arrivals, info.lighting_jobs,
            ),
            format!(
                "meshed/s {}   integrated/s {}   stale {}",
                self.chunks_meshed_this_second,
                self.chunks_integrated_this_second,
                self.stale_meshes_discarded,
            ),
            format!(
                "cpu/frame  mesh {:.1} ms   chunks {:.1} ms   upload {:.1} ms",
                self.mesh_time_ms_this_second,
                self.chunk_time_ms_this_second,
                self.upload_time_ms_this_second,
            ),
            format!(
                "net in/s {}   out/s {}   corrections {}",
                self.network_messages_in_this_second,
                self.network_messages_out_this_second,
                self.corrections_received,
            ),
            format!(
                "players {}   entities {}   draws {}",
                info.remote_players, info.entities, info.draw_calls,
            ),
            format!(
                "time {}   sun {:.0}%   holding {}{}",
                info.clock,
                info.sun_intensity * 100.0,
                info.selected_block,
                if info.underwater { "   underwater" } else { "" },
            ),
            format!("seed {}", info.seed),
        ]
    }

    /// Call once per frame; only actually prints once a second when
    /// `console_enabled` is on.
    pub fn maybe_dump_console(&mut self, info: &FrameInfo) {
        if !self.console_enabled {
            return;
        }
        let now = Instant::now();
        let should_dump = match self.last_console_dump {
            Some(last) => now.duration_since(last) >= Duration::from_secs(1),
            None => true,
        };
        if !should_dump {
            return;
        }
        self.last_console_dump = Some(now);

        println!(
            "[F3] fps={:.0} frame(avg/p95/p99)={:.1}/{:.1}/{:.1}ms | chunks loaded={} pending={} \
             mesh_queue={} arrivals={} lighting={} | meshed/s={} mesh_time/s={:.1}ms | \
             integrated/s={} chunk_time/s={:.1}ms upload/s={:.1}ms | \
             players={} entities={} draws={} culled={} | \
             net in/out per s={}/{} | corrections={} stale_meshes={} | time={} sun={:.0}%",
            self.fps(),
            self.frame_times.iter().sum::<Duration>().as_secs_f32() * 1000.0
                / self.frame_times.len().max(1) as f32,
            self.frame_time_percentile(0.95).as_secs_f32() * 1000.0,
            self.frame_time_percentile(0.99).as_secs_f32() * 1000.0,
            info.loaded_chunks,
            info.pending_chunks,
            info.queued_meshes,
            info.queued_arrivals,
            info.lighting_jobs,
            self.chunks_meshed_this_second,
            self.mesh_time_ms_this_second,
            self.chunks_integrated_this_second,
            self.chunk_time_ms_this_second,
            self.upload_time_ms_this_second,
            info.remote_players,
            info.entities,
            info.draw_calls,
            info.chunks_culled,
            self.network_messages_in_this_second,
            self.network_messages_out_this_second,
            self.corrections_received,
            self.stale_meshes_discarded,
            info.clock,
            info.sun_intensity * 100.0,
        );

        self.chunks_meshed_this_second = 0;
        self.mesh_time_ms_this_second = 0.0;
        self.chunks_integrated_this_second = 0;
        self.chunk_time_ms_this_second = 0.0;
        self.upload_time_ms_this_second = 0.0;
        self.network_messages_in_this_second = 0;
        self.network_messages_out_this_second = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fps_is_derived_from_recorded_frames() {
        let mut stats = DebugStats::default();
        for _ in 0..10 {
            stats.record_frame(Duration::from_millis(20));
        }
        assert!((stats.fps() - 50.0).abs() < 1.0, "got {}", stats.fps());
    }

    #[test]
    fn frame_history_is_bounded() {
        let mut stats = DebugStats::default();
        for _ in 0..(FRAME_HISTORY * 3) {
            stats.record_frame(Duration::from_millis(16));
        }
        assert_eq!(stats.frame_times.len(), FRAME_HISTORY);
    }

    #[test]
    fn percentiles_do_not_panic_on_an_empty_history() {
        let stats = DebugStats::default();
        assert_eq!(stats.frame_time_percentile(0.99), Duration::ZERO);
        assert_eq!(stats.fps(), 0.0);
    }

    fn sample_info() -> FrameInfo {
        FrameInfo {
            position: Vec3::new(1.25, 40.0, -3.5),
            chunk: ChunkPos::new(0, -1),
            grounded: true,
            loaded_chunks: 120,
            render_distance: 8,
            pending_chunks: 3,
            queued_meshes: 4,
            queued_arrivals: 1,
            lighting_jobs: 2,
            remote_players: 0,
            entities: 0,
            clock: "12:00".to_string(),
            sun_intensity: 1.0,
            seed: 1337,
            selected_block: "stone",
            draw_calls: 90,
            chunks_culled: 30,
            underwater: false,
        }
    }

    #[test]
    fn the_overlay_reports_position_and_frame_rate_on_the_first_lines() {
        let mut stats = DebugStats::default();
        stats.record_frame(Duration::from_millis(16));
        let lines = stats.overlay_lines(&sample_info());
        assert!(lines[0].contains("fps"), "got {:?}", lines[0]);
        assert!(lines[1].contains("1.2") && lines[1].contains("40.0"), "got {:?}", lines[1]);
    }

    #[test]
    fn the_overlay_never_produces_characters_the_font_cannot_draw() {
        // It is rendered with the 5x7 bitmap font, which is ASCII only;
        // anything else comes out as a box.
        let mut stats = DebugStats::default();
        stats.record_frame(Duration::from_millis(16));
        for line in stats.overlay_lines(&sample_info()) {
            assert!(
                line.chars().all(|c| c.is_ascii_graphic() || c == ' '),
                "unrenderable line: {line:?}"
            );
        }
    }

    #[test]
    fn the_overlay_survives_a_frame_with_no_history() {
        let stats = DebugStats::default();
        assert!(!stats.overlay_lines(&sample_info()).is_empty());
    }
}
