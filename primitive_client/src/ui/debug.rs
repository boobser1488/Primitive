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
    /// Where a frame's *own* time went, in milliseconds this second.
    ///
    /// Three numbers rather than one, because "the frame took 0.6 ms"
    /// says nothing about what to do next. `simulation` is everything
    /// before the renderer is called -- network, physics, streaming,
    /// building the interface. `encode` is the renderer itself: writing
    /// uniforms, culling, and recording draws. `present` is the wait for
    /// a swapchain image and the submit, which is where a frame that is
    /// GPU-bound spends its time doing nothing.
    pub simulation_ms_this_second: f32,
    pub encode_ms_this_second: f32,
    pub present_ms_this_second: f32,
    /// Time spent waiting for the GPU to hand back an image to draw
    /// into. See `renderer::GraphicsState::acquire_time_last_frame`.
    pub acquire_ms_this_second: f32,
    /// Frames the three above were summed over, so they can be reported
    /// per frame rather than per second.
    pub phase_frames: u32,
    /// What the GPU says it spent on the render pass, summed this
    /// second, and over how many frames it answered.
    ///
    /// Counted separately from `phase_frames` because the answer comes
    /// back a frame or three late and a frame that finds no free
    /// readback slot goes unmeasured -- so the two counts drift, and
    /// dividing by the wrong one turns a busy second into a quiet one.
    /// See `engine::gpu_timing`.
    pub gpu_ms_this_second: f32,
    pub gpu_frames: u32,
    /// The same, per stage of the render pass. See
    /// `engine::gpu_timing::STAGES` for what each one covers -- and note
    /// that "which fill" is the question the whole-pass number could not
    /// answer.
    pub gpu_stage_ms_this_second: [f32; crate::engine::gpu_timing::STAGES.len()],
    pub network_messages_in_this_second: u32,
    pub network_messages_out_this_second: u32,
    pub corrections_received: u32,
    /// Frames this second whose aim was *exactly* the previous frame's,
    /// and how many frames that was out of.
    ///
    /// **The number that tells a fast frame rate from a fast-looking
    /// one.** A frame drawn from the same aim as the frame before it is
    /// a frame the player cannot distinguish from the frame before it,
    /// so a counter of frames produced can read 160 while the view moves
    /// 50 times a second. Which of the two is happening decides the fix
    /// entirely -- fewer, better-paced frames, or a look input that is
    /// sampled more often than it is drawn -- and no other number on
    /// this panel separates them.
    ///
    /// Only meaningful while the player is actually turning, which is
    /// exactly when it is looked at.
    pub repeated_aim_frames: u32,
    pub aim_frames: u32,
    /// The last aim recorded, kept across the per-second reset: clearing
    /// it would count the first frame of every second as a repeat.
    last_aim: Option<(f32, f32)>,
}

/// The latitude as the F3 panel prints it, separator included: whole
/// degrees and a hemisphere (`45N`, `12S`), a bare `0` on the equator
/// itself, and nothing at all on the test world.
///
/// Letters rather than a degree sign because the font has no degree
/// sign (`font::ORDER`), and a glyph the font lacks draws as a hole.
fn latitude_label(latitude: Option<f32>) -> String {
    let Some(degrees) = latitude else {
        return String::new();
    };
    // Rounded before the sign is read, so a column a hair south of the
    // equator reads "0" rather than "0S".
    let whole = degrees.round() as i32;
    match whole.cmp(&0) {
        std::cmp::Ordering::Equal => "   lat 0".to_string(),
        std::cmp::Ordering::Greater => format!("   lat {whole}N"),
        std::cmp::Ordering::Less => format!("   lat {}S", -whole),
    }
}

/// Everything the title bar and F3 dump want to show, gathered in one
/// place so the call sites don't turn into ten-argument functions.
pub struct FrameInfo {
    pub position: Vec3,
    pub chunk: ChunkPos,
    pub grounded: bool,
    pub loaded_chunks: usize,
    pub pending_chunks: usize,
    /// How many pixels the frame is actually being drawn at.
    ///
    /// The number every fill-rate reading is per. A pass costing half a
    /// millisecond means one thing at 1280x720 and quite another at
    /// 2560x1440, and in borderless fullscreen the window size in the
    /// settings file says nothing about either.
    pub surface: (u32, u32),
    /// Anisotropic filtering, as a tap count.
    ///
    /// In the dump because it is the single most expensive per-pixel
    /// setting the game has -- up to this many texture fetches per
    /// fragment, and the count rises with how obliquely a surface is
    /// being looked at. A performance report that does not say what it
    /// was set to cannot be compared with another one.
    pub anisotropy: u16,
    /// Samples per pixel in the main pass, as the renderer is actually
    /// drawing it. Beside the anisotropy because it is the other
    /// per-pixel cost that multiplies the whole frame: at 4x every
    /// fragment along an edge is shaded up to four times and the depth
    /// and colour buffers are four times the size. A GPU time without
    /// this number beside it is not comparable with one from another
    /// run, and the setting alone does not say it -- an adapter short
    /// of the count silently runs at a lower one.
    pub msaa: u32,
    /// How much smaller than the frame the sky is drawn. In the dump
    /// beside the anisotropy and for the same reason: a performance
    /// report that does not say what the settings were cannot be
    /// compared with another one -- and this one is easy to *think* is
    /// on when the settings file says otherwise.
    pub sky_scale: u32,
    /// How finished frames reach the display, as the surface actually
    /// agreed rather than as the setting asked. See
    /// `GraphicsState::present_mode`.
    ///
    /// Here because of a report this panel could not answer: a frame
    /// rate of 160 that looked like fifty. Producing frames faster than
    /// the panel refreshes does not show more of them -- it picks which
    /// ones to show at uneven intervals, and uneven is what the eye
    /// reads as slow. Whether that is what is happening is a question
    /// about the present mode, and nothing on screen said what it was.
    pub present_mode: &'static str,
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
    /// How many particles are alive. Beside the entity count because it
    /// answers the same question -- "what is this frame drawing" -- and
    /// because a storm is the one thing that can put a thousand extra
    /// quads on screen without anything else changing.
    pub particles: usize,
    pub clock: String,
    /// The season, off the world's day count. See `season::Season`.
    pub season: &'static str,
    pub sun_intensity: f32,
    /// The world seed the server reported. Shown so a bug report can say
    /// which world it happened in.
    pub seed: u32,
    /// The biome under the player's feet.
    pub biome: std::borrow::Cow<'static, str>,
    /// How far north or south of the equator the player stands, in
    /// degrees (`WorldGen::latitude_degrees`); `None` on the test world.
    ///
    /// On the biome's line because it answers what the biome cannot: a
    /// meadow says what is here, and the latitude says which way the
    /// desert and the snow are, and how far.
    pub latitude: Option<f32>,
    pub selected_block: &'static str,
    pub draw_calls: u32,
    /// Indices the solid pass sent: the measure of how much geometry
    /// the frame actually asked the GPU to fetch.
    pub solid_indices: u32,
    /// ...and how many were in view before the groups looking away
    /// from the camera were left out.
    pub solid_indices_in_view: u32,
    /// Indices the cut-out pass sent: leaves, grass and the loose stones'
    /// thickness. Printed beside the solid count because it is not part of
    /// it and was, when it was first counted, the larger of the two.
    pub cutout_indices: u32,
    pub chunks_culled: usize,
    /// How many loaded chunks were meshed at each detail level: full
    /// detail, then the two coarse bands (`engine::lod`).
    ///
    /// **Here because a switch that did nothing looked exactly like a
    /// switch that did little.** Moving the coarse bands in from ten
    /// chunks to four took only a tenth of the triangles off an Adreno,
    /// where the whole-world measurement in `lod.rs` says a coarse chunk
    /// sheds over half of its own -- and from outside the device there was
    /// no way to tell whether the setting had reached the mesher at all.
    /// Now the line says so.
    pub chunk_levels: [usize; 3],
    /// What the loaded world keeps on the heap for blocks and for light,
    /// in bytes, and the terrain arena's `(used, allocated)` on the card.
    ///
    /// On the console line so that a memory figure is read off a running
    /// game under the same conditions as a frame time. Before these, the
    /// only way to know what the world cost was to multiply a chunk count
    /// by a size somebody remembered -- and the size changed fourfold
    /// when the world grew to 256 blocks without anything saying so.
    pub chunk_bytes: usize,
    pub light_bytes: usize,
    pub arena_bytes: (u64, u64),
    pub underwater: bool,
    // ---- survival ----
    pub health: f32,
    pub max_health: f32,
    /// How full the player is, 0..1. On the panel because it is the
    /// number a bug report about starving has to carry, and because the
    /// bar shows a fifth of a bar and this shows a number.
    pub nourishment: f32,
    /// What the sky is doing, as the server last said, and whether
    /// there is a fire close enough to work at. Both are world state
    /// the player cannot otherwise read off the screen with any
    /// precision -- "is this close enough" is exactly the question a
    /// player asks when a recipe is greyed out.
    pub weather: &'static str,
    /// ...and what that sky is actually dropping *here*
    /// (`weather::Precipitation`), which is not the same thing: the storm
    /// over a desert arrives as dust and puts no fire out. Beside the
    /// sky's own name rather than instead of it, because the pair is the
    /// answer -- "storm, dust" says both that the world is under weather
    /// and that this corner of it is getting none of the water.
    pub falling: primitive_shared::weather::Precipitation,
    pub heat: primitive_shared::crafting::Heat,
    /// How many of the selected block the player is carrying.
    pub held: u32,
    /// Everything in the inventory, across all stacks.
    pub carried: u32,
    /// The cell being mined and how far along it is, if anything is.
    pub mining: Option<((i32, i32, i32), f32)>,
    /// What sound is coming out of, already formatted. See
    /// `audio::Audio::status` -- the panel has no business knowing what
    /// a sample rate is, and the string is built once a frame in
    /// exactly the same way `clock` is.
    pub audio: String,
}

/// Bytes in the megabytes the console line reports memory in.
const MEGABYTE: f64 = 1024.0 * 1024.0;

impl DebugStats {
    pub fn record_frame(&mut self, dt: Duration) {
        self.frame_times.push_back(dt);
        if self.frame_times.len() > FRAME_HISTORY {
            self.frame_times.pop_front();
        }
    }

    /// Notes where this frame ended up looking.
    ///
    /// Compared for exact equality rather than against a tolerance, and
    /// that is deliberate: the question is whether the *image* differs,
    /// and the smallest change to the aim moves every pixel on screen.
    /// A tolerance here would answer a question nobody asked.
    pub fn record_aim(&mut self, yaw: f32, pitch: f32) {
        if self.last_aim == Some((yaw, pitch)) {
            self.repeated_aim_frames += 1;
        }
        self.aim_frames += 1;
        self.last_aim = Some((yaw, pitch));
    }

    /// The share of this second's frames that repeated the last aim,
    /// 0..1. Zero when nothing has been recorded, which reads as "every
    /// frame was new" and is the honest answer for a still camera.
    pub fn repeated_aim_share(&self) -> f32 {
        if self.aim_frames == 0 {
            return 0.0;
        }
        self.repeated_aim_frames as f32 / self.aim_frames as f32
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
                "{:.0} fps   {:.1} ms avg   p95 {:.1}   p99 {:.1}   still {:.0}%",
                self.fps(),
                average,
                self.frame_time_percentile(0.95).as_secs_f32() * 1000.0,
                self.frame_time_percentile(0.99).as_secs_f32() * 1000.0,
                self.repeated_aim_share() * 100.0,
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
            // The GPU's own answer beside the CPU's four, because the
            // useful reading is the comparison. A frame far longer than
            // its pass is held up by something that is not drawing.
            format!(
                "frame  sim {:.2}   encode {:.2}   wait {:.2}   present {:.2}   gpu {}   aniso {}x   msaa {}x",
                self.simulation_ms_this_second / self.phase_frames.max(1) as f32,
                self.encode_ms_this_second / self.phase_frames.max(1) as f32,
                self.acquire_ms_this_second / self.phase_frames.max(1) as f32,
                self.present_ms_this_second / self.phase_frames.max(1) as f32,
                self.gpu_pass_ms()
                    .map_or_else(|| "n/a".to_string(), |ms| format!("{ms:.2}")),
                info.anisotropy,
                info.msaa,
            ),
            format!("gpu  {}", self.gpu_stage_breakdown()),
            format!(
                "surface {}x{}   {:.1} Mpx   aniso {}x   msaa {}x   sky 1/{}   present {}",
                info.surface.0,
                info.surface.1,
                info.surface.0 as f32 * info.surface.1 as f32 / 1e6,
                info.anisotropy,
                info.msaa,
                info.sky_scale,
                info.present_mode,
            ),
            format!(
                "chunks {} loaded   {} pending   {} culled   r{}",
                info.loaded_chunks, info.pending_chunks, info.chunks_culled, info.render_distance,
            ),
            format!(
                "detail  {} fine   {} coarse   {} coarser",
                info.chunk_levels[0], info.chunk_levels[1], info.chunk_levels[2],
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
                "players {}   entities {}   particles {}   draws {}",
                info.remote_players, info.entities, info.particles, info.draw_calls,
            ),
            format!(
                "time {}   sun {:.0}%   holding {} x{}{}",
                info.clock,
                info.sun_intensity * 100.0,
                info.selected_block,
                info.held,
                if info.underwater { "   underwater" } else { "" },
            ),
            format!(
                "health {:.0}/{:.0}   carrying {}   {}",
                info.health,
                info.max_health,
                info.carried,
                match info.mining {
                    Some((cell, progress)) => format!(
                        "mining ({}, {}, {}) {:.0}%",
                        cell.0,
                        cell.1,
                        cell.2,
                        progress * 100.0
                    ),
                    None => "not mining".to_string(),
                }
            ),
            format!(
                "seed {}   biome {}{}",
                info.seed,
                info.biome,
                latitude_label(info.latitude)
            ),
            format!("audio  {}", info.audio),
            format!(
                "fed {:.0}%   sky {}{}{}",
                info.nourishment * 100.0,
                info.weather,
                match info.falling {
                    primitive_shared::weather::Precipitation::None => String::new(),
                    falling => format!(", {}", falling.name()),
                },
                match (info.heat.bloomery, info.heat.kiln, info.heat.fire) {
                    (true, _, _) => "   at a bloomery",
                    (_, true, _) => "   at a kiln",
                    (_, _, true) => "   at a fire",
                    _ => "",
                }
            ),
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
            "[F3] fps={:.0} frame(avg/p95/p99)={:.1}/{:.1}/{:.1}ms still={:.0}% | chunks loaded={} pending={} \
             mesh_queue={} arrivals={} lighting={} | meshed/s={} mesh_time/s={:.1}ms | \
             integrated/s={} chunk_time/s={:.1}ms upload/s={:.1}ms | \
             players={} entities={} draws={} tris={}k/{}k cut={}k culled={} detail={}/{}/{} | \
             net in/out per s={}/{} | corrections={} stale_meshes={} |              frame sim/encode/wait/present={:.3}/{:.3}/{:.3}/{:.3}ms gpu={} aniso={}x msaa={}x sky/{} present={} |              gpu stages: {} | {}x{} ({:.1} Mpx) | time={} {} sun={:.0}% |              mem blocks={:.1}MB light={:.1}MB arena={:.1}/{:.1}MB",
            self.fps(),
            self.frame_times.iter().sum::<Duration>().as_secs_f32() * 1000.0
                / self.frame_times.len().max(1) as f32,
            self.frame_time_percentile(0.95).as_secs_f32() * 1000.0,
            self.frame_time_percentile(0.99).as_secs_f32() * 1000.0,
            self.repeated_aim_share() * 100.0,
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
            info.solid_indices / 3000,
            info.solid_indices_in_view / 3000,
            info.cutout_indices / 3000,
            info.chunks_culled,
            info.chunk_levels[0],
            info.chunk_levels[1],
            info.chunk_levels[2],
            self.network_messages_in_this_second,
            self.network_messages_out_this_second,
            self.corrections_received,
            self.stale_meshes_discarded,
            self.simulation_ms_this_second / self.phase_frames.max(1) as f32,
            self.encode_ms_this_second / self.phase_frames.max(1) as f32,
            self.acquire_ms_this_second / self.phase_frames.max(1) as f32,
            self.present_ms_this_second / self.phase_frames.max(1) as f32,
            self.gpu_pass_ms()
                .map_or_else(|| "n/a".to_string(), |ms| format!("{ms:.3}ms")),
            info.anisotropy,
            info.msaa,
            info.sky_scale,
            info.present_mode,
            self.gpu_stage_breakdown(),
            info.surface.0,
            info.surface.1,
            info.surface.0 as f32 * info.surface.1 as f32 / 1e6,
            info.clock,
            info.season,
            info.sun_intensity * 100.0,
            info.chunk_bytes as f64 / MEGABYTE,
            info.light_bytes as f64 / MEGABYTE,
            info.arena_bytes.0 as f64 / MEGABYTE,
            info.arena_bytes.1 as f64 / MEGABYTE,
        );

        self.chunks_meshed_this_second = 0;
        self.mesh_time_ms_this_second = 0.0;
        self.chunks_integrated_this_second = 0;
        self.chunk_time_ms_this_second = 0.0;
        self.upload_time_ms_this_second = 0.0;
        self.network_messages_in_this_second = 0;
        self.network_messages_out_this_second = 0;
        self.simulation_ms_this_second = 0.0;
        self.encode_ms_this_second = 0.0;
        self.present_ms_this_second = 0.0;
        self.acquire_ms_this_second = 0.0;
        self.phase_frames = 0;
        self.gpu_ms_this_second = 0.0;
        self.gpu_frames = 0;
        self.gpu_stage_ms_this_second = [0.0; crate::engine::gpu_timing::STAGES.len()];
        self.repeated_aim_frames = 0;
        self.aim_frames = 0;
    }

    /// What the GPU spent on the render pass, averaged over the frames
    /// this second that came back with an answer.
    ///
    /// **This does not belong in the sum with the other three.** They
    /// are CPU stopwatches and they partition the frame; this one
    /// overlaps all of them, because the GPU is working while the CPU
    /// encodes the next frame and waits for a swapchain image. Its use
    /// is the comparison: a frame far longer than its pass is a frame
    /// held up by something that is not drawing, and no amount of
    /// removing geometry will shorten it.
    pub fn gpu_pass_ms(&self) -> Option<f32> {
        (self.gpu_frames > 0).then(|| self.gpu_ms_this_second / self.gpu_frames as f32)
    }

    /// `solid 0.31  cutout 0.02  sky 0.29 ...` -- the stages of the pass
    /// with what each cost, averaged over the frames that answered.
    ///
    /// Empty where the driver takes no marks inside a pass, so the line
    /// simply loses its tail rather than lying about zeroes.
    pub fn gpu_stage_breakdown(&self) -> String {
        let frames = self.gpu_frames.max(1) as f32;
        if self.gpu_frames == 0 || self.gpu_stage_ms_this_second.iter().all(|ms| *ms <= 0.0) {
            return String::new();
        }
        crate::engine::gpu_timing::STAGES
            .iter()
            .zip(self.gpu_stage_ms_this_second)
            .map(|(name, total)| format!("{name} {:.3}", total / frames))
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Records where one frame's own time went. See the fields.
    pub fn record_phases(
        &mut self,
        simulation: Duration,
        encode: Duration,
        acquire: Duration,
        present: Duration,
        gpu_ms: Option<f32>,
        gpu_stages: Option<[f32; crate::engine::gpu_timing::STAGES.len()]>,
    ) {
        if let Some(gpu_ms) = gpu_ms {
            self.gpu_ms_this_second += gpu_ms;
            self.gpu_frames += 1;
        }
        if let Some(stages) = gpu_stages {
            for (total, stage) in self.gpu_stage_ms_this_second.iter_mut().zip(stages) {
                *total += stage;
            }
        }
        self.simulation_ms_this_second += simulation.as_secs_f32() * 1000.0;
        self.encode_ms_this_second += encode.as_secs_f32() * 1000.0;
        self.acquire_ms_this_second += acquire.as_secs_f32() * 1000.0;
        self.present_ms_this_second += present.as_secs_f32() * 1000.0;
        self.phase_frames += 1;
    }
}

/// The F3 panel: a translucent card in the top-left corner with one line
/// of statistics per row.
///
/// Top-left because that is the one part of the screen nothing else uses
/// -- the crosshair is centred, the hotbar is at the bottom -- so the
/// panel never covers something the player is aiming at.
///
/// ## Why this one does not follow INTERFACE SIZE
///
/// It used to, and on a phone it covered two thirds of the screen. A
/// player asking for a bigger interface is asking about the things they
/// press and the words they read while playing; thirty lines of frame
/// timings are neither. So it stays the size it is on a desktop, and
/// where even that does not fit -- a short window, a long dump -- it
/// gives up size rather than lines: a readout with the bottom third off
/// the screen is missing exactly the numbers that were added last.
pub fn panel_into(
    lines: &[String],
    aspect: f32,
    font: crate::engine::texture::FontAtlas,
    out: &mut Vec<crate::ui::hotbar::HotbarVertex>,
) {
    use crate::ui::widgets;

    const SCALE: f32 = 0.85;
    const PAD: f32 = 0.02;
    /// How small the readout may be shrunk before it is left to
    /// overflow. Below about half, a 6x9 bitmap font stops being a font.
    const FLOOR: f32 = 0.5;

    let top = 0.94;
    // Shrunk to fit the height it has, never grown past the size it was
    // designed at: the panel is only ever a problem for being too big.
    let room = (top + 1.0 - PAD * 2.0).max(0.01);
    let wanted = lines.len() as f32 * widgets::line_height(SCALE);
    let scale = if wanted > room {
        (SCALE * room / wanted).max(SCALE * FLOOR)
    } else {
        SCALE
    };

    let mut painter = widgets::Painter::onto(font, std::mem::take(out));
    let widest = lines
        .iter()
        .map(|line| widgets::measure(line, scale))
        .fold(0.0f32, f32::max);
    let height = lines.len() as f32 * widgets::line_height(scale);

    // Anchored to the window's actual left edge, which needs the aspect
    // ratio: UI x runs from -aspect to +aspect. It used to be a constant
    // -1.68, which is the left edge of a 16:9 window and *off-screen* on
    // anything narrower -- on a 4:3 window the panel hung past the edge
    // and the first characters of every line were cut off.
    let left = -aspect.max(0.1) + PAD * 2.0;
    let panel = widgets::Rect::new(left - PAD, top - height - PAD, left + widest + PAD, top + PAD);
    painter.quad(panel, [0.03, 0.04, 0.06, 0.72]);
    // The F3 panel is drawn over the world in the dark skin, like the
    // chat: it is a readout, not a screen.
    painter.border(panel, 0.003, widgets::Theme::DARK.light);

    let mut y = top;
    for line in lines {
        painter.text(line, left, y, scale, widgets::TEXT);
        y -= widgets::line_height(scale);
    }
    *out = painter.into_vertices();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A frame drawn from the same aim as the last one is counted.
    ///
    /// **The measurement that answers "160 fps but it looks like 50".**
    /// The frame counter counts frames produced; this counts the ones a
    /// player could tell apart. A high rate with a high `still` share is
    /// a game drawing the same picture repeatedly -- the look input is
    /// sampled more slowly than the frames are drawn -- and a high rate
    /// with a low one is a pacing problem instead. The two have opposite
    /// fixes, which is the whole reason to measure rather than argue.
    #[test]
    fn frames_that_repeat_the_last_aim_are_counted_as_repeats() {
        let mut stats = DebugStats::default();
        // Nothing recorded reads as "every frame was new", not as a
        // division by zero.
        assert_eq!(stats.repeated_aim_share(), 0.0);

        // Turning steadily: every frame is a different picture.
        for i in 0..10 {
            stats.record_aim(i as f32 * 0.01, 0.0);
        }
        assert_eq!(stats.repeated_aim_share(), 0.0, "a steady turn repeats nothing");

        // ...and a look sampled half as often as the frames are drawn
        // shows up as half of them repeating.
        let mut sampled = DebugStats::default();
        for i in 0..10 {
            sampled.record_aim((i / 2) as f32 * 0.01, 0.0);
        }
        assert!(
            (sampled.repeated_aim_share() - 0.5).abs() < 1e-6,
            "got {}",
            sampled.repeated_aim_share(),
        );
    }

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
            surface: (1280, 720),
            anisotropy: 16,
            msaa: 4,
            sky_scale: 1,
            present_mode: "Fifo",
            render_distance: 8,
            pending_chunks: 3,
            queued_meshes: 4,
            queued_arrivals: 1,
            lighting_jobs: 2,
            remote_players: 0,
            entities: 0,
            particles: 0,
            clock: "12:00".to_string(),
            season: "spring",
            sun_intensity: 1.0,
            seed: 1337,
            biome: "plains".into(),
            latitude: Some(45.0),
            nourishment: 1.0,
            weather: "clear",
            falling: primitive_shared::weather::Precipitation::None,
            audio: String::new(),
            heat: primitive_shared::crafting::Heat::NONE,
            selected_block: "stone",
            draw_calls: 90,
            solid_indices: 0,
            solid_indices_in_view: 0,
            cutout_indices: 0,
            chunks_culled: 30,
            chunk_levels: [40, 20, 10],
            chunk_bytes: 0,
            light_bytes: 0,
            arena_bytes: (0, 0),
            underwater: false,
            health: 20.0,
            max_health: 20.0,
            held: 12,
            carried: 40,
            mining: None,
        }
    }

    #[test]
    fn the_overlay_says_how_many_samples_the_frame_is_drawn_at() {
        // Next to the anisotropy, on the same line as the GPU time it
        // has to be read against. A frame-time report that leaves it
        // out cannot be compared with one from another machine.
        let mut stats = DebugStats::default();
        stats.record_frame(Duration::from_millis(16));
        let lines = stats.overlay_lines(&sample_info());
        let gpu_line = lines
            .iter()
            .find(|line| line.contains("gpu ") && line.contains("aniso"))
            .expect("a line with the gpu time and the anisotropy");
        assert!(gpu_line.contains("aniso 16x   msaa 4x"), "got {gpu_line:?}");
        let surface_line = lines
            .iter()
            .find(|line| line.starts_with("surface"))
            .expect("the surface line");
        assert!(surface_line.contains("msaa 4x"), "got {surface_line:?}");
    }

    #[test]
    fn the_panel_says_which_side_of_the_equator_the_player_is_on() {
        // On the biome's line: the biome says what is here, the latitude
        // says which way the desert and the snow are.
        let lines = DebugStats::default().overlay_lines(&sample_info());
        assert!(
            lines.iter().any(|line| line.ends_with("biome plains   lat 45N")),
            "got {lines:?}"
        );
        assert_eq!(latitude_label(Some(-12.4)), "   lat 12S");
        assert_eq!(latitude_label(Some(89.7)), "   lat 90N");
        // A hair either side of the equator is the equator, not "0S".
        assert_eq!(latitude_label(Some(-0.3)), "   lat 0");
        assert_eq!(latitude_label(Some(0.3)), "   lat 0");
        // The test world has no latitude, and says nothing rather than
        // print a number that changes while nothing else does.
        assert_eq!(latitude_label(None), "");
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
