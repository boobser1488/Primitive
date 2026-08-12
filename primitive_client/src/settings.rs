//! Настройки клиента. Same pattern as the server: read `settings.toml`
//! next to the binary, write out defaults if it's missing.

use serde::{Deserialize, Serialize};

use primitive_shared::types::{
    block_name, BlockId, BLOCK_COBBLESTONE, BLOCK_DIRT, BLOCK_STONE,
};

/// Blocks offered as the menu wallpaper.
///
/// Three, not every block in the game. The point is a quiet backdrop, and
/// most of the list would not be one: grass and sand are too bright to
/// put small text on, water moves the eye, glowstone glows.
pub const MENU_BACKGROUND_BLOCKS: &[BlockId] = &[BLOCK_STONE, BLOCK_DIRT, BLOCK_COBBLESTONE];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ClientSettings {
    pub server_addr: String,
    /// Sent to the server in the handshake and shown to other players.
    pub username: String,
    pub window_width: u32,
    pub window_height: u32,
    pub vsync: bool,
    pub fov_degrees: f32,
    pub render_distance_chunks: i32,
    pub mouse_sensitivity: f32,
    pub move_speed: f32,
    /// How often (Hz) we send our own position/look to the server.
    pub player_update_hz: f32,

    // ---- fog ----
    pub fog_enabled: bool,
    /// Where fog starts, as a fraction of the render distance. 0.55 keeps
    /// the near field completely clear and only veils the outer ring.
    pub fog_start_ratio: f32,
    /// Where fog reaches full strength, as a fraction of the render
    /// distance. Slightly below 1.0 so terrain is fully faded *before* it
    /// reaches the edge of the loaded area -- otherwise chunks visibly pop
    /// in at the boundary instead of emerging from the haze.
    pub fog_end_ratio: f32,
    /// Underwater fog closes in to this many blocks.
    pub underwater_fog_distance: f32,

    // ---- lighting ----
    /// Floor light level so nothing is ever absolutely black.
    pub ambient_light: f32,
    /// Multiplier on block light (glowstone and friends).
    pub block_light_boost: f32,
    /// 0.0 disables ambient occlusion, 1.0 makes creases very dark.
    pub ambient_occlusion: f32,

    // ---- performance ----
    /// Worker threads for chunk meshing and lighting. 0 = use every
    /// core except one (the main thread still renders, runs physics and
    /// drives the network).
    pub worker_threads: usize,
    /// Milliseconds per frame the client may spend integrating newly
    /// arrived chunks (mostly lighting them). Separate from the mesh
    /// budget because they're separate stages with separate costs.
    pub chunk_budget_ms: f32,
    /// Milliseconds per frame the client may spend building chunk meshes.
    /// A budget rather than a fixed count, because mesh cost varies wildly
    /// between an empty sky chunk and a cave system.
    pub mesh_budget_ms: f32,

    // ---- singleplayer ----
    /// Folder that holds singleplayer worlds, one subfolder each. Kept
    /// separate from the standalone server's `world/` so running a
    /// server in the same folder doesn't overwrite your own worlds --
    /// which, since both read their config from the working directory,
    /// is easy to do by accident.
    pub singleplayer_world_dir: String,
    /// Seed offered when creating a new world, and used for a world
    /// carried over from before worlds had their own metadata. The seed
    /// of an existing world lives with the world, not here: it *is* the
    /// world, and changing it would drop the saved edits onto completely
    /// different terrain.
    pub singleplayer_seed: u32,
    /// How far the local server will stream. It can afford to be
    /// generous -- there is exactly one player and no network between
    /// them -- so this is above the server's default.
    pub singleplayer_view_distance_chunks: i32,

    // ---- menus ----
    /// Tile a block texture behind the menus.
    ///
    /// Off by default: a wall of texture behind small text costs
    /// legibility, and that is a trade the player should opt into rather
    /// than be handed on first launch.
    pub menu_background: bool,
    /// Which block to tile. Anything in `MENU_BACKGROUND_BLOCKS`;
    /// unknown names fall back to the first of them.
    pub menu_background_block: String,

    /// Where to look for `textures/blocks.toml` and the PNGs it
    /// references. Empty string = auto-detect (next to the executable,
    /// then the workspace path baked in at compile time).
    pub assets_dir: String,
    /// Print detailed per-second stats to the console. Toggle with F3.
    pub debug_overlay_on_start: bool,
}

impl Default for ClientSettings {
    fn default() -> Self {
        Self {
            server_addr: "127.0.0.1:7878".to_string(),
            username: "player".to_string(),
            window_width: 1280,
            window_height: 720,
            vsync: true,
            fov_degrees: 70.0,
            render_distance_chunks: 6,
            mouse_sensitivity: 0.0025,
            move_speed: 5.5,
            player_update_hz: 20.0,

            fog_enabled: true,
            fog_start_ratio: 0.55,
            fog_end_ratio: 0.95,
            underwater_fog_distance: 18.0,

            ambient_light: 0.06,
            block_light_boost: 1.0,
            ambient_occlusion: 0.45,

            worker_threads: 0,
            chunk_budget_ms: 3.0,
            mesh_budget_ms: 4.0,

            singleplayer_world_dir: "saves".to_string(),
            singleplayer_seed: 1337,
            singleplayer_view_distance_chunks: 10,

            menu_background: false,
            menu_background_block: MENU_BACKGROUND_BLOCKS[0].to_string(),

            assets_dir: String::new(),
            debug_overlay_on_start: false,
        }
    }
}

impl ClientSettings {
    /// Fog distances in blocks, derived from the render distance so a
    /// player who turns view distance up doesn't end up staring into a
    /// wall of fog with clear terrain behind it.
    pub fn fog_range(&self, render_distance_chunks: i32) -> (f32, f32) {
        let max_visible = (render_distance_chunks as f32) * 16.0;
        let start = (max_visible * self.fog_start_ratio).max(4.0);
        let end = (max_visible * self.fog_end_ratio).max(start + 8.0);
        (start, end)
    }

    /// Writes the settings back to disk.
    ///
    /// Called when the player leaves the settings screen, so there is no
    /// separate save step to forget and nothing that only takes effect
    /// after a manual file edit. Returns the error rather than logging
    /// it, because the settings screen has somewhere to show it.
    pub fn save(&self) -> Result<(), String> {
        let text = toml::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(SETTINGS_PATH, text)
            .map_err(|e| format!("could not write {SETTINGS_PATH}: {e}"))
    }

    /// The block tiled behind the menus.
    ///
    /// Falls back rather than failing: the name comes from a file a
    /// person can edit, and a typo in it should give a stone background
    /// and not a crash or an empty screen.
    pub fn menu_background_block(&self) -> BlockId {
        MENU_BACKGROUND_BLOCKS
            .iter()
            .copied()
            .find(|id| block_name(*id) == self.menu_background_block)
            .unwrap_or(MENU_BACKGROUND_BLOCKS[0])
    }

    /// Clamps every value into its supported range.
    ///
    /// Public because the settings screen edits these fields directly
    /// and has to re-clamp afterwards -- the same values arriving from a
    /// hand-edited file and from a button in the game deserve the same
    /// treatment, and duplicating the limits is how the two drift.
    pub fn sanitize(&mut self) {
        self.clamp();
    }

    /// Server settings for a singleplayer world.
    ///
    /// Derived from the server's own defaults rather than written out
    /// fresh, so a change to any of the dozens of knobs that don't differ
    /// between local and hosted play applies to both.
    ///
    /// Three things do differ. It binds to loopback on port 0, so the
    /// world is unreachable from the network and two copies of the game
    /// can run side by side. It has no plugin directory. And the
    /// anti-cheat is off: it exists to stop a client lying to a server it
    /// doesn't own, and here they are the same process -- leaving it on
    /// would only add the chance of rubber-banding the player for a
    /// physics disagreement with themselves.
    pub fn singleplayer_server(
        &self,
        world: &crate::worlds::World,
    ) -> primitive_server::settings::ServerSettings {
        primitive_server::settings::ServerSettings {
            bind_addr: "127.0.0.1:0".to_string(),
            server_name: world.name.clone(),
            // A world carried over from the old single-folder layout has
            // no recorded seed; fall back to the configured default,
            // which is what generated it back then.
            world_seed: world.seed.unwrap_or(self.singleplayer_seed),
            max_players: 4,
            view_distance_chunks: self.singleplayer_view_distance_chunks,
            world_dir: world.directory.display().to_string(),
            plugin_dir: String::new(),
            stats_interval_secs: 0.0,
            anticheat: primitive_server::settings::AntiCheatSettings {
                enabled: false,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    fn clamp(&mut self) {
        // Normalise the name so the settings screen and the file always
        // agree on what is selected.
        self.menu_background_block = block_name(self.menu_background_block()).to_string();
        self.render_distance_chunks = self.render_distance_chunks.clamp(1, 24);
        self.singleplayer_view_distance_chunks =
            self.singleplayer_view_distance_chunks.clamp(1, 32);
        self.fov_degrees = self.fov_degrees.clamp(30.0, 120.0);
        self.mouse_sensitivity = self.mouse_sensitivity.clamp(0.0001, 0.05);
        self.move_speed = self.move_speed.clamp(0.5, 20.0);
        self.player_update_hz = self.player_update_hz.clamp(1.0, 60.0);
        self.fog_start_ratio = self.fog_start_ratio.clamp(0.0, 0.95);
        self.fog_end_ratio = self.fog_end_ratio.clamp(self.fog_start_ratio + 0.05, 2.0);
        self.underwater_fog_distance = self.underwater_fog_distance.clamp(2.0, 200.0);
        self.ambient_light = self.ambient_light.clamp(0.0, 0.6);
        self.block_light_boost = self.block_light_boost.clamp(0.0, 3.0);
        self.ambient_occlusion = self.ambient_occlusion.clamp(0.0, 1.0);
        self.mesh_budget_ms = self.mesh_budget_ms.clamp(0.5, 33.0);
        self.chunk_budget_ms = self.chunk_budget_ms.clamp(0.5, 33.0);
        self.worker_threads = self.worker_threads.min(64);
        self.window_width = self.window_width.clamp(320, 7680);
        self.window_height = self.window_height.clamp(240, 4320);
    }
}

/// The client's own file, deliberately *not* `settings.toml`.
///
/// Both binaries read their config from the current working directory,
/// so running the server and the client from the same folder (which is
/// what `cargo run -p ...` does from the workspace root) had them
/// fighting over one `settings.toml`: whichever started last rewrote it
/// with its own defaults and the other one's settings vanished.
const SETTINGS_PATH: &str = "client_settings.toml";

/// The old shared name. Read once, for people upgrading, then left
/// alone -- a config file the user has edited shouldn't silently
/// disappear because the game renamed it.
const LEGACY_SETTINGS_PATH: &str = "settings.toml";

impl ClientSettings {
    pub fn load_or_default() -> Self {
        let mut settings = match std::fs::read_to_string(SETTINGS_PATH) {
            Ok(text) => Self::parse(&text, SETTINGS_PATH),
            Err(_) => match std::fs::read_to_string(LEGACY_SETTINGS_PATH) {
                Ok(text) => {
                    // Migrate: parse the old file, write the new one,
                    // and say so. The old file stays where it is.
                    println!(
                        "migrating settings from {LEGACY_SETTINGS_PATH} to {SETTINGS_PATH}"
                    );
                    let migrated = Self::parse(&text, LEGACY_SETTINGS_PATH);
                    migrated.write_defaults();
                    migrated
                }
                Err(_) => {
                    let settings = Self::default();
                    settings.write_defaults();
                    settings
                }
            },
        };
        settings.clamp();
        settings
    }

    fn parse(text: &str, path: &str) -> Self {
        match toml::from_str::<Self>(text) {
            Ok(settings) => {
                println!("loaded {path}");
                settings
            }
            Err(e) => {
                eprintln!("{path} is invalid ({e}), using defaults");
                Self::default()
            }
        }
    }

    fn write_defaults(&self) {
        if let Ok(text) = toml::to_string_pretty(self) {
            if std::fs::write(SETTINGS_PATH, text).is_ok() {
                println!("wrote {SETTINGS_PATH}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fog_scales_with_render_distance() {
        let s = ClientSettings::default();
        let (near_start, near_end) = s.fog_range(4);
        let (far_start, far_end) = s.fog_range(12);
        assert!(far_start > near_start && far_end > near_end);
        assert!(near_start < near_end, "fog must start before it ends");
    }

    #[test]
    fn a_nonsense_config_is_clamped_not_obeyed() {
        let mut s = ClientSettings {
            render_distance_chunks: 9999,
            fov_degrees: 0.0,
            fog_start_ratio: 5.0,
            mesh_budget_ms: -3.0,
            ..Default::default()
        };
        s.clamp();
        assert!(s.render_distance_chunks <= 24);
        assert!(s.fov_degrees >= 30.0);
        assert!(s.fog_end_ratio > s.fog_start_ratio);
        assert!(s.mesh_budget_ms > 0.0);
    }

    #[test]
    fn partial_config_keeps_defaults() {
        let parsed: ClientSettings = toml::from_str("username = \"shamkhan\"").unwrap();
        assert_eq!(parsed.username, "shamkhan");
        assert!(parsed.fog_enabled);
    }

    fn test_world() -> crate::worlds::World {
        crate::worlds::World {
            name: "Test".to_string(),
            seed: Some(77),
            directory: std::path::PathBuf::from("saves/test"),
            last_played: 0,
        }
    }

    #[test]
    fn a_singleplayer_world_is_not_reachable_from_the_network() {
        // Binding 0.0.0.0 would quietly turn "playing alone" into
        // "hosting a public server".
        let server = ClientSettings::default().singleplayer_server(&test_world());
        assert!(
            server.bind_addr.starts_with("127.0.0.1:"),
            "bound to {}",
            server.bind_addr
        );
        assert!(
            server.bind_addr.ends_with(":0"),
            "should let the OS pick the port so two copies can run at once"
        );
    }

    #[test]
    fn a_singleplayer_world_runs_no_plugins_and_no_anticheat() {
        let server = ClientSettings::default().singleplayer_server(&test_world());
        assert!(server.plugin_dir.is_empty());
        assert!(!server.anticheat.enabled);
    }

    #[test]
    fn the_world_supplies_its_own_seed_and_folder() {
        // Not the client config: two worlds must keep their own terrain.
        let server = ClientSettings::default().singleplayer_server(&test_world());
        assert_eq!(server.world_seed, 77);
        assert!(server.world_dir.contains("test"));
        assert_eq!(server.server_name, "Test");
    }

    #[test]
    fn a_world_with_no_recorded_seed_falls_back_to_the_configured_one() {
        // The layout from before worlds had metadata genuinely didn't
        // record a seed, and that default is what generated it.
        let client = ClientSettings {
            singleplayer_seed: 4242,
            ..Default::default()
        };
        let legacy = crate::worlds::World {
            seed: None,
            ..test_world()
        };
        assert_eq!(client.singleplayer_server(&legacy).world_seed, 4242);
    }

    #[test]
    fn worlds_are_saved_somewhere_of_their_own() {
        // The standalone server defaults to `world/` in the working
        // directory, and the client is normally run from the same one.
        let client = ClientSettings::default();
        let server_default = primitive_server::settings::ServerSettings::default();
        assert_ne!(client.singleplayer_world_dir, server_default.world_dir);
        assert!(!client.singleplayer_world_dir.is_empty());
    }

    #[test]
    fn an_unknown_background_block_falls_back_instead_of_failing() {
        // The name comes from a file a person can edit. A typo should
        // give a stone wall, not an empty screen.
        let mut s = ClientSettings {
            menu_background_block: "cheese".to_string(),
            ..Default::default()
        };
        assert_eq!(s.menu_background_block(), MENU_BACKGROUND_BLOCKS[0]);
        s.sanitize();
        assert_eq!(s.menu_background_block, "stone", "the file should be corrected");
    }

    #[test]
    fn the_menu_background_is_off_until_asked_for() {
        // A wall of texture behind small text is a legibility cost, and
        // one the player should choose.
        assert!(!ClientSettings::default().menu_background);
    }

    #[test]
    fn settings_edited_in_game_are_clamped_the_same_way_a_file_is() {
        // The settings screen writes these fields directly, so nonsense
        // typed into it has to be caught by the same limits.
        let mut s = ClientSettings {
            render_distance_chunks: 999,
            fov_degrees: 5.0,
            mouse_sensitivity: 100.0,
            ..Default::default()
        };
        s.sanitize();
        assert!(s.render_distance_chunks <= 24);
        assert!(s.fov_degrees >= 30.0);
        assert!(s.mouse_sensitivity <= 0.05);
    }
}

#[cfg(test)]
mod file_tests {
    use super::*;

    #[test]
    fn the_client_and_server_no_longer_share_a_filename() {
        // Regression: both binaries used "settings.toml" in the working
        // directory, so running them from the same folder meant each
        // overwrote the other's config with its own defaults.
        assert_ne!(SETTINGS_PATH, "settings.toml");
        assert_eq!(SETTINGS_PATH, "client_settings.toml");
    }

    #[test]
    fn an_old_settings_file_still_parses() {
        // Migration path: the legacy file is read with the same parser,
        // so an upgrading player keeps their tweaks.
        let legacy = "server_addr = \"10.0.0.5:7878\"\nfov_degrees = 95.0\n";
        let parsed = ClientSettings::parse(legacy, LEGACY_SETTINGS_PATH);
        assert_eq!(parsed.server_addr, "10.0.0.5:7878");
        assert_eq!(parsed.fov_degrees, 95.0);
    }

    #[test]
    fn a_broken_file_falls_back_instead_of_refusing_to_start() {
        let parsed = ClientSettings::parse("this is not toml {{{", SETTINGS_PATH);
        assert_eq!(parsed.server_addr, ClientSettings::default().server_addr);
    }
}
