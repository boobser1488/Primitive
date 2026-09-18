//! Настройки клиента. Same pattern as the server: read `settings.toml`
//! next to the binary, write out defaults if it's missing.

use serde::{Deserialize, Serialize};

use crate::logic::menu_scene::Place;

/// What `menu_background_scene` says when the player has not picked a
/// place and wants a different one each launch.
///
/// A name rather than an empty string, because the settings row shows
/// this value and "" is not something to show anybody.
pub const MENU_SCENE_ANY: &str = "random";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ClientSettings {
    pub server_addr: String,
    /// Sent to the server in the handshake and shown to other players.
    pub username: String,
    pub window_width: u32,
    pub window_height: u32,
    /// Start in fullscreen, and stay there next time.
    ///
    /// Borderless rather than exclusive: exclusive changes the display
    /// mode, which on Windows means a black flicker on every alt-tab
    /// and a real chance of leaving the desktop at the wrong resolution
    /// if the game exits badly. Borderless is a window the size of the
    /// screen with no frame, which is what "fullscreen" means to almost
    /// everybody who asks for it.
    #[serde(default)]
    pub fullscreen: bool,
    pub vsync: bool,
    pub fov_degrees: f32,
    pub render_distance_chunks: i32,
    pub mouse_sensitivity: f32,
    /// How hard the view rises, falls and rolls in step with the
    /// player's feet: 0 is off, 1 is the full effect.
    ///
    /// **A setting because this is the one effect in the game people
    /// have opposite and equally strong opinions about.** A walk with a
    /// perfectly still head is what makes a first-person game read as a
    /// camera on rails; a walk that heaves is what makes some people
    /// put the game down after ten minutes, and they are not wrong
    /// either. Neither default satisfies both, so the number itself is
    /// the answer.
    ///
    /// **Not on the settings screen, on purpose.** It is a row that
    /// would be read as a graphics option and fiddled with by everyone,
    /// and the two players it is for -- the one who wants it off and
    /// the one who wants it doubled -- are both people who will happily
    /// edit a file. See `ClientSettings::clamp`, which treats this the
    /// way it treats every other hand-edited number.
    ///
    /// It never reaches the aim. See `logic::shake` and
    /// `engine::camera::Camera::shake`.
    pub view_bob: f32,
    pub move_speed: f32,
    /// How often (Hz) we send our own position/look to the server.
    pub player_update_hz: f32,

    // ---- fog ----
    pub fog_enabled: bool,
    /// Where the fog begins, as a share of how far the world is streamed.
    /// It is complete exactly where the world ends; see `fog_range`.
    ///
    /// **A new name, because the old pair meant something else and every
    /// file carried it.** `fog_start_ratio = 0.55` and `fog_end_ratio =
    /// 0.95` were shares of `render distance x 16`, and at a render distance
    /// of 24 they tinted the world from 211 blocks out -- thirteen chunks of
    /// twenty-four, which is the report "24 looks like 10 to 15, and the fog
    /// hides half of it". Every settings file ever saved holds that pair, so
    /// a new default under the old names would have reached nobody (the
    /// trap `move_speed` describes). Under a new name the old keys are
    /// ignored, and a file that has them opens on this fog.
    pub fog_start_share: f32,
    /// Underwater fog closes in to this many blocks.
    pub underwater_fog_distance: f32,

    // ---- sound ----
    /// Everything, together. The one a player reaches for when the game
    /// is too loud next to whatever else they have running.
    ///
    /// Below one by default, because the first thing a game should not
    /// do is be the loudest thing on the machine.
    pub master_volume: f32,
    /// The composer. Separately, and quieter by default, because it is
    /// the part people turn off -- and a music slider that is really a
    /// master volume is the thing that makes them turn the game off
    /// instead.
    pub music_volume: f32,

    // ---- lighting ----
    /// Floor light level: what a surface is lit by when nothing is
    /// lighting it.
    ///
    /// **This is the whole of what a mine looks like without fire.** A
    /// cave has no skylight by construction, so the sun term and the sky
    /// fill are both zero down there and this number is the only thing
    /// left in `shade`. At 0.06 a torchless shaft was legible -- you
    /// could mine by it -- which quietly made every lamp in the game
    /// decoration. At 0.02 it is not: stone at 78 comes out at about
    /// one and a half levels, which is black with a hint of shape in it.
    ///
    /// Kept as a setting, and this is the setting it is kept for. A
    /// player on a bright screen, or one who simply does not want to
    /// play in the dark, raises it and nothing else in the game
    /// changes. See `NIGHT_INTENSITY`, which was lowered with it.
    pub ambient_light: f32,
    /// Multiplier on block light (glowstone and friends).
    pub block_light_boost: f32,
    /// 0.0 disables ambient occlusion, 1.0 makes creases very dark.
    pub ambient_occlusion: f32,
    /// Anisotropic filtering, as a sample count: 1 turns it off.
    ///
    /// Only the *minification* end of filtering changes with it --
    /// magnification stays nearest-neighbour, so a block you are
    /// standing next to keeps its hard texel edges. What it cures is the
    /// crawl on faces seen edge-on at a distance.
    pub anisotropy: u16,
    /// Multisampling of the world, as a sample count: 1 turns it off;
    /// 2, 4 and 8 are the sizes a GPU offers.
    ///
    /// **What it cures is the dots.** A block whose texture differs
    /// from its neighbour's meets it at an edge where a sliver of a
    /// face is seen nearly edge-on -- the side of a plank against a
    /// cobble wall, a doorway jamb, the risers of a stair. Such a
    /// sliver is narrower than a pixel, and a rasteriser samples each
    /// pixel once, at its centre: the sliver hits some centres and
    /// misses others, and what it draws is a broken one-pixel line
    /// that pops in and out as the camera moves. It is not a crack
    /// in the mesh (the mesher's T-junction test says there is none)
    /// and no amount of texture filtering reaches it, because it is
    /// the *geometry* being sampled too coarsely. Several samples per
    /// pixel, averaged, is the one cure: the sliver contributes a
    /// fraction of a pixel wherever it is, instead of a whole pixel
    /// wherever it happens to be hit.
    ///
    /// Anisotropy is the sibling setting and the two answer different
    /// questions: that one is how the *texture* is read inside a
    /// face, this one is how a face's *edge* lands on the pixel grid.
    /// A flat single-texture world shows none of this and pays for
    /// it all the same, which is why it is a setting.
    ///
    /// Only what the adapter offers is used: an unsupported count
    /// falls back to the largest one supported, and the F3 line shows
    /// the count in force rather than the one asked for.
    #[serde(default = "default_msaa")]
    pub msaa: u32,
    /// How much smaller than the frame the sky is drawn, before being
    /// stretched back over it. 1 draws it at full size.
    ///
    /// The sky is the single most expensive thing in the frame -- with
    /// the horizon in view it measured 0.68 ms of a 1.25 ms one -- and
    /// every attempt to make its shader cheaper failed on measurement.
    /// Giving it fewer pixels is the one saving left, and it cannot
    /// fail, because it is less work rather than faster work: a quarter
    /// of the pixels at 2, a ninth at 3.
    ///
    /// What softens is stars and the rims of the sun and the moon. The
    /// gradient and the clouds barely notice -- the clouds are already
    /// drawn on a grid about twelve blocks square, far coarser than a
    /// pixel.
    ///
    /// **On by default at 3.** The sky is the largest single item in
    /// the frame and three attempts to make its shader cheaper were
    /// measured and failed, one of them making it a third slower.
    /// Fewer pixels is the only saving there is. Measured on the
    /// benchmark world, twenty-five samples each:
    ///
    /// ```text
    /// sky_scale=1   930 fps   gpu 0.759   sky 0.253
    /// sky_scale=2   997 fps   gpu 0.676   sky 0.186
    /// sky_scale=3  1087 fps   gpu 0.599   sky 0.103
    /// ```
    ///
    /// Three rather than two because the stars stopped being the reason
    /// not to: they are grid-aligned squares now (see `sky.wgsl`), and a
    /// square survives being drawn small and stretched in a way a
    /// sub-pixel point never could. `sky_scale = 1` puts it back.
    pub sky_scale: u32,

    /// How much bigger the interface is drawn than it was authored.
    ///
    /// **A physical size, not a pixel count, and that is why a number
    /// of pixels cannot decide it.** The interface is laid out as a
    /// fraction of the screen's height, so a button is about the same
    /// number of pixels on a 1080p monitor and on this phone -- 1080
    /// against 1220. What differs is that one of those screens is
    /// twenty-four inches across and the other is six, so the same
    /// button is a third of the size in the hand that is meant to
    /// press it.
    ///
    /// One is the size it was drawn at, and stays the default on a
    /// desktop. A touch platform starts at two, which is roughly what
    /// closes the gap -- and it is a setting rather than a constant
    /// because "roughly" is doing real work in that sentence: screens
    /// and eyes differ, and the player is the one holding both.
    pub ui_scale: f32,
    /// How far out, in chunks, leaves are drawn see-through; past it a
    /// canopy is a solid shell drawn in the opaque pass.
    /// `lod::LEAVES_SEE_THROUGH_EVERYWHERE` is no limit, zero is solid
    /// everywhere. See `lod::leaves_see_through_at` for why the line is
    /// decided at the mesher.
    ///
    /// **This was a switch, and the switch's note is kept below**, because
    /// every word of it is still true of one chunk either side of the
    /// line. What changed is that "on" was a distance already -- it hid a
    /// line at 0.45 of the view distance, past which the renderer drew
    /// leaves solid anyway -- and a distance the player cannot see is one
    /// they cannot move. A solid chunk is now also meshed without the
    /// insides of its crowns, which the old far half always drew and never
    /// showed.
    ///
    /// **Six by default.** A leaf picture is thirty-two texels to a block
    /// and a hole in it two or three; at ninety-five degrees and 1080 lines
    /// a block is about 500/d pixels tall at d blocks, so a hole is under a
    /// pixel from about 45 blocks and the mip chain has averaged it into
    /// the leaf colour by a hundred -- six chunks. Nearer, the holes are
    /// what a canopy looks like; further, they are the sparkle of a
    /// sub-pixel alpha test, the flicker of distant gaps this setting was
    /// asked for to remove.
    ///
    /// Written as `transparent_leaves = false` by versions before this,
    /// which `clamp` reads as zero: a player who turned the canopy solid
    /// keeps it solid.
    ///
    /// The switch's note, as it was:
    ///
    /// On, a leaf texture's holes are cut out and you can see through a
    /// canopy into the tree. That costs the GPU early depth rejection on
    /// every draw the cutout shader touches -- and a canopy is layer
    /// over layer of fragments, each shaded before the depth test throws
    /// it away, which is why standing under one is the slowest place in
    /// the world.
    ///
    /// Off, leaves are solid. The tree is a shape rather than a texture
    /// with gaps, and the pass gets its early-Z back. It is the classic
    /// detail-versus-speed setting, and it is offered for the classic
    /// reason: on a weak machine it is worth several tens of frames a
    /// second, and that is the sort of thing a player should get to
    /// decide.
    ///
    /// **Measured, and it is not free in the direction you would
    /// expect.** On a GTX 1050 Ti at 1920x1080, render distance 24,
    /// turning it *off* moved the solid pass from 1.78 ms to 1.91 ms
    /// and the cutout pass from 0.137 ms to 0.053 ms -- a net GPU loss
    /// of about 0.2 ms. The triangle count does not change; the same
    /// leaf faces simply move from the cutout pipeline to the solid one
    /// (see `renderer::render`, where `leaf_cutout_limit` goes to -1),
    /// and on that card they are *dearer* there than the early-Z is
    /// worth. Where it pays is a weaker part, and standing inside a
    /// canopy rather than looking across a valley -- so the setting
    /// stays and the note is here so the next person measures their own
    /// machine instead of assuming the argument above settles it.
    #[serde(default = "default_transparent_leaves_chunks")]
    pub transparent_leaves_chunks: i32,
    /// The old switch, read from a file written before the distance and
    /// never written back. See `transparent_leaves_chunks`.
    #[serde(default, skip_serializing)]
    pub transparent_leaves: Option<bool>,
    /// How far out, in chunks, stones, sticks and flint lying on the ground
    /// keep their thickness (`engine::relief`); past it each is the flat
    /// quad it used to be. **Zero is flat everywhere**, which is what the
    /// game drew before they had any.
    ///
    /// Four by default, which is the line `lod::RELIEF_CHUNKS` measured:
    /// a stone's rim is under a pixel and a half at forty blocks. Further
    /// costs memory for thickness nobody sees -- a world given it
    /// everywhere held 132 MB more -- so the stops end at eight; nearer,
    /// or none, is for a machine where the extra forty quads a stone are
    /// felt. Grass is not in this: a tuft was never flat.
    #[serde(default = "default_relief_chunks")]
    pub relief_chunks: i32,
    /// Shadows cast by the sun and by fire: Off, Hard or Soft.
    ///
    /// The sun's are a depth picture of the world around the player taken
    /// from the sun's side; they lengthen toward evening, go at night and
    /// thin under cloud (`engine::shadow`). A fire's are its light walked
    /// cell by cell to what it lights, so a pillar by a hearth throws a
    /// spoke of shadow across the floor (`engine::lamp_shadow`). The step
    /// is the edge of both -- see `shadow::Mode`.
    ///
    /// **Off by default, and off costs nothing** -- no pass, no texture,
    /// and the terrain draws through exactly the pipelines it had before
    /// the setting existed. On, it is a second drawing of the nearby
    /// terrain on every frame the sun is up and a walk to the nearest fires
    /// on every pixel they light, which on a weak card or a phone is a
    /// price a player should choose to pay rather than find they are
    /// paying. A file written before the setting existed opens with it
    /// off, which is what the game looked like then; one written while it
    /// was a switch opens with `true` as Soft, the shadows it had.
    #[serde(default)]
    pub shadows: crate::engine::shadow::Mode,
    /// How far from the eye the sun's shadows reach, in blocks.
    ///
    /// **A setting now, and it was a constant** (`shadow::RADIUS`): "добавь
    /// настройку дальности теней". The picture has one resolution whatever
    /// it covers, so this is a trade a player makes with their eyes rather
    /// than with their frame rate alone -- nearer is a crisper edge on the
    /// things beside them, further is shadow on the hill across the valley
    /// at a softer edge. The steps are in `SHADOW_DISTANCES`.
    #[serde(default = "default_shadow_distance")]
    pub shadow_distance: f32,
    /// Which plants throw the sun's shadow: none, the trees' crowns, or the
    /// grass, flowers and crops as well ("добавь настройку теней от
    /// растений"). See `shadow::PlantShadows` for what each costs.
    ///
    /// **Trees by default**, in a file from before the row too: it is what
    /// the game drew then, and a setting that changes the picture on its own
    /// the day it appears is a regression report.
    #[serde(default)]
    pub plant_shadows: crate::engine::shadow::PlantShadows,
    /// How much colour the light carries: Simple, Balanced or High. See
    /// `engine::lighting` for what each step does and how it reaches the
    /// shader.
    ///
    /// **Simple until the player picks otherwise**, on a fresh install and
    /// in a file from before the row -- the rule the two switches above
    /// follow, and for their reason: the warmer steps were measured at up
    /// to 0.7 ms a frame on a GTX 1050 Ti (see `lighting::default_quality`),
    /// which is a price a player should choose to pay rather than find
    /// they are paying.
    #[serde(default)]
    pub lighting: crate::engine::lighting::Quality,
    /// How far out the small stuff is drawn, as a fraction of the fog
    /// distance.
    ///
    /// Tufts of grass and loose stones are the densest thing in the
    /// world -- a plains chunk is a tuft every three columns -- and the
    /// least worth drawing at range, where each one is a couple of
    /// pixels the fog is already washing out. Cutting them short is the
    /// single biggest lever on frame rate in open country.
    ///
    /// 1.0 draws them as far as anything else. Below about three quarters
    /// the line where they stop lies in clear air, because the fog does
    /// not begin until three quarters of the streamed reach (`fog_range`).
    /// This said "below about 0.4" while the fog began at 55% of the
    /// distance.
    #[serde(default = "default_detail_distance")]
    pub detail_distance: f32,
    /// How far out chunks are meshed out of bigger blocks, in chunks.
    ///
    /// The first coarse band starts here and merges 2x2 columns; the
    /// second starts at twice this and merges 4x4. **0 turns it off**,
    /// and that is a real answer: a player who would rather have the
    /// triangles than the frames should be able to say so, and on a
    /// small render distance the first threshold is never reached
    /// anyway.
    ///
    /// Ten by default, against a stock render distance of 24: a hundred
    /// and sixty blocks out, which is past everything a player builds
    /// or fights in and short enough that most of the world is in a
    /// coarse band. See `engine::lod` for what it costs to look at.
    #[serde(default = "default_lod_distance")]
    pub lod_distance_chunks: i32,
    /// How much a coarse chunk is allowed to give up.
    ///
    /// The other half of the setting above, and the one that decides
    /// what the simplification *looks* like rather than where it
    /// starts. Three steps, because the decision is "I would rather
    /// have the picture" or "I would rather have the frames" -- see
    /// `engine::lod::Quality`, which carries the measurements each step
    /// is made of.
    ///
    /// Ignored entirely while `lod_distance_chunks` is zero: with the
    /// simplification off there is no coarse chunk to have an opinion
    /// about.
    #[serde(default)]
    pub lod_quality: crate::engine::lod::Quality,
    /// How much of the sky the cloud layer covers, 0 (clear) to 1
    /// (overcast). Weather is not simulated, so this is a dial rather
    /// than something the world decides.
    #[serde(default = "default_cloudiness")]
    pub cloudiness: f32,
    /// What language the interface is in. See `ui::lang`.
    ///
    /// Defaults to English rather than to the system locale: guessing
    /// wrong puts a player into a language they cannot read *and*
    /// cannot navigate out of, and the settings row that fixes it is
    /// two screens in. English until asked is the recoverable mistake.
    #[serde(default)]
    pub language: crate::ui::lang::Language,
    /// What each key does. See `keybinds`.
    #[serde(default)]
    pub keybinds: crate::ui::keybinds::Keybinds,

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
    // **There is no singleplayer view distance any more**, and a file that
    // still carries `singleplayer_view_distance_chunks` opens fine: the
    // struct is `serde(default)` and an unknown key is ignored. See
    // `singleplayer_server` for why it had to go.

    // ---- menus ----
    /// Generate a patch of world and stand the menus in front of it.
    ///
    /// **On by default, which it was not when the backdrop was
    /// wallpaper.** The old default was off because a wall of tiled
    /// stone behind small text bought nothing and cost legibility. What
    /// is behind the menus now is a real place -- see
    /// `logic::menu_scene` -- and it is the first thing the game says
    /// about itself, so it is worth the twenty-five chunks. The switch
    /// stays because the cost is real on a weak phone, and switching it
    /// off is the difference between building a scene and not.
    pub menu_background: bool,
    /// Which kind of place: a name from `menu_scene::Place`, or
    /// `MENU_SCENE_ANY` for a different one each launch.
    ///
    /// **This row used to choose a block**, back when the backdrop was
    /// one texture tiled across the screen. A block is not a thing to
    /// choose about a landscape, so the row now chooses the landscape.
    /// An old file naming a block parses and lands on `random` -- see
    /// `menu_background_place` and the test that opens one.
    pub menu_background_scene: String,

    /// Where to look for `textures/blocks.toml` and the PNGs it
    /// references. Empty string = auto-detect (next to the executable,
    /// then the workspace path baked in at compile time).
    pub assets_dir: String,
    /// Print detailed per-second stats to the console. Toggle with F3.
    pub debug_overlay_on_start: bool,

    /// Where the thumb controls sit. See [`TouchLayout`].
    ///
    /// **This was the missing half of an arrangeable layout.** The type
    /// existed, with a `Default` written out so RESET had somewhere to
    /// go back to and a `same_as` so the screen could tell a changed
    /// arrangement from an untouched one -- and nothing ever stored one.
    /// The frame loop asked for `TouchLayout::default()` outright, so a
    /// player could not move a button and could not have been given the
    /// screen to move it on, because there was nowhere for the answer
    /// to live.
    #[serde(default)]
    pub touch_layout: TouchLayout,
}

/// Where the thumb controls sit and how big they are, written down as
/// fractions rather than pixels.
///
/// Fractions of the **shorter** side, measured inward from a corner.
/// Three separate reasons, and each one is a way this has gone wrong
/// before:
///
///   * Pixels do not survive a rotation. The same phone is a tall
///     screen and a wide one depending on how it is held, and a button
///     written down as "1900 px from the left" is a button off the
///     glass the first time it is turned.
///   * The shorter side rather than the window, because a phone held
///     sideways is twice as wide as it is tall. A thumb ring sized
///     against the long side is a ring the size of a fist.
///   * Inward from a corner rather than from the origin, because that
///     is what the player is actually arranging: how far the button is
///     from the corner their hand is wrapped around. Which corner is
///     part of the placement, so a button put on the right stays on
///     the right when the phone is turned.
///
/// The defaults reproduce, exactly, the layout that was computed in
/// code before this was arrangeable, so a player who never opens the
/// editor sees no change at all.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(default)]
pub struct TouchLayout {
    pub stick: Placement,
    /// One entry per `touch::Control`, in `Control::ALL` order.
    #[serde(deserialize_with = "buttons_of_any_length")]
    pub buttons: [Placement; TouchLayout::BUTTONS],
}

/// Reads the buttons out of a file that has more or fewer of them.
///
/// **A file saved before a button was added must still load.** serde reads
/// `[Placement; N]` as exactly N entries, so the day this array grew its
/// seventh member -- the journal's MAP -- every arrangement a player had
/// saved stopped parsing; and a settings file that does not parse is
/// replaced by defaults whole, which takes the language, the keys and the
/// sensitivity down with the buttons. So the list is read as a list and
/// laid over the arrangement that ships: what the file names keeps the
/// player's placement, and what it does not name comes out where it ships.
fn buttons_of_any_length<'de, D>(deserializer: D) -> Result<[Placement; TouchLayout::BUTTONS], D::Error>
where
    D: serde::Deserializer<'de>,
{
    let listed: Vec<Placement> = Vec::deserialize(deserializer)?;
    let mut buttons = TouchLayout::default().buttons;
    for (slot, placement) in buttons.iter_mut().zip(listed) {
        *slot = placement;
    }
    Ok(buttons)
}

/// The corner a placement is measured from.
///
/// Not a decoration: it is what makes an arrangement mean the same
/// thing on a screen of another shape. A button two thumbs in from the
/// right edge is two thumbs in from the right edge on any phone; the
/// same button written as "nine tenths of the way across" drifts over
/// the glass as the screen gets wider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Corner {
    BottomLeft,
    BottomRight,
    TopLeft,
    TopRight,
}

impl Corner {
    /// Whether the horizontal inset counts from the left edge.
    pub fn counts_from_left(self) -> bool {
        matches!(self, Corner::BottomLeft | Corner::TopLeft)
    }

    /// Whether the vertical inset counts from the top edge.
    pub fn counts_from_top(self) -> bool {
        matches!(self, Corner::TopLeft | Corner::TopRight)
    }
}

/// What pressing a button on the glass sends.
///
/// **A key, not an action.** A button carries the key it emulates, and
/// the game below it never learns the press came from a thumb -- it
/// arrives through the same door a keyboard's does, at the same moment,
/// with the same release afterwards. That is what makes this worth
/// having: every binding the game already has works on a phone without
/// being taught to, and so does every one it grows later, and so does
/// anything a mod binds.
///
/// The cost, and it is a real one: a button emits `Space`, not "jump".
/// Rebind jump to G and a button set to `Space` stops jumping, exactly
/// as a keyboard with SPACE printed on it would. That is the honest
/// behaviour for something called an on-screen *key*, and the editor
/// shows the key's name on the button so the player can see what it is
/// rather than having to remember.
///
/// Mining and placing are the two exceptions and cannot be keys: they
/// are mouse buttons, and there is no key on any board that does them.
///
/// ## Why there is no button that changes with what you are looking at
///
/// It was asked for, and the complaint behind it is real: on glass a
/// short tap in the look area is both "put a block down" and "use the
/// thing in front of me", and which one a player gets depends on what
/// is under the crosshair. One button that said OPEN at a chest and
/// LIGHT at a hearth would take the guessing away.
///
/// It was not built, and the reason is that the ambiguity is not the
/// phone's. A mouse has exactly the same one: the right button is
/// use-or-place and the *server* decides which, from the block. A
/// thumb button that sent anything else would be a second set of rules
/// for touching the world, living on one platform -- which is the one
/// thing the note at the top of `platform::touch` says must not happen:
/// a control scheme nobody can test daily stays working only by
/// producing exactly the state a keyboard and mouse produce. If the
/// ambiguity is worth removing it is worth removing in
/// `primitive_shared`, for both hands at once.
///
/// What would genuinely help a thumb is the *label* -- knowing before
/// the tap that it is about to open a chest rather than place a plank.
/// That needs a name for every block worth interacting with, and names
/// are translated: they live in `ui::lang`, which is not somewhere a
/// control scheme should be adding entries to on its own initiative.
/// That is the shape the feature should come back in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Emits {
    /// The left mouse button, held.
    ///
    /// The default because a `Placement` needs one and mining is the
    /// first thing anybody does in this game.
    #[default]
    Mine,
    /// The right mouse button.
    Place,
    /// A key, pressed when the thumb lands and released when it lifts.
    Key(crate::platform::Key),
    /// Opens the wheel: the buttons marked [`Placement::in_wheel`] come
    /// out around this one, and go away again when one is pressed.
    ///
    /// **Not a key, and it cannot be one.** Everything else here is a
    /// thing the game already understands arriving through the door a
    /// keyboard uses; this one is about the glass itself -- which
    /// controls are on it -- and there is nothing downstream to send.
    /// It is the price of the wheel: one `Emits` the game's own event
    /// path ignores. See `platform::touch::Touch::wheel_is_open`.
    More,
}

impl Emits {
    /// What is printed on the button.
    ///
    /// **What it does, where the key name would not say it.** The rule
    /// used to be "the key's own label", on the argument that the key
    /// is the one name a player already knows -- and that is true of a
    /// player with a keyboard. On glass there is no keyboard to know:
    /// `ESC` is not a thing a phone has, and the two keys a touch
    /// player most needs are exactly the two whose names mean nothing
    /// to them. So the buttons that stand for a *place to go* are
    /// named for the place.
    ///
    /// A word rather than a picture, still: a glyph would need
    /// inventing for every key and the font carries ASCII, Cyrillic and
    /// the Polish diacritics and nothing to invent one out of. See
    /// `texture::GLYPHS`.
    ///
    /// Anything else falls back to the key's name, which is right for
    /// the keys a player chose themselves.
    pub fn label(self) -> &'static str {
        use crate::platform::Key;
        match self {
            Emits::Mine => "MINE",
            Emits::Place => "PLACE",
            Emits::Key(Key::Escape) => "MENU",
            Emits::Key(Key::Enter) => "CHAT",
            Emits::Key(Key::Space) => "JUMP",
            Emits::Key(Key::KeyI) => "PACK",
            Emits::Key(Key::KeyM) => "MAP",
            // **"SHIFT" rather than the key's own "L SHIFT".** There is
            // no left and right shift on glass -- there is one button --
            // and a name that says there are two is a name that makes a
            // player look for the other. The word itself stays English
            // like the rest of this row; it is the one modifier every
            // phone keyboard also calls SHIFT, so it is not a word only
            // a desktop player knows. See [`Emits::label`] on why a
            // picture was not an option.
            Emits::Key(Key::ShiftLeft) => "SHIFT",
            // Three dots, because that is what the gesture is: there
            // is more here, and it is not worth a button each. The
            // font has a full stop and no ellipsis glyph -- see
            // `texture::GLYPHS` -- so it is three of them.
            Emits::More => "...",
            Emits::Key(key) => crate::ui::keybinds::key_name(key),
        }
    }
}

/// One control: where its middle is, how big it is, and whether it is
/// on the glass at all.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(default)]
pub struct Placement {
    pub corner: Corner,
    /// Middle of the control, in from `corner` on each axis.
    pub inset: (f32, f32),
    pub width: f32,
    pub height: f32,
    /// Whether it is drawn and can be pressed.
    ///
    /// A hidden control is not a deleted one: its place, size and key
    /// are kept, so switching it back on brings it back as it was
    /// rather than as a default the player has to set up again.
    pub shown: bool,
    /// What this button sends. Ignored for the stick, which sends a
    /// direction rather than a press.
    #[serde(default)]
    pub emits: Emits,
    /// Whether this button lives inside the wheel rather than on the
    /// glass.
    ///
    /// A wheel member is drawn and pressed only while the wheel is
    /// open, and **its own `corner` and `inset` are not read**: the
    /// wheel decides where its members go, because the one thing that
    /// makes a wheel a wheel is that the members are arranged around
    /// its middle. The size is still the member's own, so a player who
    /// wants a bigger MENU gets one.
    ///
    /// This is what the wheel buys: three buttons that a player wants
    /// a few times an hour used to stand along the top edge for the
    /// whole game. See `platform::touch::Layout::for_size`.
    #[serde(default)]
    pub in_wheel: bool,
}

impl Default for Placement {
    fn default() -> Self {
        Self {
            corner: Corner::BottomRight,
            inset: (0.15, 0.15),
            width: 0.18,
            height: 0.18,
            shown: false,
            emits: Emits::Mine,
            in_wheel: false,
        }
    }
}

impl Placement {
    /// Half the width and half the height, which is what every
    /// hit-test and every clamp actually wants.
    pub fn half(&self) -> (f32, f32) {
        (self.width / 2.0, self.height / 2.0)
    }

    /// Whether two placements are the same one. See
    /// [`TouchLayout::same_as`] for why the comparison is exact.
    #[allow(clippy::float_cmp)]
    fn same_as(&self, other: &Self) -> bool {
        self.corner == other.corner
            && self.inset == other.inset
            && self.width == other.width
            && self.height == other.height
            && self.shown == other.shown
            && self.emits == other.emits
            && self.in_wheel == other.in_wheel
    }
}

impl Default for TouchLayout {
    fn default() -> Self {
        // The layout the game shipped with, which used to be worked out
        // in code: a ring 0.16 of the short side across with a 0.06
        // margin, and a column of 0.09-radius buttons 2.3 radii apart
        // up the right edge. Written out here so the arrangement has
        // somewhere to start and RESET has somewhere to go back to.
        const MARGIN: f32 = 0.06;
        const RING: f32 = 0.16;
        const BUTTON: f32 = 0.09;
        /// Middle to middle, between two buttons in a column.
        ///
        /// **Widened from 2.3 radii, and the reason is what is drawn
        /// rather than what is hit-tested.** At 2.3 the boxes stood 33
        /// px apart on the phone this is played on, and the two frames
        /// -- each drawn a stroke and a half outside its own box, see
        /// `hud::frame_overhang` -- closed all but six of that. A player
        /// sent a picture of the result: three buttons that read as one
        /// bar with words running into the lines between them.
        const GAP: f32 = BUTTON * 2.6;
        /// How big a wheel member is against a button on the glass.
        ///
        /// Smaller, and it buys the wheel its room: the ring has to be
        /// wide enough that the members clear each other, so the ring
        /// grows with the member, and a wheel of full-sized buttons
        /// reaches far enough down from the corner to meet the buttons
        /// the hotbar has pushed up. Still most of two fingers across
        /// on the screen this was cut for -- `widgets::FINGER` is 0.075
        /// of the shorter side and this is 0.144 of it.
        const WHEEL_BUTTON: f32 = BUTTON * 0.8;
        /// ...and how big the modifier is, which is smaller again.
        ///
        /// **Because of the band it has to fit in, not because it
        /// matters less.** It stands on the left edge, above the stick,
        /// and the two rules that bracket it leave 0.14 of the shorter
        /// side between them: `touch::crowds` keeps it clear of the
        /// ring, and
        /// `the_buttons_pressed_while_moving_are_the_ones_under_the_thumb`
        /// keeps every mid-stride control inside the bottom 55% of the
        /// glass, which is how far a thumb swinging from the corner
        /// actually reaches. Still 141 px on the 2712x1220 phone this
        /// was cut for -- 57 dp, above the 44 dp that every platform
        /// calls the smallest honest target, and wider than
        /// `widgets::FINGER` is.
        const MOD_BUTTON: f32 = 0.058;

        use crate::platform::Key;

        // ## Where each button goes, and why not all in one corner
        //
        // **A button pressed while moving and a button that stops the
        // game do not belong in the same place.** They were all stacked
        // into the bottom-right block, which is exactly where the right
        // thumb lives: that thumb is resting on the glass to mine and
        // tapping it to place (see `touch::Touch::is_mining`), so chat
        // and the pause menu sat under the finger doing the playing.
        // Opening a text box by mis-aiming a swing is worse than having
        // no button at all.
        //
        // So the corner nearest the thumb holds only what is wanted
        // *mid-stride*, and everything that takes the player out of the
        // world is put along the top edge, where no thumb rests and
        // nothing is aimed. Reaching them means letting go and reaching
        // -- which is the right cost for an action that was going to
        // interrupt the game anyway.
        //
        // The top row is on the right rather than the left because the
        // left is where the debug overlay writes, and a button under a
        // wall of text is a button nobody can read.
        //
        // ## ...and why that row is now one button
        //
        // Three buttons stood along the top edge for the whole game to
        // answer three things a player does a few times an hour. On a
        // 2712x1220 phone that is 3 x 220 px of world behind glass,
        // permanently, for the pause menu, the chat box and a debug
        // readout. They are now *in* the wheel: one `...` where the row
        // began, and the three come out around it when it is pressed.
        //
        // The rule for what belongs in there is the one the arrangement
        // already draws a line along: a control wanted mid-stride is on
        // the glass, and a control that takes the player out of the
        // world can cost an extra tap, because it was going to
        // interrupt them anyway.
        struct Spot {
            emits: Emits,
            corner: Corner,
            /// In from that corner, in short sides, middle of the
            /// button. Not read for a wheel member -- see
            /// `Placement::in_wheel` -- and written down all the same,
            /// so that switching one out of the wheel puts it
            /// somewhere rather than at the origin.
            inset: (f32, f32),
            in_wheel: bool,
            /// Half the side of the button.
            ///
            /// **A field rather than "wheel members are smaller".** The
            /// size used to be worked out from `in_wheel` alone, which
            /// was true of every button there was and stopped being true
            /// with the modifier: it is on the glass and it is small,
            /// because the band it has to fit in is narrow. A rule that
            /// happens to hold is a rule that breaks silently.
            radius: f32,
        }
        let spots = [
            // Under the thumb: jump, and the pack directly above it.
            Spot {
                emits: Emits::Key(Key::Space),
                corner: Corner::BottomRight,
                inset: (MARGIN + BUTTON, MARGIN + BUTTON),
                in_wheel: false,
                radius: BUTTON,
            },
            Spot {
                emits: Emits::Key(Key::KeyI),
                corner: Corner::BottomRight,
                inset: (MARGIN + BUTTON, MARGIN + BUTTON + GAP),
                in_wheel: false,
                radius: BUTTON,
            },
            // The wheel itself, where the top row used to start.
            Spot {
                emits: Emits::More,
                corner: Corner::TopRight,
                inset: (MARGIN + BUTTON, MARGIN + BUTTON),
                in_wheel: false,
                radius: BUTTON,
            },
            // Inside it, in the order they are wanted. Where they were
            // is kept, so a player who takes one out of the wheel gets
            // the old top row back a button at a time.
            Spot {
                emits: Emits::Key(Key::Escape),
                corner: Corner::TopRight,
                inset: (MARGIN + BUTTON + GAP, MARGIN + BUTTON),
                in_wheel: true,
                radius: WHEEL_BUTTON,
            },
            Spot {
                emits: Emits::Key(Key::Enter),
                corner: Corner::TopRight,
                inset: (MARGIN + BUTTON + GAP * 2.0, MARGIN + BUTTON),
                in_wheel: true,
                radius: WHEEL_BUTTON,
            },
            Spot {
                emits: Emits::Key(Key::F3),
                corner: Corner::TopRight,
                inset: (MARGIN + BUTTON + GAP * 3.0, MARGIN + BUTTON),
                in_wheel: true,
                radius: WHEEL_BUTTON,
            },
            // The journal -- the map and the recipe book on one button --
            // on the top edge beside the wheel, and **not in it**.
            //
            // It went in the wheel first, by the rule above, and the wheel
            // broke: a fourth member closes the angle between neighbours
            // from forty-five degrees to thirty, the ring has to nearly
            // double to keep them apart, and on the 2712x1220 phone it
            // put F3 at (2210, 735) -- in the bottom-right corner, under
            // the thumb that mines. `the_buttons_pressed_while_moving_are_the_ones_under_the_thumb`
            // caught it.
            //
            // Replacing a member was the other way, and each of the three
            // is somebody's only way to something on a phone. So the map
            // costs one button of glass along the top edge, where the old
            // row stood. It is the one out-of-the-world screen that earns
            // that: the pause menu, the chat and the readout are wanted a
            // few times an hour, and a map is looked at every few hundred
            // steps of a walk back to a bag. An open wheel takes it off
            // the glass while it is out -- see `yield_to_the_wheel`.
            Spot {
                emits: Emits::Key(Key::KeyM),
                corner: Corner::TopRight,
                inset: (MARGIN + BUTTON + GAP, MARGIN + BUTTON),
                in_wheel: false,
                radius: BUTTON,
            },
            // ## Shift, held -- "добавь эмуляцию shift путем удерживания"
            //
            // Shift is not one thing in this game. It sprints, it drops
            // a whole stack instead of one, it lays a log on a pile
            // rather than placing it, it sends a stack across a chest
            // and it is how a flying player goes down. A phone had one
            // of those five: a thumb pushed to the rim of the stick
            // runs (`touch::Touch::stick_sprinting`). The other four
            // needed a key that no glass has, so they could not be
            // asked for at all.
            //
            // ### Why a button that is held, and not a gesture
            //
            // Because a modifier *is* a held thing, and every other
            // finger on this glass is already spoken for. A long press
            // on the stick is what walking looks like -- a thumb pushed
            // out and kept there for as long as the walk lasts -- so a
            // hold there would sprint-modify every journey longer than
            // half a second and leave no way to simply walk. A long
            // press in the look area is already the pick swinging, and
            // it is the one gesture a player holds for whole seconds. A
            // second finger, which is what shift is on the menus
            // (`touch::Chord::Quick`), has nothing to land on in the
            // world: the glass there is divided into zones that must
            // not overlap, and the two halves are the stick and the
            // aim. A button is the only place left, and a button held
            // down is exactly what the player asked for.
            //
            // A latch -- tap once, stays on -- was the other candidate
            // and is refused for the reason `Hit::Released` exists: a
            // modifier that outlives the finger holding it is a key
            // stuck down, and the player finds out about it three
            // actions later, when the log they meant to place has gone
            // onto a pile instead.
            //
            // ### Why above the stick, on the left
            //
            // Because what it modifies is a *tap by the other thumb*.
            // Sprinting is the one use the phone already had; the four
            // it did not are all "hold shift and then do something",
            // and the something is placing, dropping or moving a stack
            // -- all of them the right thumb's. So the modifier goes
            // under the left thumb, which is the one free at that
            // moment, and it goes above the stick because the stick
            // owns the corner: a control in the bottom-left corner is a
            // control a walking thumb is already sitting on.
            //
            // It is not on the right, beside JUMP and PACK, for the
            // same reason the arrangement keeps the chat button off
            // that corner: that thumb is resting on the glass to mine,
            // and a button under it is a button pressed by a mis-aimed
            // swing. Here a mis-aim costs a modifier nobody asked for;
            // there it would cost the swing itself.
            //
            // Its state is the frame and the word, drawn bright while
            // it is down (`hud::touch_controls`, `EDGE_PRESSED`), which
            // is how every other button on the glass says the same
            // thing.
            Spot {
                emits: Emits::Key(Key::ShiftLeft),
                corner: Corner::BottomLeft,
                // Clear of the stick's *drawn* edge, which is a stroke
                // and a half outside its box on every side -- the sum
                // `touch::crowds` uses, so what ships cannot be an
                // arrangement the game's own rule calls crowded.
                inset: (
                    MARGIN + MOD_BUTTON,
                    MARGIN
                        + RING * 2.0
                        + crate::ui::hud::frame_overhang(RING)
                        + crate::ui::hud::frame_overhang(MOD_BUTTON)
                        + 0.02
                        + MOD_BUTTON,
                ),
                in_wheel: false,
                radius: MOD_BUTTON,
            },
        ];

        let mut buttons = [Placement::default(); Self::BUTTONS];
        for (button, spot) in buttons.iter_mut().zip(spots) {
            let side = spot.radius * 2.0;
            *button = Placement {
                corner: spot.corner,
                inset: spot.inset,
                width: side,
                height: side,
                shown: true,
                emits: spot.emits,
                in_wheel: spot.in_wheel,
            };
        }

        Self {
            stick: Placement {
                corner: Corner::BottomLeft,
                inset: (MARGIN + RING, MARGIN + RING),
                width: RING * 2.0,
                height: RING * 2.0,
                shown: true,
                // Unread: the stick reports a direction, not a press.
                // It is here because a placement is one shape, and a
                // shape with a hole in it for one case is a shape every
                // reader has to check.
                emits: Emits::Mine,
                in_wheel: false,
            },
            buttons,
        }
    }
}

impl TouchLayout {
    /// One placement per `touch::Control`. Indexed against
    /// `Control::ALL`, which nothing in the type system enforces, so a
    /// test in `touch` checks the two agree.
    pub const BUTTONS: usize = 8;

    /// Whether two arrangements are the same one.
    ///
    /// Exact float equality, deliberately. The question is not "are
    /// these near enough" but "did the player just move something",
    /// and both sides of the comparison are the *same* numbers copied
    /// -- never two results of arithmetic that ought to agree. A
    /// tolerance here would swallow the smallest nudge the editor can
    /// make, which is exactly the nudge someone lining a button up
    /// with their thumb is making.
    pub fn same_as(&self, other: &Self) -> bool {
        self.stick.same_as(&other.stick)
            && self
                .buttons
                .iter()
                .zip(other.buttons.iter())
                .all(|(a, b)| a.same_as(b))
    }

}

/// See-through near and solid past six chunks: see
/// `ClientSettings::transparent_leaves_chunks` for the arithmetic.
fn default_transparent_leaves_chunks() -> i32 {
    6
}

/// The stops the see-through leaves row walks, in chunks: solid
/// everywhere, then out to sixteen, then no limit.
pub const TRANSPARENT_LEAVES_STOPS: [i32; 9] =
    [0, 2, 3, 4, 6, 8, 12, 16, crate::engine::lod::LEAVES_SEE_THROUGH_EVERYWHERE];

fn default_relief_chunks() -> i32 {
    crate::engine::lod::RELIEF_CHUNKS
}

/// The stops the relief row walks, in chunks: flat everywhere, then out
/// to eight. See `ClientSettings::relief_chunks` for why no further.
pub const RELIEF_STOPS: [i32; 6] = [0, 2, 3, 4, 6, 8];

/// The stop in `stops` nearest `value`. A settings file is user input,
/// and a line at seven chunks is a line the row can neither show nor
/// step from.
fn nearest_stop(stops: &[i32], value: i32) -> i32 {
    stops
        .iter()
        .copied()
        .min_by_key(|stop| (i64::from(*stop) - i64::from(value)).abs())
        .unwrap_or(0)
}

/// Four on a desktop, off on a phone.
///
/// A discrete card resolves four samples in a fraction of a
/// millisecond; on a phone every sample is shaded and stored by a
/// GPU that was already the frame's bottleneck, into a framebuffer
/// several times the size of the one it has now, and the screen the
/// result lands on is six inches across -- the dotted line this
/// setting exists to remove is a pixel wide there and hard to see
/// at all. A player with a strong phone can turn it on; the default
/// is the one that does not cost the weak phone its frame rate.
fn default_msaa() -> u32 {
    if cfg!(target_os = "android") {
        1
    } else {
        4
    }
}

/// Seven tenths of the render distance: near enough to be worth having.
///
/// This used to add "far enough that the cut is inside haze", which was
/// true only while the fog began at 55% of the distance. It begins at three
/// quarters of the disc's reach now (`ClientSettings::fog_range`) -- 271
/// blocks at 24 -- and seven tenths of 384 is 269, so the tufts stop a
/// couple of blocks short of the haze, where a tuft is about two pixels
/// tall. Left at 0.7 rather than raised: it is the frame-rate lever for open
/// country, every saved file already holds it, and a player who sees the
/// line raises the row.
/// The stops the shadow distance row walks, in blocks: two chunks to
/// twelve.
pub const SHADOW_DISTANCES: [f32; 7] = [32.0, 48.0, 64.0, 96.0, 128.0, 160.0, 192.0];

fn default_shadow_distance() -> f32 {
    crate::engine::shadow::RADIUS
}

fn default_detail_distance() -> f32 {
    0.7
}

/// Ten chunks: a hundred and sixty blocks, which is past anything a
/// player is interacting with and near enough that the bands cover most
/// of a stock render distance.
fn default_lod_distance() -> i32 {
    10
}

/// The furthest the RENDER DISTANCE row goes, in chunks.
///
/// **One owner, because two readers have to agree on it.** The row clamps
/// to it and the singleplayer server streams to it (`singleplayer_server`),
/// and the second reader used to be a separate setting with its own limit
/// of 32 and its own default of 10 -- which quietly held a render distance
/// of 24 at ten. A copy of this number that drifts is that bug again.
pub const MAX_RENDER_DISTANCE: i32 = 24;

/// Enough cloud to break the sky up, not enough to close it in.
fn default_cloudiness() -> f32 {
    0.45
}

impl Default for ClientSettings {
    fn default() -> Self {
        Self {
            touch_layout: TouchLayout::default(),
            server_addr: "127.0.0.1:7878".to_string(),
            username: "player".to_string(),
            window_width: 1280,
            window_height: 720,
            fullscreen: false,
            vsync: true,
            fov_degrees: 70.0,
            render_distance_chunks: 6,
            mouse_sensitivity: 0.0025,
            // Modest rather than full. The bob used to be the sprint's
            // alone and is now on at every step, so it is in front of
            // the player many times more often than the number it was
            // tuned at ever was; the peak comes down to pay for that.
            // A player who wants the old sprint kick sets this to one.
            view_bob: 0.7,
            // **The one number, not the second copy of it.** The base
            // speed was lowered to 4.3 in the physics
            // (`DEFAULT_MOVE_SPEED`) and this default stayed at 5.5, so
            // the reduction reached nobody: every new player got the old
            // speed from here and the constant was decoration. It also
            // put a sprinting player at 8.25 blocks a second, which is
            // faster than anything in the world can run -- see
            // `animals::Species::run_speed`, where a deer is 7.6.
            move_speed: crate::logic::physics::DEFAULT_MOVE_SPEED,
            player_update_hz: 20.0,

            fog_enabled: true,
            // Three quarters of the way to where the world ends. See the
            // field for why the old pair had to go, and `fog_range` for
            // what the share is of.
            fog_start_share: 0.75,
            underwater_fog_distance: 18.0,

            master_volume: 0.8,
            music_volume: 0.7,

            ambient_light: 0.02,
            block_light_boost: 1.0,
            ambient_occlusion: 0.45,
            // On by default at a modest level: the shimmer it removes is
            // most of what makes distant terrain look noisy, and 4x is
            // free on anything with a discrete GPU.
            anisotropy: 4,
            msaa: default_msaa(),
            sky_scale: 3,
            // The default differs by platform, and `Default` is the
            // right place for it: it is the base both for a fresh
            // install *and* for a key missing from an older settings
            // file, so a phone gets a readable interface either way
            // without a second code path deciding when to apply it.
            //
            // Two, on the arithmetic in the field's own note: a phone
            // has about as many pixels down the screen as a monitor and
            // is a quarter of the size. It is a starting point, not a
            // measurement -- the settings row is what makes it right.
            //
            // One and a half rather than two, and the difference is
            // what a phone actually showed: at two the hotbar was
            // comfortable and the menus were a third of a screen too
            // big in both directions. The panels are held to what fits
            // by `widgets::fit_scale` whatever this says, so the number
            // is really the size of the *in-world* interface -- the bar
            // and the gauges, which are pinned to the bottom edge and
            // have room to grow into.
            ui_scale: if cfg!(target_os = "android") { 1.5 } else { 1.0 },
            transparent_leaves_chunks: default_transparent_leaves_chunks(),
            transparent_leaves: None,
            relief_chunks: default_relief_chunks(),
            shadows: crate::engine::shadow::Mode::Off,
            shadow_distance: default_shadow_distance(),
            plant_shadows: crate::engine::shadow::PlantShadows::Trees,
            lighting: crate::engine::lighting::default_quality(),
            detail_distance: 0.7,
            lod_distance_chunks: default_lod_distance(),
            lod_quality: crate::engine::lod::Quality::default(),
            cloudiness: 0.45,
            language: crate::ui::lang::Language::English,
            keybinds: crate::ui::keybinds::Keybinds::default(),

            worker_threads: 0,
            chunk_budget_ms: 3.0,
            mesh_budget_ms: 4.0,

            singleplayer_world_dir: "saves".to_string(),
            singleplayer_seed: 1337,

            menu_background: true,
            menu_background_scene: MENU_SCENE_ANY.to_string(),

            assets_dir: String::new(),
            debug_overlay_on_start: false,
        }
    }
}

impl ClientSettings {
    /// Fog distances in blocks for a render distance: clear to
    /// `fog_start_share` of where the world ends, and complete exactly
    /// there.
    ///
    /// **Measured from the end of the streamed disc, not from
    /// `render distance x 16`.** The disc is whole chunks and reaches less
    /// far off the axes (`ChunkManager::reach_blocks`): 362 blocks at 24,
    /// not 384. The old pair laid the fade out against 384, began it at
    /// 211 and relied on `Fog::clamp_to` to pull the end in -- so it was
    /// the *clear* field that was a share of a number the world never
    /// reached, and the fog that took what was left. Laid out against the
    /// reach, the share means what it says at every distance.
    ///
    /// What the two layouts come to at the player's 24 chunks, with the
    /// squared ramp `shade` applies:
    ///
    /// ```text
    ///               begins      10%          50%          complete
    /// 0.55 x 384    211 (13.2)  259 (16.2)   318 (19.9)   362 (22.6)   <- was
    /// 0.75 x 362    271 (17.0)  300 (18.8)   335 (21.0)   362 (22.6)   <- is
    /// ```
    ///
    /// (blocks, and chunks in brackets.)
    pub fn fog_range(&self, render_distance_chunks: i32) -> (f32, f32) {
        let reach = crate::logic::chunk_manager::ChunkManager::reach_blocks(render_distance_chunks);
        let start = (reach * self.fog_start_share).max(4.0);
        let end = reach.max(start + 8.0);
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

    /// What kind of place to open on, or `None` for "surprise me".
    ///
    /// Falls back rather than failing: the name comes from a file a
    /// person can edit, and anything unrecognised -- a typo, or the
    /// block name an older version wrote here -- means the same thing as
    /// asking for no particular place. See `menu_scene::look_for`.
    pub fn menu_background_place(&self) -> Option<Place> {
        Place::parse(&self.menu_background_scene)
    }

    /// Clamps every value into its supported range.
    ///
    /// Public because the settings screen edits these fields directly
    /// and has to re-clamp afterwards -- the same values arriving from a
    /// hand-edited file and from a button in the game deserve the same
    /// treatment, and duplicating the limits is how the two drift.
    pub fn sanitize(&mut self) {
        self.clamp();
        self.keybinds.sanitize();
    }
}

/// Where a singleplayer world looks for native mods.
///
/// The same rule as [`crate::engine::texture::resolve_assets_dir`], and
/// deliberately the same: a `mods` folder next to the executable in a
/// packaged build, else the one in the repository for `cargo run`. A
/// bare relative path would be read against the working directory,
/// which for a game started from a shortcut, from a file manager, or by
/// Steam is not the folder the game lives in -- and the symptom would
/// be a mod the player installed correctly and the game never mentions.
///
/// Falls back to a plain `mods` when there is no executable path to ask
/// about, which is the answer the server has always used.
pub fn resolve_mods_dir() -> std::path::PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join("mods");
            if candidate.is_dir() {
                return candidate;
            }
        }
    }
    let in_repository =
        std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../mods"));
    if in_repository.is_dir() {
        return in_repository;
    }
    std::path::PathBuf::from("mods")
}

impl ClientSettings {
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
        world: &crate::logic::worlds::World,
    ) -> primitive_server::settings::ServerSettings {
        let default_start_time = primitive_server::settings::ServerSettings::default().start_time_of_day;
        primitive_server::settings::ServerSettings {
            bind_addr: "127.0.0.1:0".to_string(),
            server_name: world.name.clone(),
            // A world carried over from the old single-folder layout has
            // no recorded seed; fall back to the configured default,
            // which is what generated it back then.
            world_seed: world.seed.unwrap_or(self.singleplayer_seed),
            // ...and which generator to hand that seed to. A world
            // records both, because both are what its saved edits were
            // written against.
            world_preset: world.preset,
            // ...and where on the planet, which is the third half.
            world_zone: world.zone,
            // ...and at which scale: an old world keeps the one its
            // buildings stand on.
            world_scale: world.scale,
            max_players: 4,
            // **Eight times what a shared server sends.**
            //
            // The budget exists so that one player joining cannot
            // starve everyone else's traffic -- that is what its own
            // documentation says it is for. Here there is exactly one
            // player, no network, and a socket on loopback: there is
            // nobody to starve and nothing to share.
            //
            // At the shared figure the visible world took eleven
            // seconds to fill in at a hundred and sixty chunks a
            // second. What limits it now is the client's own integration
            // and meshing budgets, which is where that decision belongs
            // -- they exist to spread the work across frames, and they
            // do it whether the chunks arrive in a trickle or a flood.
            chunk_send_budget_per_tick: 64,
            // **The render distance row's own ceiling, not a second
            // number.** This was `singleplayer_view_distance_chunks`, a
            // row of its own at ten by default -- and the client draws the
            // smaller of what it asks for and what `Welcome` says the
            // server streams. So a player who set RENDER DISTANCE to 24
            // and never found LOCAL WORLD DISTANCE was drawing ten chunks
            // under a menu saying twenty-four; that row's limit of 32 was
            // a number nothing could reach, because RENDER DISTANCE stops
            // at 24; and it was read once, when the world opened, so
            // raising the distance in the pause menu stopped short at
            // whatever the world had been opened with.
            //
            // A view distance is a cap for a *shared* server, where one
            // player's radius is everyone's bandwidth. Here there is one
            // player: the client's own disc is the whole of what is asked
            // for, and nothing else on the server reads the number except
            // to prune requests from outside it. Set to the ceiling so the
            // cap can never be the smaller of the two.
            view_distance_chunks: MAX_RENDER_DISTANCE,
            // **`PRIMITIVE_TIME=<0..1>` opens a new world at that hour**,
            // where 0 is midnight and 0.5 is noon.
            //
            // A hook rather than a convenience, on the rule CLAUDE.md
            // states: anything that has to be verified has to be
            // reachable from the environment or it cannot be verified
            // unattended at all. Night is exactly that -- a singleplayer
            // world starts mid-morning, a day is fifteen minutes long,
            // and photographing a dark cliff meant waiting seven of them
            // with a hand on the keyboard. `/time` is operator-only and
            // a singleplayer world has no operator.
            //
            // Only the *start*: a world that has been played remembers
            // its own clock (see `load_time_of_day`), so this cannot
            // quietly rewind somebody's evening.
            start_time_of_day: std::env::var("PRIMITIVE_TIME")
                .ok()
                .and_then(|raw| raw.trim().parse::<f32>().ok())
                .filter(|hour| hour.is_finite())
                .map(|hour| hour.rem_euclid(1.0))
                .unwrap_or(default_start_time),
            world_dir: world.directory.display().to_string(),
            plugin_dir: String::new(),
            // ...but the mods **are** loaded, and the folder is found
            // the way `assets/` is rather than trusted to be under the
            // working directory. A game started from a shortcut has
            // whatever working directory the shortcut felt like, and a
            // player who put `mods/` next to the executable would
            // otherwise be told there is no such folder.
            mod_dir: resolve_mods_dir().display().to_string(),
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
        // agree on what is selected. An unrecognised name -- including
        // the block name a version before 1.5 wrote into this row --
        // becomes `random`, which is what it is being treated as.
        self.menu_background_scene = match self.menu_background_place() {
            Some(place) => place.name().to_string(),
            None => MENU_SCENE_ANY.to_string(),
        };
        self.render_distance_chunks = self.render_distance_chunks.clamp(1, MAX_RENDER_DISTANCE);
        // To the nearest stop the row can show, and NaN to the default: a
        // hand-edited radius of zero is a shadow picture of nothing, and
        // one of a thousand is a texel the size of a house.
        self.shadow_distance = if self.shadow_distance.is_finite() {
            SHADOW_DISTANCES
                .iter()
                .copied()
                .min_by(|a, b| (a - self.shadow_distance).abs().total_cmp(&(b - self.shadow_distance).abs()))
                .unwrap_or_else(default_shadow_distance)
        } else {
            default_shadow_distance()
        };
        self.fov_degrees = self.fov_degrees.clamp(30.0, 120.0);
        self.mouse_sensitivity = self.mouse_sensitivity.clamp(0.0001, 0.05);
        // A hand-edited amplitude, so NaN as well as the range: a NaN
        // here multiplies the eye offset and the roll, and a view
        // matrix built from NaN is a black screen with no message on
        // it. Off is the safe answer to a number that is not one.
        self.view_bob = if self.view_bob.is_finite() {
            self.view_bob.clamp(0.0, 1.0)
        } else {
            0.0
        };
        self.move_speed = self.move_speed.clamp(0.5, 20.0);
        self.player_update_hz = self.player_update_hz.clamp(1.0, 60.0);
        // Below one so there is a fade at all -- a fog that begins where
        // the world ends is a wall -- and NaN to the default rather than
        // through `clamp`, which hands a NaN straight back to the shader.
        self.fog_start_share = if self.fog_start_share.is_finite() {
            self.fog_start_share.clamp(0.0, 0.95)
        } else {
            Self::default().fog_start_share
        };
        self.underwater_fog_distance = self.underwater_fog_distance.clamp(2.0, 200.0);
        self.master_volume = self.master_volume.clamp(0.0, 1.0);
        self.music_volume = self.music_volume.clamp(0.0, 1.0);
        self.ambient_light = self.ambient_light.clamp(0.0, 0.6);
        self.block_light_boost = self.block_light_boost.clamp(0.0, 3.0);
        self.ambient_occlusion = self.ambient_occlusion.clamp(0.0, 1.0);
        // Powers of two only, and never past 16: wgpu rejects anything
        // else outright, and a settings file is user input.
        self.detail_distance = self.detail_distance.clamp(0.2, 1.0);
        // The switch this distance replaced: off was solid everywhere, and
        // a player who chose it keeps it. On says nothing a default does
        // not, and is dropped either way so it is read exactly once.
        if self.transparent_leaves.take() == Some(false) {
            self.transparent_leaves_chunks = 0;
        }
        self.transparent_leaves_chunks = nearest_stop(&TRANSPARENT_LEAVES_STOPS, self.transparent_leaves_chunks);
        self.relief_chunks = nearest_stop(&RELIEF_STOPS, self.relief_chunks);
        // Off, or far enough out that the near band is not the ground
        // under the player's feet. A settings file is user input, and a
        // threshold of one would mesh the chunk they are standing in out
        // of two-block lumps.
        self.lod_distance_chunks = if self.lod_distance_chunks <= 0 {
            0
        } else {
            self.lod_distance_chunks.clamp(4, 64)
        };
        self.cloudiness = self.cloudiness.clamp(0.0, 1.0);
        self.anisotropy = match self.anisotropy {
            0..=1 => 1,
            2..=3 => 2,
            4..=7 => 4,
            8..=15 => 8,
            _ => 16,
        };
        // Powers of two up to 8, rounding *down*: a count wgpu has no
        // flag for is a validation error at the first pipeline, before
        // a frame is drawn, and 16 is refused by every desktop driver
        // this has been tried on. What the adapter actually offers is
        // a further cut the renderer makes; see `choose_sample_count`.
        self.msaa = match self.msaa {
            0..=1 => 1,
            2..=3 => 2,
            4..=7 => 4,
            _ => 8,
        };
        self.sky_scale = self.sky_scale.clamp(1, 4);
        // Half size is the smallest anybody could still read; four
        // times is one letter filling a phone. Outside that the player
        // cannot get back to the settings screen to undo it, which is
        // the only thing a clamp on a hand-editable file has to
        // prevent.
        self.ui_scale = if self.ui_scale.is_finite() {
            self.ui_scale.clamp(0.5, 4.0)
        } else {
            1.0
        };
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

    /// The mods folder is looked for where the game is, not where the
    /// game happens to have been started from.
    ///
    /// A bare `mods` is read against the working directory, and a game
    /// launched from a shortcut, a file manager or a store client has
    /// whatever working directory the launcher felt like -- usually not
    /// the folder holding the executable. The symptom is the worst kind
    /// there is: a mod installed correctly, in the right folder, that
    /// the game never mentions and cannot be told about.
    #[test]
    fn the_mods_folder_is_found_beside_the_game_rather_than_beside_the_shortcut() {
        let found = resolve_mods_dir();
        assert!(
            found.is_absolute() || found == std::path::Path::new("mods"),
            "either a real location or the honest fallback, not something in between: {}",
            found.display()
        );
        // Running the tests, the repository's own `mods/` is there, so
        // this must have resolved to a real folder rather than to the
        // fallback.
        assert!(
            found.is_dir(),
            "the repository has a mods folder and this did not find it: {}",
            found.display()
        );
    }

    /// A moved button is still moved after the game is shut.
    ///
    /// **The half that was missing.** `TouchLayout` had a `Default`
    /// written out so RESET had somewhere to go back to, and a
    /// `same_as` so a screen could tell a changed arrangement from an
    /// untouched one -- and nothing ever stored one. The frame loop
    /// asked for `TouchLayout::default()` outright, so the arrangement
    /// could not be changed, and could not have been given a screen to
    /// change it on, because there was nowhere for the answer to live.
    ///
    /// Round-tripped through the file rather than compared in memory:
    /// what broke here would break in `serde`, not in the struct.
    #[test]
    fn a_moved_thumb_control_survives_being_written_down() {
        let mut settings = ClientSettings::default();
        let shipped = settings.touch_layout;
        settings.touch_layout.buttons[0].inset = (0.31, 0.29);
        settings.touch_layout.buttons[0].corner = Corner::TopLeft;
        settings.touch_layout.stick.width = 0.21;

        let written = toml::to_string(&settings).expect("settings must serialise");
        let read: ClientSettings = toml::from_str(&written).expect("and parse back");

        assert!(
            !read.touch_layout.same_as(&shipped),
            "the arrangement came back as the one that shipped",
        );
        assert!(
            read.touch_layout.same_as(&settings.touch_layout),
            "the arrangement did not survive the file",
        );
    }

    /// A file written before any of this opens, and opens on the
    /// arrangement the game shipped with.
    ///
    /// Every player's settings file is one of these, so this is not a
    /// corner case -- it is the first launch after the update.
    #[test]
    fn a_settings_file_with_no_arrangement_in_it_gets_the_one_that_shipped() {
        let read: ClientSettings =
            toml::from_str("username = \"кто-то\"
").expect("an old file must still parse");
        assert_eq!(read.username, "кто-то");
        assert!(
            read.touch_layout.same_as(&TouchLayout::default()),
            "an old file came back with an arrangement nobody chose",
        );
    }

    /// The word on the modifier is one a phone player can read.
    ///
    /// `key_name` calls it `L SHIFT`, which names a key that glass does
    /// not have a second of. See [`Emits::label`].
    #[test]
    fn the_modifier_button_is_labelled_for_the_glass_and_not_for_a_keyboard() {
        assert_eq!(Emits::Key(crate::platform::Key::ShiftLeft).label(), "SHIFT");
    }

    #[test]
    fn the_background_scene_is_stored_by_name_and_survives_sanitising() {
        // The value goes on the settings screen as well as into the
        // file, so it has to be a word in both places -- an earlier
        // version of this row held a block *id* stringified, and a
        // fresh install showed a numeral where a word goes.
        let fresh = ClientSettings::default();
        assert_eq!(fresh.menu_background_scene, MENU_SCENE_ANY);
        assert_eq!(fresh.menu_background_place(), None, "random is not a place");
        // Sanitising leaves a good value alone rather than rewriting it.
        let mut same = fresh.clone();
        same.sanitize();
        assert_eq!(same.menu_background_scene, fresh.menu_background_scene);
        for place in Place::ALL {
            let mut chosen = ClientSettings {
                menu_background_scene: place.name().to_string(),
                ..Default::default()
            };
            chosen.sanitize();
            assert_eq!(chosen.menu_background_place(), Some(place));
        }
    }

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
            fog_start_share: 5.0,
            mesh_budget_ms: -3.0,
            ..Default::default()
        };
        s.clamp();
        assert!(s.render_distance_chunks <= 24);
        assert!(s.fov_degrees >= 30.0);
        assert!(s.fog_start_share < 1.0, "a fog that begins where the world ends is a wall");
        let mut nan = ClientSettings { fog_start_share: f32::NAN, ..Default::default() };
        nan.clamp();
        assert!(nan.fog_start_share.is_finite());
        assert!(s.mesh_budget_ms > 0.0);
    }

    #[test]
    fn a_settings_file_saved_before_the_last_button_still_loads_and_gains_it() {
        // The seventh thumb button arrived with the journal and the
        // eighth with the modifier that stands in for shift, and this is
        // the first launch after either: a file written with one fewer
        // must keep what the player set and grow the new one where it
        // ships -- the alternative is the whole file replaced by
        // defaults, language and keys included.
        //
        // **Against the arrangement rather than against a named key**,
        // which is what it used to do: the assertion said the last
        // button is `M`, so the day an eighth was appended a test about
        // old files went red for a reason that had nothing to do with
        // old files.
        let mut settings = ClientSettings {
            username: "old hand".to_string(),
            ..ClientSettings::default()
        };
        settings.touch_layout.buttons[0].inset = (0.3, 0.4);
        let mut value = toml::Value::try_from(&settings).expect("a value");
        value
            .get_mut("touch_layout")
            .and_then(|layout| layout.get_mut("buttons"))
            .and_then(|buttons| buttons.as_array_mut())
            .expect("the buttons are a list")
            .pop();
        let text = toml::to_string(&value).expect("written");
        let read: ClientSettings = toml::from_str(&text).expect("a six-button file must still parse");
        assert_eq!(read.username, "old hand", "the rest of the file was thrown away");
        assert_eq!(read.touch_layout.buttons[0].inset, (0.3, 0.4), "the player's arrangement was lost");
        let last = TouchLayout::BUTTONS - 1;
        assert!(
            read.touch_layout.buttons[last].same_as(&TouchLayout::default().buttons[last]),
            "the new button did not arrive where it ships"
        );
    }

    #[test]
    fn partial_config_keeps_defaults() {
        let parsed: ClientSettings = toml::from_str("username = \"shamkhan\"").unwrap();
        assert_eq!(parsed.username, "shamkhan");
        assert!(parsed.fog_enabled);
    }

    fn test_world() -> crate::logic::worlds::World {
        crate::logic::worlds::World {
            name: "Test".to_string(),
            seed: Some(77),
            preset: primitive_shared::worldgen::Preset::Normal,
            zone: primitive_shared::worldgen::Zone::Temperate,
            scale: primitive_shared::worldgen::Scale::Earth,
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
    fn in_singleplayer_the_render_distance_row_is_the_distance_drawn() {
        // **A render distance of 24 that looked like ten.** The local
        // server had a view distance of its own -- LOCAL WORLD DISTANCE,
        // ten by default -- and the client draws the smaller of what it
        // asks for and what `Welcome` says is streamed. So the row read 24
        // and ten chunks were drawn. Stated as that same `min` over every
        // value the row can hold, so no second number can come back in
        // under it.
        for wanted in 1..=MAX_RENDER_DISTANCE {
            let settings = ClientSettings {
                render_distance_chunks: wanted,
                ..ClientSettings::default()
            };
            let cap = settings.singleplayer_server(&test_world()).view_distance_chunks;
            assert_eq!(
                settings.render_distance_chunks.min(cap.max(1)),
                wanted,
                "the row says {wanted} and the local server streams {cap}"
            );
        }
    }

    #[test]
    fn a_file_with_the_old_local_world_distance_still_draws_what_its_row_says() {
        // The player's own file: 24 in the row and a leftover local
        // distance. It has to open, and the leftover has to mean nothing.
        let mut parsed: ClientSettings = toml::from_str(
            "render_distance_chunks = 24\nsingleplayer_view_distance_chunks = 10\n",
        )
        .expect("a file from before the row went must still parse");
        parsed.sanitize();
        let cap = parsed.singleplayer_server(&test_world()).view_distance_chunks;
        assert_eq!(parsed.render_distance_chunks.min(cap), 24);
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
    fn the_world_supplies_its_own_type_as_well_as_its_seed() {
        // The other half of what a world is. A test world whose preset
        // did not reach the server is a test world that generates
        // ordinary terrain and drops the player wherever the noise puts
        // them, with a `world.toml` that still says "test".
        use primitive_shared::worldgen::Preset;
        let showcase = crate::logic::worlds::World {
            preset: Preset::Test,
            ..test_world()
        };
        let server = ClientSettings::default().singleplayer_server(&showcase);
        assert_eq!(server.world_preset, Preset::Test);
        assert_eq!(
            ClientSettings::default()
                .singleplayer_server(&test_world())
                .world_preset,
            Preset::Normal
        );
    }

    #[test]
    fn a_world_with_no_recorded_seed_falls_back_to_the_configured_one() {
        // The layout from before worlds had metadata genuinely didn't
        // record a seed, and that default is what generated it.
        let client = ClientSettings {
            singleplayer_seed: 4242,
            ..Default::default()
        };
        let legacy = crate::logic::worlds::World {
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
    fn an_unknown_background_scene_falls_back_instead_of_failing() {
        // The name comes from a file a person can edit. A typo should
        // give some place or other, not an empty screen.
        let mut s = ClientSettings {
            menu_background_scene: "cheese".to_string(),
            ..Default::default()
        };
        assert_eq!(s.menu_background_place(), None);
        s.sanitize();
        assert_eq!(
            s.menu_background_scene, MENU_SCENE_ANY,
            "the file should be corrected"
        );
    }

    #[test]
    fn a_settings_file_from_before_the_menu_had_a_world_behind_it_still_opens() {
        // **The upgrade path, and the reason this test exists.** Every
        // installed copy of the game has `menu_background_block` in its
        // file, and that row is gone: it chose which block to tile, and
        // there is no tiling any more. A file naming one has to load,
        // keep every other setting in it, and quietly land on `random`
        // -- not fail to parse and hand the player back the defaults,
        // which would silently reset their language and their key
        // bindings along with it.
        let old = r#"
username = "somebody"
menu_background = true
menu_background_block = "cobblestone"
fov_degrees = 95.0
"#;
        let mut parsed: ClientSettings = toml::from_str(old).expect("an old file must parse");
        parsed.sanitize();
        assert_eq!(parsed.username, "somebody");
        assert_eq!(parsed.fov_degrees, 95.0);
        assert!(parsed.menu_background);
        assert_eq!(parsed.menu_background_scene, MENU_SCENE_ANY);
    }

    #[test]
    fn the_menu_opens_on_a_world_unless_the_player_turns_it_off() {
        // The switch is what a weak phone needs, and the default is
        // what the game says about itself on the first screen. It was
        // off while the backdrop was tiled wallpaper -- a legibility
        // cost that bought nothing. See `logic::menu_scene`.
        assert!(ClientSettings::default().menu_background);
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

    #[test]
    fn a_file_written_before_a_setting_existed_keeps_the_old_look() {
        // Every setting added later needs a `serde` default, and the
        // default has to be what the game did before it existed --
        // otherwise upgrading silently changes the picture. Leaves are
        // the case in hand: they have always been see-through.
        let old = "server_addr = \"127.0.0.1:7878\"
";
        let mut parsed = ClientSettings::parse(old, SETTINGS_PATH);
        parsed.clamp();
        assert!(
            parsed.transparent_leaves_chunks > 0,
            "upgrading turned the canopy solid"
        );
        assert!(parsed.relief_chunks > 0, "upgrading laid every stone flat");
    }

    #[test]
    fn a_player_who_turned_the_canopy_solid_keeps_it_solid_after_the_switch_became_a_distance() {
        // `transparent_leaves = false` is what the old switch wrote. Read
        // as a default it would quietly make a weak machine's canopy
        // see-through again -- the frames it was turned off to win.
        let mut off = ClientSettings::parse("transparent_leaves = false\n", SETTINGS_PATH);
        off.clamp();
        assert_eq!(off.transparent_leaves_chunks, 0);
        let mut on = ClientSettings::parse("transparent_leaves = true\n", SETTINGS_PATH);
        on.clamp();
        assert_eq!(on.transparent_leaves_chunks, default_transparent_leaves_chunks());
        // ...and it is never written back, so it is read once.
        let text = toml::to_string_pretty(&off).expect("settings should serialise");
        assert!(!text.contains("transparent_leaves ="), "the old switch was written back:\n{text}");
    }

    #[test]
    fn a_hand_edited_leaf_or_stone_distance_lands_on_a_stop_the_row_can_show() {
        let mut odd = ClientSettings::parse("transparent_leaves_chunks = 7\nrelief_chunks = -3\n", SETTINGS_PATH);
        odd.clamp();
        assert!(TRANSPARENT_LEAVES_STOPS.contains(&odd.transparent_leaves_chunks));
        assert_eq!(odd.relief_chunks, 0);
        let mut far = ClientSettings::parse("transparent_leaves_chunks = 2147483647\nrelief_chunks = 40\n", SETTINGS_PATH);
        far.clamp();
        assert_eq!(far.transparent_leaves_chunks, crate::engine::lod::LEAVES_SEE_THROUGH_EVERYWHERE);
        assert_eq!(far.relief_chunks, 8);
    }

    #[test]
    // Written field by field on purpose: the point of the test is that
    // *each* one round-trips, and a struct literal would silently keep
    // passing when a new field is added.
    #[allow(clippy::field_reassign_with_default)]
    fn every_setting_survives_being_written_and_read_back() {
        // The screen edits these and the file stores them; a setting
        // that does not round-trip is one that resets itself whenever
        // the player restarts.
        // Written field by field on purpose: the point of the test is
        // that *each* one round-trips, and a struct literal would
        // silently keep passing when a new field is added.
        let mut settings = ClientSettings::default();
        settings.transparent_leaves_chunks = 3;
        settings.relief_chunks = 0;
        settings.shadows = crate::engine::shadow::Mode::Hard;
        settings.lighting = crate::engine::lighting::Quality::High;
        settings.fog_enabled = false;
        settings.anisotropy = 8;
        settings.msaa = 2;
        settings.render_distance_chunks = 12;
        settings.sky_scale = 2;

        let text = toml::to_string_pretty(&settings).expect("settings should serialise");
        let parsed = ClientSettings::parse(&text, SETTINGS_PATH);
        assert_eq!(parsed.transparent_leaves_chunks, 3, "the see-through leaf distance reset itself");
        assert_eq!(parsed.relief_chunks, 0, "flat stones came back with a thickness");
        assert_eq!(parsed.shadows, crate::engine::shadow::Mode::Hard, "hard shadows came back as something else");
        assert_eq!(parsed.lighting, crate::engine::lighting::Quality::High, "the lighting step reset itself");
        assert!(!parsed.fog_enabled);
        assert_eq!(parsed.anisotropy, 8);
        assert_eq!(parsed.msaa, 2);
        assert_eq!(parsed.render_distance_chunks, 12);
        assert_eq!(parsed.sky_scale, 2);
    }

    #[test]
    fn shadows_are_off_until_the_player_asks_for_them() {
        // Both roads to a settings struct: a fresh install, and a file
        // from before the line existed. Either one arriving with shadows
        // on would hand every player a second terrain pass they did not
        // choose.
        assert!(!ClientSettings::default().shadows.is_on(), "a fresh install has shadows on");
        let old = "server_addr = \"127.0.0.1:7878\"\nfov_degrees = 80.0\n";
        let parsed = ClientSettings::parse(old, SETTINGS_PATH);
        assert!(!parsed.shadows.is_on(), "a file written before the setting opened with shadows on");
    }

    #[test]
    fn a_file_from_when_shadows_were_a_switch_opens_with_the_shadows_it_had() {
        // `shadows = true` was the soft shadows, the only ones there were.
        // Read as anything but a step, it would be a parse error -- and a
        // parse error is every setting in the file back at its default.
        use crate::engine::shadow::Mode;
        let base = "server_addr = \"127.0.0.1:7878\"\nfov_degrees = 83.0\n";
        for (line, mode) in [("shadows = true", Mode::Soft), ("shadows = false", Mode::Off), ("shadows = \"hard\"", Mode::Hard)] {
            let parsed = ClientSettings::parse(&format!("{base}{line}\n"), SETTINGS_PATH);
            assert_eq!(parsed.shadows, mode, "`{line}` opened as {:?}", parsed.shadows);
            assert_eq!(parsed.fov_degrees, 83.0, "`{line}` threw the rest of the file away");
        }
    }

    #[test]
    fn the_light_stays_as_it_was_until_the_player_asks_for_more() {
        // Both roads, as for the shadows: a fresh install and a file from
        // before the row. The warmer steps cost measurable frame time
        // (see `lighting::default_quality`), so neither road may arrive
        // on one.
        use crate::engine::lighting::Quality;
        assert_eq!(ClientSettings::default().lighting, Quality::Simple, "a fresh install has the warm light on");
        let parsed = ClientSettings::parse("server_addr = \"127.0.0.1:7878\"\n", SETTINGS_PATH);
        assert_eq!(parsed.lighting, Quality::Simple, "a file written before the row opened on a dearer step");
    }

    #[test]
    fn a_file_without_a_sample_count_gets_the_platform_default() {
        // The line is new; every settings file in the world lacks it.
        let parsed = ClientSettings::parse("server_addr = \"127.0.0.1:7878\"\n", SETTINGS_PATH);
        assert_eq!(parsed.msaa, default_msaa());
        assert_eq!(parsed.msaa, if cfg!(target_os = "android") { 1 } else { 4 });
    }

    #[test]
    fn a_sample_count_wgpu_has_no_flag_for_is_rounded_down_to_one_it_has() {
        // Only 1, 2, 4 and 8 exist as pipeline states; anything else
        // from a hand-edited file would be a validation panic before
        // the first frame.
        for (asked, got) in [(0, 1), (1, 1), (3, 2), (5, 4), (7, 4), (16, 8), (1000, 8)] {
            let mut parsed = ClientSettings::parse(&format!("msaa = {asked}\n"), SETTINGS_PATH);
            parsed.sanitize();
            assert_eq!(parsed.msaa, got, "msaa = {asked}");
        }
    }
}
