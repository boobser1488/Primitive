//! Настройки сервера. Loaded from `settings.toml` next to wherever the
//! server binary runs; if the file doesn't exist we write out the
//! defaults, so there's always something to edit next time instead of a
//! startup failure.
//!
//! Every knob that matters for running this at scale is here rather than
//! hard-coded: tick rate, interest radius, per-client queue depth, chunk
//! streaming budget, cache size, connection caps, and the whole
//! anti-cheat section.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerSettings {
    pub bind_addr: String,
    /// Shown to clients in `Welcome`, and in the console banner.
    pub server_name: String,
    pub world_seed: u32,
    /// Which generator makes the terrain. See `worldgen::Preset`.
    ///
    /// A seed says *which* world of a kind; this says of what kind. It
    /// lives beside the seed here for the same reason it lives beside
    /// the seed in a singleplayer world's `world.toml`: the pair is what
    /// the saved edits were written against, and changing either one
    /// puts a player's buildings on ground that never existed.
    pub world_preset: primitive_shared::worldgen::Preset,
    /// Where on the planet the world is laid. See `worldgen::Zone`.
    ///
    /// Beside the seed and the preset for their reason, and like them not
    /// something to change under a world with edits in it: the same
    /// buildings laid at another latitude stand in another country.
    pub world_zone: primitive_shared::worldgen::Zone,
    /// Which scale the country is drawn at. See `worldgen::Scale`.
    ///
    /// **A file without it is an old world's**, and reads as
    /// `Scale::Regional` -- the field's own default, not the struct's: a
    /// server upgraded under an existing world keeps the ground its players
    /// built on, and a server writing its first settings file writes the
    /// Earth's. Not something to change under a world with edits in it, for
    /// the seed's reason.
    #[serde(default = "primitive_shared::worldgen::Scale::unrecorded")]
    pub world_scale: primitive_shared::worldgen::Scale,
    /// Hard cap on simultaneous players. Beyond this, new connections are
    /// answered with `Rejected(ServerFull)` instead of being silently
    /// dropped, so the client can say something useful.
    pub max_players: usize,

    // ---- simulation ----
    /// Simulation/broadcast rate. Player snapshots are assembled once per
    /// tick, not once per received movement message.
    pub tick_rate_hz: f32,
    /// How far (in blocks) another player has to be before we stop
    /// including them in someone's snapshot. This is the single most
    /// important number for player-count scaling: cost per tick is
    /// O(players × players-in-radius), not O(players²).
    pub interest_radius_blocks: f32,
    /// How far the server is willing to stream chunks. A client asking
    /// for more than this gets ignored (and flagged, see anti-cheat).
    pub view_distance_chunks: i32,
    /// Full in-game day length in real seconds. Drives the sun position,
    /// sky colour and skylight level on every client.
    ///
    /// Thirty minutes by default. It was fifteen, then twenty after "ночь
    /// быстро наступает", and thirty is the number the player asked for
    /// outright ("сделай сутки 30 минут"). A day is the clock every other slow thing in the world is
    /// measured against -- rot, growth, a fire burning down -- so lengthening
    /// it slows all of them together, which is the point. Dusk is the other
    /// half of the same complaint and is the sky's: see `TWILIGHT_FALL` in
    /// the client's `engine::sky`.
    pub day_length_seconds: f32,
    /// Time of day the world starts at (0.0 = midnight, 0.5 = noon).
    pub start_time_of_day: f32,

    // ---- streaming / memory ----
    /// Chunks kept in RAM. Beyond this the least-recently-used ones are
    /// dropped; because generation is deterministic and player edits live
    /// in a separate overlay, an evicted chunk can be rebuilt byte-identical
    /// later. Memory is therefore bounded by this number, not by how much
    /// of the world players have explored.
    pub max_cached_chunks: usize,
    /// Chunks sent to a single client per tick. Caps how much one player
    /// joining can starve everyone else's traffic.
    ///
    /// This is a *sending* budget and no longer a generation one: since
    /// terrain moved onto its own pool (`logic::chunkgen`) a chunk that
    /// is not ready yet costs the pump nothing, so this number can be
    /// what it was always meant to be -- how much of one player's world
    /// may go down the wire in a fiftieth of a second.
    pub chunk_send_budget_per_tick: usize,
    /// Threads the terrain generator runs on. Zero means "one per core
    /// less two", leaving a core for the async runtime driving the
    /// sockets and one for the machine.
    ///
    /// Worth setting by hand in exactly two cases: a shared host, where
    /// the server should not take every core it can see; and a
    /// single-core box, where a value of 1 is what the default already
    /// clamps to.
    #[serde(default)]
    pub generator_threads: usize,
    /// Chunks generated around the spawn point at startup, as a radius
    /// in chunks. Zero disables it.
    ///
    /// The first player to join asks for this exact area and waits for
    /// it; making it while nobody is connected costs nothing and is the
    /// difference between walking into a world and watching one appear.
    #[serde(default = "default_pregenerate_radius")]
    pub pregenerate_radius_chunks: i32,
    /// Depth of a client's outgoing queue. A client that can't keep up
    /// fills this; see `slow_client_drop_threshold`.
    pub outgoing_queue_capacity: usize,
    /// Pending chunk requests queued per client before new ones are
    /// dropped (the client re-requests anything still missing).
    pub chunk_queue_capacity: usize,
    /// How many messages we're willing to drop for one client before
    /// deciding the connection is hopeless and closing it. Without this a
    /// single slow client would either stall the server (unbounded
    /// blocking send) or grow its queue forever (unbounded channel).
    pub slow_client_drop_threshold: u64,

    // ---- connection hygiene ----
    pub keepalive_interval_secs: f32,
    /// No traffic at all from a client for this long -> disconnect.
    pub client_timeout_secs: f32,
    /// Time a connection gets to complete its handshake before it's cut.
    pub handshake_timeout_secs: f32,
    /// Crude anti-DoS: simultaneous connections allowed from one IP.
    pub max_connections_per_ip: u32,

    // ---- persistence ----
    /// Directory for the block-edit overlay. Empty = don't persist.
    pub world_dir: String,
    /// Folder scanned for plugins at startup. Each subfolder with a
    /// `plugin.toml` is one plugin.
    pub plugin_dir: String,
    /// Where native mods live. See `logic::mods`.
    ///
    /// Its own directory rather than sharing the plugins' one, because
    /// they are different things with different manifests and different
    /// risks -- and because "drop this folder in and it runs native code
    /// in my server" deserves to be a deliberate act rather than a file
    /// that happened to land beside a script.
    #[serde(default = "default_mod_dir")]
    pub mod_dir: String,
    pub autosave_interval_secs: f32,

    // ---- observability ----
    /// How often to print the stats line (0 = never).
    pub stats_interval_secs: f32,

    pub anticheat: AntiCheatSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AntiCheatSettings {
    pub enabled: bool,
    /// Blocks per second a player may move horizontally. The client
    /// walks at 5.5 and *sprints at 8.8*, which is the number this has
    /// to leave room for; the rest of the headroom absorbs lag spikes
    /// and the occasional bunched-up update.
    ///
    /// The sprint is the half that gets forgotten, and forgetting it
    /// rubber-bands every running player on the server. The client-side
    /// test `the_fastest_an_honest_client_can_move_fits_inside_the_servers_limits`
    /// measures what the collider actually produces and holds it against
    /// this.
    pub max_horizontal_speed: f32,
    /// Downward speed is bounded by terminal velocity in the client's
    /// physics; upward is bounded by the jump impulse.
    pub max_vertical_speed: f32,
    /// A single position delta larger than this is a teleport, whatever
    /// the timing says.
    pub max_teleport_distance: f32,
    /// Total climb allowed while the client claims to be airborne before
    /// we call it flight. A legal jump gains about 1.5 blocks.
    pub max_airborne_ascent: f32,
    /// Seconds a client may claim to be airborne without losing height.
    pub max_hover_seconds: f32,
    /// Interaction range for breaking/placing, in blocks, measured from
    /// the player's eyes.
    ///
    /// The client reaches 4.0 (`INTERACT_RANGE`), and the gap is not
    /// slack for its own sake: this is measured to the *centre* of the
    /// cell, which is up to 0.87 further than the face the player
    /// clicked, from the last position the server accepted, which is up
    /// to a network update stale. The comment here used to say the
    /// client used 6.0, which it has not for some time -- and a wrong
    /// number in a comment beside a limit is an invitation to lower the
    /// limit onto honest players.
    pub max_reach: f32,
    /// Reject a claimed `on_ground` when the block underneath is air *and*
    /// we already have that chunk cached (we never generate a chunk just
    /// to run this check).
    pub verify_ground: bool,
    /// Horizontal limit of the playable world, in blocks.
    pub world_border: f32,

    // ---- rate limits (token buckets, per client) ----
    pub max_messages_per_sec: f32,
    /// **Sixty, and it was fifteen when a block was one edit.** A rock or
    /// a soil comes away a quarter at a time now (`primitive_shared::dig`),
    /// so a dig that used to send one message sends four -- and the fastest
    /// ground in the game with the right tool is well under half a second,
    /// which put an honest player with a shovel inside a bucket sized for
    /// one edit a block. Four times the number is the same number of
    /// *blocks* a second it always allowed.
    pub max_block_edits_per_sec: f32,
    pub max_chunk_requests_per_sec: f32,
    pub max_transform_updates_per_sec: f32,
    pub max_chat_per_sec: f32,

    /// Violation score at which the player is kicked. Each violation adds
    /// weight (a teleport is worth more than a slightly-too-fast step),
    /// and the score decays over time so one bad lag spike never
    /// accumulates into a ban.
    pub violation_kick_threshold: f32,
    pub violation_decay_per_sec: f32,
}

/// The spawn area, pre-made. Four chunks is a 9x9 block of terrain --
/// enough that a player is standing in finished world the instant they
/// arrive, small enough that it is a few dozen milliseconds of a pool
/// that has nothing else to do yet.
fn default_mod_dir() -> String {
    "mods".to_string()
}

fn default_pregenerate_radius() -> i32 {
    4
}

impl Default for ServerSettings {
    fn default() -> Self {
        Self {
            bind_addr: "0.0.0.0:7878".to_string(),
            server_name: "Primitive".to_string(),
            world_seed: 1337,
            world_preset: primitive_shared::worldgen::Preset::Normal,
            world_zone: primitive_shared::worldgen::Zone::Temperate,
            world_scale: primitive_shared::worldgen::Scale::Earth,
            max_players: 256,

            tick_rate_hz: 20.0,
            interest_radius_blocks: 160.0,
            view_distance_chunks: 8,
            day_length_seconds: 1800.0,
            start_time_of_day: 0.3,

            max_cached_chunks: 8192,
            chunk_send_budget_per_tick: 8,
            generator_threads: 0,
            pregenerate_radius_chunks: default_pregenerate_radius(),
            outgoing_queue_capacity: 512,
            chunk_queue_capacity: 1024,
            slow_client_drop_threshold: 256,

            keepalive_interval_secs: 5.0,
            client_timeout_secs: 30.0,
            handshake_timeout_secs: 10.0,
            max_connections_per_ip: 8,

            world_dir: "world".to_string(),
            plugin_dir: "plugins".to_string(),
            mod_dir: default_mod_dir(),
            autosave_interval_secs: 120.0,

            stats_interval_secs: 30.0,

            anticheat: AntiCheatSettings::default(),
        }
    }
}

impl Default for AntiCheatSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            max_horizontal_speed: 12.0,
            max_vertical_speed: 60.0,
            max_teleport_distance: 24.0,
            max_airborne_ascent: 4.0,
            max_hover_seconds: 3.0,
            max_reach: 5.5,
            verify_ground: true,
            world_border: 2_000_000.0,

            max_messages_per_sec: 200.0,
            max_block_edits_per_sec: 60.0,
            max_chunk_requests_per_sec: 400.0,
            max_transform_updates_per_sec: 40.0,
            max_chat_per_sec: 2.0,

            violation_kick_threshold: 12.0,
            violation_decay_per_sec: 0.5,
        }
    }
}

const SETTINGS_PATH: &str = "settings.toml";

impl ServerSettings {
    pub fn load_or_default() -> Self {
        let mut settings = match std::fs::read_to_string(SETTINGS_PATH) {
            Ok(text) => match toml::from_str::<Self>(&text) {
                Ok(settings) => {
                    println!("loaded {SETTINGS_PATH}");
                    settings
                }
                Err(e) => {
                    eprintln!("{SETTINGS_PATH} is invalid ({e}), using defaults");
                    Self::default()
                }
            },
            Err(_) => {
                let settings = Self::default();
                if let Ok(text) = toml::to_string_pretty(&settings) {
                    if std::fs::write(SETTINGS_PATH, text).is_ok() {
                        println!("wrote default {SETTINGS_PATH}");
                    }
                }
                settings
            }
        };
        settings.clamp();
        settings
    }

    /// A settings file is user input too. Nonsense values here would show
    /// up as division by zero or a runaway allocation, so clamp rather
    /// than trust.
    fn clamp(&mut self) {
        self.tick_rate_hz = self.tick_rate_hz.clamp(1.0, 120.0);
        self.view_distance_chunks = self.view_distance_chunks.clamp(1, 32);
        self.interest_radius_blocks = self.interest_radius_blocks.clamp(16.0, 4096.0);
        self.day_length_seconds = self.day_length_seconds.max(10.0);
        self.start_time_of_day = self.start_time_of_day.rem_euclid(1.0);
        self.max_players = self.max_players.clamp(1, 100_000);
        self.max_cached_chunks = self.max_cached_chunks.max(64);
        self.chunk_send_budget_per_tick = self.chunk_send_budget_per_tick.clamp(1, 256);
        self.generator_threads = self.generator_threads.min(64);
        self.pregenerate_radius_chunks = self.pregenerate_radius_chunks.clamp(0, 32);
        self.outgoing_queue_capacity = self.outgoing_queue_capacity.clamp(16, 65_536);
        self.chunk_queue_capacity = self.chunk_queue_capacity.clamp(16, 65_536);
        self.keepalive_interval_secs = self.keepalive_interval_secs.clamp(1.0, 300.0);
        self.client_timeout_secs = self
            .client_timeout_secs
            .clamp(self.keepalive_interval_secs * 2.0, 600.0);
        self.handshake_timeout_secs = self.handshake_timeout_secs.clamp(1.0, 120.0);
        self.max_connections_per_ip = self.max_connections_per_ip.max(1);
    }

    pub fn tick_duration(&self) -> std::time::Duration {
        std::time::Duration::from_secs_f32(1.0 / self.tick_rate_hz)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_roundtrip_through_toml() {
        let text = toml::to_string_pretty(&ServerSettings::default()).unwrap();
        let parsed: ServerSettings = toml::from_str(&text).unwrap();
        assert_eq!(parsed.bind_addr, ServerSettings::default().bind_addr);
        assert!(parsed.anticheat.enabled);
    }

    #[test]
    fn clamping_rejects_nonsense() {
        let mut s = ServerSettings {
            tick_rate_hz: 0.0,
            view_distance_chunks: 9999,
            max_players: 0,
            ..Default::default()
        };
        s.clamp();
        assert!(s.tick_rate_hz >= 1.0);
        assert!(s.view_distance_chunks <= 32);
        assert!(s.max_players >= 1);
    }

    #[test]
    fn partial_config_keeps_defaults_for_the_rest() {
        // `#[serde(default)]` means an operator can write a two-line
        // settings.toml without losing every other knob.
        let parsed: ServerSettings = toml::from_str("bind_addr = \"0.0.0.0:25565\"").unwrap();
        assert_eq!(parsed.bind_addr, "0.0.0.0:25565");
        assert_eq!(parsed.tick_rate_hz, ServerSettings::default().tick_rate_hz);
    }
}
