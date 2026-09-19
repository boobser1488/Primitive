//! The sun's shadows: one depth picture of the world taken from the sun,
//! laid over the ground around the player.
//!
//! **Off unless the player asks** (`ClientSettings::shadows`), and off
//! means off: no pass, no texture, no extra instruction in any shader
//! the frame runs. The terrain shader has separate *shadowed* entry
//! points (`vs_main_shadowed`, `fs_solid_shadowed`, `fs_cutout_shadowed`)
//! and the game switches pipelines rather than testing a flag per pixel,
//! so a player who never opens the setting runs byte for byte the
//! pipelines that ran before it existed. A uniform branch around the
//! lookup was the other way to write it and was turned down for that
//! reason alone: "costs nothing when off" is then a measurement that has
//! to be repeated on every GPU, where two pipelines make it a fact.
//!
//! ## How it works
//!
//! While the sun is up, the terrain around the player is drawn into a
//! depth texture through an orthographic camera looking along the
//! sunlight ([`LightView`]). The terrain pass then projects every
//! fragment into that picture and asks whether something nearer the sun
//! is in the way. What a "yes" takes away is **only the direct beam**:
//! the half-Lambert term goes down to a floor, and the sky fill, the
//! torches and the ambient floor are untouched. So a torch still lights a
//! shaded wall, and a cave -- which has no sky light for the beam to be a
//! share of -- looks the way it always did.
//!
//! **The picture is not taken every frame.** It is a picture of the world
//! in world coordinates, and most frames change nothing in it; which
//! frames do is [`ShadowCache`]'s decision.
//!
//! ## What it would otherwise get wrong
//!
//! * **Crawling edges.** An orthographic box that follows the camera
//!   smoothly moves its texel grid under the world every frame, and every
//!   shadow edge crawls one texel back and forth as the player walks.
//!   The box is moved in whole texels instead, in `f64`, so a still block
//!   lands on the same texel whatever the camera does, including across
//!   a jump of the render origin.
//! * **Shimmer under a moving sun** -- the same crawl, from the other
//!   thing that moves the grid. The camera looks along the beam, so the
//!   picture turns with the sun, and a grid that turns cannot be held
//!   still by snapping. The sun the shadows are drawn from moves in
//!   steps ([`SUN_STEP`]) and turns about the picture's own centre, so
//!   between two steps nothing moves at all and a step moves the grid
//!   under the player by a fraction of a texel.
//! * **Acne.** A surface compared against its own depth shadows itself
//!   in a moiré. Solid casters are drawn **back faces only** (front-face
//!   culled): a face turned toward the sun is then compared against the
//!   far side of whatever it belongs to, a block's thickness away, and
//!   the only faces that can meet their own depth are the ones turned
//!   away from the sun -- which the lighting has already put on the floor
//!   and which a shadow therefore cannot darken. A small offset along the
//!   receiver's normal covers what is left at the foot of a wall.
//! * **Light through a canopy.** A crown of leaves is not a solid, and
//!   back faces alone left only its underside casting. See
//!   [`LEAF_CASTER_CULL`].
//! * **The sun at the horizon.** Shadows grow as `1 / tan(elevation)`;
//!   at two degrees a fence post's is thirty blocks long and its texels
//!   are smeared along it. Strength fades in over the first eleven
//!   degrees of the day ([`strength`]), the pass is skipped entirely
//!   while it is zero -- all night, and under a closed storm deck -- and
//!   the lookup is skipped for faces the beam does not reach.
//!
//! ## Rejected
//!
//! * **Cascaded shadow maps** (three or four maps nested by distance).
//!   The standard answer for a sharp shadow at the feet and a shadow on a
//!   far hill in one frame, and it is three or four extra passes over the
//!   terrain on a renderer whose own measurements say the terrain pass is
//!   vertex-bound (see `renderer::solid_ranges_facing`). One map around
//!   the player is what "simple shadows" asked for; past its radius the
//!   baked sky light already darkens what is under a canopy or overhang.
//! * **Marching rays through the voxel grid** toward the sun. Exact, no
//!   acne, no resolution -- and a per-pixel loop through a 3D texture of
//!   the loaded world that the renderer does not have, re-uploaded on
//!   every block change, in a fragment shader the phone already finds
//!   dear (see `mottled_shade`).
//! * **Baking sun occlusion into the light map** on the CPU. Free to
//!   draw, but the sun moves: every chunk re-lit several times an hour,
//!   through the streaming budget that exists to keep chunk work off the
//!   frame.

use glam::{DMat4, DVec3, Mat4, Vec3};

/// The depth texture's format -- the frame's own, which every backend
/// this game runs on renders and compares.
pub const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// Which shadows the player asked for (`ClientSettings::shadows`): none,
/// hard-edged, or softened.
///
/// **One row for the sun's shadows and the fire's** (`engine::lamp_shadow`).
/// The player asked for one look -- a lamp in a dark room throwing hard
/// shadows -- and two rows free to disagree would be a camp at dusk with a
/// hard shadow from its fire beside a soft one from the sun. What the step
/// changes is the edge of both: Hard takes the sun's map in one filtered
/// lookup and walks one ray to a fire; Soft takes the taps `sunlit_share` has
/// always taken and walks four rays to four points of the flame.
///
/// **Hard is the cheaper step**, the opposite of what the words suggest in
/// most games' menus: here the soft edge is the extra work.
///
/// Carried to the shader as `shadow_bias.w`, one for Hard and nought for
/// Soft -- that way round so every offscreen tool that fills the globals
/// itself, and so leaves the spare slot at nought, keeps drawing the soft
/// shadows its measurements were taken of.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    #[default]
    Off,
    Hard,
    Soft,
}

impl Mode {
    pub const ALL: [Mode; 3] = [Mode::Off, Mode::Hard, Mode::Soft];

    pub fn is_on(self) -> bool {
        self != Mode::Off
    }

    /// The step `delta` along, **round rather than walked to an end**: the
    /// settings row is one wide switch (`Setting::is_toggle`), and a switch
    /// that stopped answering at its last value would read as broken.
    pub fn step(self, delta: i32) -> Self {
        let at = Self::ALL.iter().position(|mode| *mode == self).unwrap_or(0) as i32;
        Self::ALL[(at + delta).rem_euclid(Self::ALL.len() as i32) as usize]
    }
}

/// **A file written while shadows were a switch still opens as it was.**
/// `shadows = true` meant the only shadows there were, which are Soft's; read
/// as a string that is not there it would have been a parse error, and
/// `ClientSettings::parse` answers a parse error with every setting in the
/// file back at its default.
impl<'de> serde::Deserialize<'de> for Mode {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        #[serde(untagged)]
        enum Written {
            Switch(bool),
            Named(String),
        }
        Ok(match Written::deserialize(deserializer)? {
            Written::Switch(false) => Mode::Off,
            Written::Switch(true) => Mode::Soft,
            Written::Named(name) => match name.as_str() {
                "hard" => Mode::Hard,
                "soft" => Mode::Soft,
                _ => Mode::Off,
            },
        })
    }
}

/// Which plants throw the sun's shadow (`ClientSettings::plant_shadows`).
///
/// * **Off**: none -- not a tuft, not a crown. A wood is lit like a meadow,
///   which is the cheapest pass there is and the look of the game before it
///   had shadows under trees.
/// * **Trees** (the default, and what the game did before the row existed):
///   the crowns cast (`ShadowPass::cast_leaves`); grass, flowers, ferns,
///   crops and bushes drawn as sprites do not.
/// * **All**: the sprites as well, through their own caster
///   ([`ShadowMap::plant_caster`]).
///
/// **Why sprites were left out, and still are by default**: they are the
/// densest geometry in the world, a tuft's shadow is a few texels of speckle
/// that crawls as the sun steps, and in open country they would roughly
/// double the pass (`ShadowPass::draw`, "What is left out"). A choice a player
/// makes for a field of wheat with a long evening shadow, not a price every
/// player pays.
///
/// Rejected: **a switch for the sprites alone.** Turning the crowns off is the
/// one thing a player on a weak card asks of this row that nothing else gives
/// them -- a forest is where the pass is dearest (`LEAF_CASTER_CULL`) -- and a
/// second row for it would be two rows about one question.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PlantShadows {
    Off,
    #[default]
    Trees,
    All,
}

impl PlantShadows {
    pub const ALL: [PlantShadows; 3] = [PlantShadows::Off, PlantShadows::Trees, PlantShadows::All];

    /// Whether leaves go into the picture -- and into the Hard step's walk,
    /// which the caller of `lamp_shadow::Volume::gather` answers.
    pub fn crowns_cast(self) -> bool {
        self != PlantShadows::Off
    }

    /// Whether the sprites -- tufts, flowers, ferns, crops -- go in.
    pub fn sprites_cast(self) -> bool {
        self == PlantShadows::All
    }

    /// Walked, not wrapped, for the reason `Setting::Lighting` gives.
    pub fn step(self, delta: i32) -> Self {
        let at = Self::ALL.iter().position(|p| *p == self).unwrap_or(1) as i32;
        Self::ALL[(at + delta).clamp(0, Self::ALL.len() as i32 - 1) as usize]
    }
}

/// Texels along each side of the shadow map.
///
/// **Smaller on a phone**, where the texture is a quarter of the memory
/// and a quarter of the fill: 1024 over a 64-block radius and its margin
/// is a seventh of a block a texel, against 2048 over 96 on a desktop,
/// which is a tenth. Both are finer than a texture's pixel is at arm's
/// length, which is the scale the eye judges a shadow's edge against.
pub const RESOLUTION: u32 = if cfg!(target_os = "android") { 1024 } else { 2048 };

/// How far from the eye shadows reach, in blocks.
///
/// Ninety-six on a desktop: six chunks, past the place a player is doing
/// anything in and inside the fog of any render distance above eight.
/// Beyond it shadows fade out over the last seventh (`FADE_FROM`) rather
/// than ending on a line.
pub const RADIUS: f32 = if cfg!(target_os = "android") { 64.0 } else { 96.0 };

/// How far past [`RADIUS`] the picture reaches on every side of its own
/// centre, in blocks: the room the camera has to walk in before the
/// picture has to be taken again.
///
/// **The picture lags the camera on purpose.** A box that followed the eye
/// moved a texel on nearly every frame of walking, and each of those frames
/// drew the whole world from the sun again, although nothing in the
/// picture had changed but where its edge lay. With half a chunk of slack
/// a walking player retakes it about once in two seconds; the price is
/// texels an eleventh coarser -- a block in 9.8 against 10.7 on a desktop
/// -- which is less than the filtered edge is wide.
pub const MARGIN: f32 = 8.0;

/// The height of the world, which is as far above a receiver as a
/// caster can be.
const WORLD_TOP: f64 = primitive_shared::types::CHUNK_SIZE_Y as f64;

/// The band of sun elevation -- as the sine, which is what
/// `-sun_direction.y` is -- over which shadows arrive in the morning and
/// leave in the evening. Two and a third degrees to eleven and a half.
///
/// Below the bottom of it the shadow of a two-block player is fifty
/// blocks long, is mostly off the map, and would be drawn from texels
/// stretched twenty-five times along it; that is a picture of the
/// filtering, not of a shadow.
const RISE: (f32, f32) = (0.04, 0.20);

/// Where the fade toward the edge of the map begins, as a share of the
/// radius.
const FADE_FROM: f32 = 6.0 / 7.0;

/// How far a receiver's lookup point is pushed out along its own face
/// normal, in texels.
///
/// **The fix for the foot of a wall.** Ground touching the shaded side
/// of a block sits exactly where that side's back face ends, so its own
/// depth and the stored one agree to the last bit and the comparison is
/// a coin toss -- a speckled seam along every wall. Pushed one and a half
/// texels up off the ground, the point looks past the wall's foot into
/// the wall, which is nearer the sun, and the answer is the right one.
const NORMAL_OFFSET_TEXELS: f32 = 1.5;

/// Taken off a receiver's depth before the comparison, in blocks.
///
/// Small, because the back-face casters already keep a lit face a
/// block's thickness from anything it can be compared against; this is
/// for the float arithmetic and nothing else. Large biases are what
/// lift a shadow off its caster ("peter-panning"), which on a world of
/// one-block steps reads as every step floating.
const DEPTH_BIAS_BLOCKS: f64 = 0.02;

/// How far apart the four filtered lookups are, in texels: sharper at
/// noon, softer when the sun is low and a real shadow's edge has a long
/// way to blur over.
///
/// **A fifth of a texel at noon, where it was six tenths.** The spread was
/// there to hide the grid's staircase, and the staircase is gone from every
/// edge a block has along x or z (`LightView::around`). What was left was
/// the blur itself, and at 13:24 a one-block step's shadow is 0.38 of a
/// block -- under four texels -- so a filter that spread it over 0.20 of a
/// block from dark to lit (`what_a_step_casts_and_a_pale_wall_is_lit`,
/// `corner`) never let the shadow reach its own darkness: the floor beside
/// the wall came down to 0.62 of lit where the shadow is 0.57, and read as
/// a smudge. With the lookups together the edge goes over 0.08 of a block
/// and reaches 0.57. A fifth keeps a little of the spread for the edges
/// still diagonal -- an upright's, a crown's -- at a cost of a few
/// hundredths of a block of edge.
const SOFTNESS_TEXELS: (f32, f32) = (0.2, 1.4);

/// How far the sun turns between two pictures of its shadows: a tenth of a
/// degree, in radians, about the axis its path turns on.
///
/// **This is the shimmer.** The sun's camera looks along the beam, so as
/// the sun moves the picture turns, and snapping the box to whole texels
/// -- which holds a still block on its texel while the camera *walks* --
/// holds nothing while the camera *turns*. It was worse than that: the
/// snapping anchored the grid at the world's zero, and a grid turning about
/// a point a kilometre away slides under the player a kilometre's lever arm
/// at a time. At the default 900-second day that is seven blocks a second,
/// more than a texel every frame, so every shadow edge and every hole in a
/// canopy's shadow was rasterised afresh at a new sub-texel phase sixty
/// times a second, and boiled -- in the offscreen measurement
/// (`what_the_shadows_do_between_frames`) as tens of thousands of pixels a
/// frame changing under a camera that had not moved.
///
/// So the sun the shadows are drawn from moves in steps. Between two steps
/// nothing turns and the snapping holds exactly; a step turns the grid
/// about the picture's own centre ([`ShadowCache::plan`]), which is never
/// more than [`MARGIN`] from the player, and so moves a point twenty blocks
/// away by a third of a texel rather than by whatever a kilometre makes of
/// it. A shadow ten blocks long moves 0.017 of a block per step -- a sixth
/// of a texel, inside the filtered edge. At the default day that is a new
/// picture every quarter of a second.
///
/// Only the shadow's direction is stepped. The lighting reads the true sun,
/// and a tenth of a degree between the two is not visible in a lambert
/// term or anywhere else.
const SUN_STEP: f64 = 0.1 * std::f64::consts::PI / 180.0;

/// How much further than half a step the sun must have gone before the
/// next step is taken, as a share of a step.
///
/// A sun sitting on the line between two steps would otherwise round to
/// one on this frame and the other on the next -- the `f32` direction is
/// renormalised every frame and differs in its last bits -- and the
/// picture would be retaken on every frame and flick between the two.
/// The sun only ever goes one way, so the give costs a quarter-step's
/// delay and nothing else.
const SUN_STEP_GIVE: f64 = 0.25;

/// Units of the beam's tilt out of its plane, per unit: the tilt is part of
/// a step's key, and it is held as an integer for the reason
/// [`SunKey`] gives. `Sky` keeps the tilt constant, so this decides
/// nothing but the key.
const TILT_UNITS: f64 = 4096.0;

/// How often the picture is retaken while something that moves stands in
/// the world, in seconds.
///
/// **Thirty times a second.** The animals and the falling blocks cast
/// shadows and move on every frame, and the only way to move a shadow in a
/// depth picture is to take the picture again. At thirty the shadow of a
/// running boar trails it by at most a fifteenth of a block, and at sixty
/// frames a second and above that is the pass saved on every other frame
/// or better. Rejected: a second, smaller map for the moving casters
/// alone, which doubles the lookups of every receiver on every frame to
/// save a pass on some; and keeping a copy of the terrain's picture to
/// draw the animals over, which is sixteen megabytes copied a frame to
/// avoid drawing them.
///
/// **On the clock's thirtieths, not a thirtieth after the last picture.**
/// The first version compared the time since the last picture with this,
/// and at sixty frames a second two frames are a thirtieth to the last bit
/// -- so a frame that came in a hair early waited for a third, and a herd's
/// shadow stepped at thirty and twenty a second by turns: 45 pictures in two
/// seconds where there should be 60, which is how the test found it.
/// Retaking the picture on the first frame of each thirtieth of the clock
/// keeps thirty a second on average at any frame rate, and above sixty
/// frames a second it is never taken on two frames running.
const MOVING_CASTERS_EVERY: f64 = 1.0 / 30.0;

/// Which faces of the leaves are drawn into the depth picture: **the ones
/// looking at the sun**, and not the ones looking away.
///
/// **The leak through a canopy.** Solid casters are drawn back faces only
/// (see the module note on acne), and so were the leaves; but a crown is
/// not a solid. The mesher draws the plane two leaf cells share once, as
/// the lower cell's *top* face (`mesh::face_visible`), so while the sun is
/// up every plane inside a crown looks at the sun and was culled. A crown
/// five leaves deep cast the one lace of its underside -- a quarter of it
/// holes -- and the ground under an oak at noon was dappled like the
/// ground under a single leaf.
/// `a_canopy_casts_every_layer_of_itself_and_not_only_its_underside`
/// counts the planes on the line of the sun, and with back faces only it
/// was one of six.
///
/// The cure was to draw **all** of them, and half of that was waste: a
/// depth picture wants the nearest surface to the sun, and a face turned
/// away from the sun is by construction behind the face of the same cell
/// that is turned toward it. Measured on the forest of
/// `where_the_shadows_spend_the_frame` at 1920x1080, the crowns were 1.39
/// ms of a 2.42 ms pass at the golden hour -- more than half of it, on
/// geometry drawn from both sides through an alpha test, which is the one
/// pipeline in the pass with a fragment stage at all. Front faces only
/// keeps every plane the old rule kept (they all look at the sun, which is
/// the whole point of the note above) and drops the ones that could never
/// have won the depth test.
///
/// A face drawn toward the sun is also a receiver, and compared against
/// its own depth it would shade itself; so the shader draws such a face
/// [`LEAF_PUSH_BLOCKS`] behind itself (`vs_shadow_cutout`) -- which is now
/// every face this pipeline draws.
pub const LEAF_CASTER_CULL: Option<wgpu::Face> = Some(wgpu::Face::Back);

/// Which faces of the animals, the falling blocks and a dropped model are
/// drawn into the picture: **all of them**, as the leaves were.
///
/// **Their own pipeline, and the reason is thickness.** A limb is a
/// quarter of a block through, and the push that keeps a lit face from
/// shading itself is three quarters of one ([`LEAF_PUSH_BLOCKS`]): drawn
/// front faces only, a sheep's leg would write its depth further from the
/// sun than the leg's own far side, and the leg would stop casting. A leaf
/// cell is a whole block and does not have that problem. So the saving
/// above is taken on the geometry it is safe on, and a figure keeps the
/// rule it had.
pub const ENTITY_CASTER_CULL: Option<wgpu::Face> = None;

/// Which faces of solid terrain are drawn into the depth picture: those
/// turned away from the sun. See the module note on acne.
pub const SOLID_CASTER_CULL: Option<wgpu::Face> = Some(wgpu::Face::Front);

/// **Whether a crown is drawn into the picture with its holes**, or as
/// whole cells: with them at [`Mode::Soft`], without them at
/// [`Mode::Hard`].
///
/// "тени от листвы мягкие... а должна быть такой же чёткой по клеткам, как
/// от блоков". At the Hard step the sun is walked through the voxel volume
/// (`lamp_shadow`, "The sun, at the Hard step"), and there a leaf cell
/// stops the sun *whole* -- [`lamp_shadow::STOPS_SUN`] is a cell and not a
/// picture. The map, which answers everywhere the walk cannot reach -- past
/// the volume's thirty-two cells, or past `SUN_COLUMNS` of run -- drew the
/// same crown as a lace of alpha holes, and the comparison filter averaged
/// that lace into a smooth grey. So one crown cast a cell-sharp shadow ten
/// blocks from the player and a soft blob forty blocks away, in the same
/// frame, at the step whose whole promise is a hard edge.
/// `what_a_canopy_casts` photographs both.
///
/// Drawn whole, the map says what the walk says: a leaf cell is opaque, a
/// gap between two leaf cells is a gap, and the shadow of a wood is the
/// grid of its crowns at every distance.
///
/// **Soft keeps its lace**, because there the dapple under a tree is the
/// look that was asked for and the filter is meant to be averaging
/// something.
///
/// It is also the cheaper pipeline -- no texture fetch, no `discard`, so
/// the depth test runs before the fragment instead of after it -- which is
/// why `where_the_shadows_spend_the_frame` prices the crowns at all.
///
/// [`lamp_shadow::STOPS_SUN`]: crate::engine::lamp_shadow::STOPS_SUN
pub fn leaf_caster_is_cut_out(mode: Mode) -> bool {
    mode != Mode::Hard
}

/// How far a leaf face that looks at the sun is drawn behind itself in the
/// depth picture, in blocks.
///
/// **Most of a cell, and not all of it.** Pushed back, the lit face meets
/// the comparison a solid block's lit face meets against the block's far
/// side, and does not shade itself. Pushed back a whole cell, the plane
/// under an exposed top of a crown would be lit through that top at noon:
/// the sun is nearly overhead, the two planes are only a hair more than a
/// block apart along the beam, and the lookup is lifted a little toward
/// the sun as well. Three quarters keeps both.
const LEAF_PUSH_BLOCKS: f64 = 0.75;

fn smoothstep(from: f32, to: f32, x: f32) -> f32 {
    let t = ((x - from) / (to - from)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// How much of the direct light a shadow takes away right now, 0..1.
///
/// **Follows the beam, not the clock.** Zero from sunset to sunrise --
/// the moon is not given shadows; its light is a cool floor over the
/// whole world and a moon shadow at that brightness is an effect nobody
/// could see -- rising through [`RISE`] as the sun clears the horizon.
///
/// **And the weather takes it away.** A shadow is the absence of a
/// direct beam, and under cloud there is no beam to be absent: the sun's
/// colour already goes to the sky's white with the overcast
/// (`Sky::sun_color`), and the shadows go with it, gone entirely under a
/// closed storm deck. The eased figure, so they thin as a front comes in
/// rather than switching off when the packet lands.
pub fn strength(sun_direction: Vec3, overcast: f32) -> f32 {
    let elevation = -sun_direction.normalize_or_zero().y;
    smoothstep(RISE.0, RISE.1, elevation) * (1.0 - overcast.clamp(0.0, 1.0))
}

/// One step of the sun, as the shadows see it: how many [`SUN_STEP`]s round
/// its path, and its tilt out of that path.
///
/// **Integers, so that one step is equal to itself on every frame.** The
/// direction arrives renormalised from `f32` each frame and differs in its
/// last bits; kept as a direction, "has the sun moved?" would be yes on
/// every frame and the picture would be taken every frame, which is the
/// cost this whole arrangement exists to avoid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SunKey {
    turn: i64,
    tilt: i64,
}

impl SunKey {
    /// The nearest step to a direction. The turn is measured about the
    /// world's z axis, which is the axis `Sky::sun_direction` turns the sun
    /// on.
    fn of(sun_direction: Vec3) -> Self {
        let beam = sun_direction.as_dvec3().normalize_or(DVec3::NEG_Y);
        Self {
            turn: (beam.y.atan2(beam.x) / SUN_STEP).round() as i64,
            tilt: (beam.z * TILT_UNITS).round() as i64,
        }
    }

    /// This step, or the nearest one to where the sun is now if it has
    /// gone [`SUN_STEP_GIVE`] past the halfway line.
    fn follow(self, sun_direction: Vec3) -> Self {
        let beam = sun_direction.as_dvec3().normalize_or(DVec3::NEG_Y);
        let steps_round = (std::f64::consts::TAU / SUN_STEP).round();
        let turned = beam.y.atan2(beam.x) / SUN_STEP - self.turn as f64;
        // The short way round, so midnight's wrap from +pi to -pi is a
        // step and not a whole turn.
        let turned = turned - (turned / steps_round).round() * steps_round;
        let tilt = (beam.z * TILT_UNITS).round() as i64;
        if turned.abs() <= 0.5 + SUN_STEP_GIVE && tilt == self.tilt {
            self
        } else {
            Self::of(sun_direction)
        }
    }

    /// The direction the light travels at this step, unit length.
    fn beam(self) -> DVec3 {
        let z = (self.tilt as f64 / TILT_UNITS).clamp(-1.0, 1.0);
        let round = (1.0 - z * z).max(0.0).sqrt();
        let angle = self.turn as f64 * SUN_STEP;
        DVec3::new(angle.cos() * round, angle.sin() * round, z)
    }
}

/// How far a point slides across the picture per block it stands higher:
/// the beam's run over its drop, in x and in z.
///
/// **This is what keeps a block's edge on the grid.** The picture is taken
/// along the beam and laid out in the world's own x and z -- an oblique
/// projection, `x - shear.0 * y` and `z - shear.1 * y` -- so every edge a
/// block has along x or along z is a line of whole texels in the picture,
/// at every height. See `LightView::around` for the staircase this ended.
fn shear(beam: DVec3) -> (f64, f64) {
    let drop = beam.y.min(-1e-3);
    (beam.x / drop, beam.z / drop)
}

/// The sun's camera for one frame.
#[derive(Debug, Clone, Copy)]
pub struct LightView {
    /// Frame-relative world position (see `FrameParams::render_origin`)
    /// to the shadow map's clip space: x and y in -1..1 across the map,
    /// depth 0 at the sun's end of the box and 1 at the far side of the
    /// player's surroundings.
    pub view_proj: Mat4,
    /// See [`strength`].
    pub strength: f32,
    /// The size of one texel, in blocks.
    pub texel: f32,
    /// How far from the eye shadows reach, which is what they fade toward.
    /// The picture itself covers [`MARGIN`] more than this on every side
    /// of its own centre.
    pub radius: f32,
    /// Blocks from the near plane to the far one, which turns a distance
    /// in blocks into one in depth.
    depth_range: f32,
    softness_texels: f32,
    /// The direction the light travels -- the stepped sun's, which is the
    /// one the picture was taken along.
    pub direction: Vec3,
}

impl LightView {
    /// A picture with no history: centred on `eye` (world coordinates),
    /// looking along the stepped sun, expressed relative to
    /// `render_origin`.
    ///
    /// What a single still asks for -- the tools' pictures and the tests.
    /// The game asks [`ShadowCache::plan`], which decides where the centre
    /// goes and whether the pass has to run at all.
    ///
    /// **Built for tests only**, because only tests call it: the game never
    /// wants a picture without a history, and in the game's own build this
    /// was dead code that clippy, rightly, would not stay quiet about.
    #[cfg(test)]
    pub fn new(
        sun_direction: Vec3,
        eye: Vec3,
        render_origin: Vec3,
        radius: f32,
        resolution: u32,
        strength: f32,
    ) -> Self {
        Self::around(SunKey::of(sun_direction).beam(), eye.as_dvec3(), render_origin, radius, resolution, strength)
    }

    /// The sun's camera looking along `beam` at a square [`MARGIN`] wider
    /// than `radius` about `centre`, laid out in the world's x and z.
    ///
    /// **An oblique projection, not a camera turned to face the sun.** A
    /// point lands at `(x - sx * y, z - sz * y)` about the centre, where
    /// `(sx, sz)` is [`shear`] -- the beam's run over its drop -- and its
    /// depth is how far it stands below the top of the world. Everything on
    /// one line of the beam lands on one texel, which is all a shadow map
    /// needs, and the grid of texels lies along the world's own axes.
    ///
    /// **Why, measured.** The camera this replaced looked down the beam with
    /// its axes square to it, and the sun's path is tilted off the world's
    /// axes (`Sky::sun_direction`), so on the ground the texel grid lay at
    /// thirty-odd degrees to every block edge. A one-block step at 13:24
    /// casts a shadow 0.38 of a block wide -- under four texels -- and its
    /// edge crossed the grid diagonally: the filter voted over a staircase,
    /// and from a player's eye two blocks away each stair was tens of pixels.
    /// The player's report was a blotchy smear beside every step that did
    /// not follow it (`what_a_step_casts_and_a_pale_wall_is_lit`): with the
    /// filter's lookups drawn together, the place the shadow turns back to
    /// lit wandered 0.024 of a block along a wall through the turned camera
    /// and 0.006 through this one. Laid out
    /// along x and z, an edge a block has along either axis is a straight
    /// line of whole texels at any height and any hour; only the upright
    /// edges of a block are left diagonal, and they end a shadow rather than
    /// run along it.
    ///
    /// What it costs: the square covers the ground around the centre at the
    /// centre's own height, and a receiver far above or below slides across
    /// by `shear` times the difference -- at an elevation of twenty degrees,
    /// nearly three blocks for every block of height. A hillside a long way
    /// up-sun at dawn can fall off the picture, and `sunlit_share` fades a
    /// shadow out toward the picture's rim rather than cutting it there. The
    /// turned camera kept a sphere; this keeps the ground, which is where
    /// shadows are looked at.
    ///
    /// **Depth is height.** Along a beam that comes down, nearer the sun is
    /// higher, so the depth runs from the top of the world down to the
    /// lowest receiver the square is for: linear, the same for every texel,
    /// and a flat floor has one depth across all of it -- so no floor can
    /// meet its own depth at a slant, which is where acne comes from.
    ///
    /// **In `f64` until the last line.** The centre is a world coordinate,
    /// and a million blocks out an `f32` holds one to a sixteenth of a
    /// block, which is coarser than a texel. The render origin is taken off
    /// first, and only the small numbers left are narrowed.
    ///
    /// The winding is the turned camera's: a face toward the sun comes out
    /// counter-clockwise, which `SOLID_CASTER_CULL` relies on
    /// (`a_face_turned_to_the_sun_is_drawn_counter_clockwise_in_the_picture`).
    fn around(
        beam: DVec3,
        centre: DVec3,
        render_origin: Vec3,
        radius: f32,
        resolution: u32,
        strength: f32,
    ) -> Self {
        use glam::DVec4;
        let half = f64::from(radius + MARGIN);
        let texel = 2.0 * half / f64::from(resolution.max(1));
        let (sx, sz) = shear(beam);
        let origin = render_origin.as_dvec3();
        let c = centre - origin;
        // From the top of the world, or of the square if the player is above
        // it, down to the lowest receiver.
        let top = WORLD_TOP.max(centre.y + half) - origin.y;
        let range = top - (c.y - half);
        // x' = (x - cx - sx (y - cy)) / half
        // y' = -(z - cz - sz (y - cy)) / half    (negated: see the winding)
        // z' = (top - y) / range
        let view_proj = DMat4::from_cols(
            DVec4::new(1.0 / half, 0.0, 0.0, 0.0),
            DVec4::new(-sx / half, sz / half, -1.0 / range, 0.0),
            DVec4::new(0.0, -1.0 / half, 0.0, 0.0),
            DVec4::new((sx * c.y - c.x) / half, (c.z - sz * c.y) / half, top / range, 1.0),
        );
        let elevation = (-beam.y).max(1e-3) as f32;
        Self {
            view_proj: view_proj.as_mat4(),
            strength,
            texel: texel as f32,
            radius,
            depth_range: range as f32,
            softness_texels: SOFTNESS_TEXELS.1
                + (SOFTNESS_TEXELS.0 - SOFTNESS_TEXELS.1) * smoothstep(0.2, 0.7, elevation),
            direction: beam.as_vec3(),
        }
    }

    /// The camera [`around`](Self::around) replaced -- turned to look down
    /// the beam, its grid square to it -- centred on `eye`: kept for the step
    /// tool, which draws the same frame through both to show what the
    /// turned grid did to a step's shadow.
    #[cfg(test)]
    pub fn turned_with_the_sun(sun_direction: Vec3, eye: Vec3, render_origin: Vec3, radius: f32, resolution: u32, strength: f32) -> Self {
        let beam = SunKey::of(sun_direction).beam();
        let reference = if beam.y.abs() > 0.999 { DVec3::Z } else { DVec3::Y };
        let up = beam.cross(reference).normalize().cross(beam);
        let half = f64::from(radius + MARGIN);
        let centre = eye.as_dvec3();
        let reach = ((WORLD_TOP - (centre.y - half)).max(0.0) / (-beam.y).max(1e-3)).min(768.0);
        let relative = centre - render_origin.as_dvec3();
        let view = DMat4::look_at_rh(relative, relative + beam, up);
        let projection = DMat4::orthographic_rh(-half, half, -half, half, -(half + reach), half);
        let elevation = (-beam.y).max(1e-3) as f32;
        Self {
            view_proj: (projection * view).as_mat4(),
            strength,
            texel: (2.0 * half / f64::from(resolution.max(1))) as f32,
            radius,
            depth_range: (2.0 * half + reach) as f32,
            softness_texels: SOFTNESS_TEXELS.1
                + (SOFTNESS_TEXELS.0 - SOFTNESS_TEXELS.1) * smoothstep(0.2, 0.7, elevation),
            direction: beam.as_vec3(),
        }
    }

    /// Where a frame-relative point lands: x and y in -1..1 across the
    /// map (y up), z the depth the comparison uses.
    pub fn project(&self, relative: Vec3) -> Vec3 {
        self.view_proj.project_point3(relative)
    }

    /// Whether anything inside a frame-relative box could shade a
    /// receiver inside the map: whether the box, seen from the sun,
    /// overlaps the map and is not wholly behind its far plane.
    ///
    /// Asked of every loaded chunk on a frame that takes the picture, so
    /// it is eight matrix multiplies and no allocation. Conservative in the
    /// one direction that is safe -- a chunk whose corners straddle the map
    /// is kept -- and never keeps a chunk the GPU would clip in full.
    pub fn might_shade(&self, min: Vec3, max: Vec3) -> bool {
        let mut low = Vec3::splat(f32::INFINITY);
        let mut high = Vec3::splat(f32::NEG_INFINITY);
        for corner in 0..8 {
            let point = Vec3::new(
                if corner & 1 == 0 { min.x } else { max.x },
                if corner & 2 == 0 { min.y } else { max.y },
                if corner & 4 == 0 { min.z } else { max.z },
            );
            let at = self.project(point);
            low = low.min(at);
            high = high.max(at);
        }
        high.x >= -1.0 && low.x <= 1.0 && high.y >= -1.0 && low.y <= 1.0 && low.z <= 1.0
    }

    /// The three `Globals` fields the shadowed entry points read:
    /// the matrix, then `[strength, fade from, fade to, tap spacing in
    /// uv]`, then `[normal offset in blocks, depth bias, leaf push, 0]`,
    /// the last two in depth.
    pub fn globals(&self, resolution: u32) -> ([[f32; 4]; 4], [f32; 4], [f32; 4]) {
        let depth_range = f64::from(self.depth_range.max(1.0));
        (
            self.view_proj.to_cols_array_2d(),
            [
                self.strength,
                self.radius * FADE_FROM,
                self.radius,
                self.softness_texels / resolution.max(1) as f32,
            ],
            [
                self.texel * NORMAL_OFFSET_TEXELS,
                (DEPTH_BIAS_BLOCKS / depth_range) as f32,
                (LEAF_PUSH_BLOCKS / depth_range) as f32,
                0.0,
            ],
        )
    }
}

/// What the shadow map holds, and when it has to be taken again.
///
/// **The picture is kept.** It is a depth image of the world, and nothing
/// in it goes stale when the camera turns, or walks a few blocks, or when
/// the day's strength or the cloud changes -- those are uniforms the
/// receivers read, not the picture. It has to be taken again only when
///
/// * the sun has turned a [`SUN_STEP`];
/// * the camera has walked [`MARGIN`] from the picture's centre;
/// * a chunk that could cast into it has a new mesh, or none
///   ([`ShadowCache::touch`]);
/// * something that moves is in the world -- but no more often than
///   [`MOVING_CASTERS_EVERY`].
///
/// On every other frame the receivers read the picture through the camera
/// it was taken with, rebuilt for the frame's render origin, and the pass
/// does not run. On a desktop with the default day, a still player with no
/// animals about takes it four times a second instead of sixty, a walking
/// one about as often, and one watching a herd thirty times.
///
/// **A new picture's centre moves in whole texels from the old one's**, in
/// the new picture's own axes, so the grid stays where it was on the world
/// under the player: exactly, if the sun has not turned since, and by the
/// step's angle times the distance from the centre if it has. That is the
/// anchor the shimmer needed (see [`SUN_STEP`]).
///
/// The alternative was a pass every frame at the texel-snapped eye, which
/// is what this replaced, and what `what_the_shadows_cost_while_the_world_moves`
/// times it against.
#[derive(Debug, Default)]
pub struct ShadowCache {
    drawn: Option<Drawn>,
    /// Something that casts into the picture changed since it was taken.
    stale: bool,
}

/// The picture as it was taken.
#[derive(Debug, Clone, Copy)]
struct Drawn {
    sun: SunKey,
    /// World coordinates.
    centre: DVec3,
    /// Half the picture's side in blocks, and its texels along a side --
    /// which change only with the map, but a picture read through the
    /// wrong size is every shadow in the wrong place, so they are checked.
    half: f64,
    resolution: u32,
    /// The frame clock when it was taken.
    at: f64,
}

impl Drawn {
    /// How far across the picture's square a camera at `eye` stands from
    /// its centre, as the picture lays it out (`LightView::around`): the
    /// walk across, less what the camera's height slides it back -- or the
    /// camera's height from the centre, if that is the more.
    fn slid(&self, eye: DVec3) -> f64 {
        let (sx, sz) = shear(self.sun.beam());
        let d = eye - self.centre;
        (d.x - sx * d.y).hypot(d.z - sz * d.y).max(d.y.abs())
    }
}

/// One frame's shadow camera, and whether the pass has to run to fill it.
#[derive(Debug, Clone, Copy)]
pub struct Plan {
    pub view: LightView,
    pub redraw: bool,
}

impl ShadowCache {
    /// This frame's camera, and whether the picture has to be taken.
    ///
    /// `eye` and `render_origin` are world coordinates, `now` a clock in
    /// seconds that only goes forward (a clock that goes back retakes the
    /// picture rather than trusting it), and `moving_casters` whether
    /// anything that moves is being drawn this frame. See the type's note
    /// for the rule.
    #[allow(clippy::too_many_arguments)]
    pub fn plan(
        &mut self,
        sun_direction: Vec3,
        eye: DVec3,
        render_origin: Vec3,
        radius: f32,
        resolution: u32,
        strength: f32,
        moving_casters: bool,
        now: f64,
    ) -> Plan {
        let half = f64::from(radius + MARGIN);
        let sun = match self.drawn {
            Some(drawn) => drawn.sun.follow(sun_direction),
            None => SunKey::of(sun_direction),
        };
        // Which thirtieth of the clock a moment falls in. See
        // `MOVING_CASTERS_EVERY` for why a tick and not an interval.
        let tick = |seconds: f64| (seconds / MOVING_CASTERS_EVERY).floor();
        let redraw = match self.drawn {
            None => true,
            Some(drawn) => {
                self.stale
                    || drawn.sun != sun
                    || drawn.half != half
                    || drawn.resolution != resolution
                    // **The walk measured the way the picture sees it.** A
                    // camera that drops a block lands `shear` blocks across the
                    // picture -- nearly six at dawn -- so a player falling off
                    // a ledge under a low sun had left the square long before
                    // they were eight blocks from its centre.
                    || drawn.slid(eye) > f64::from(MARGIN)
                    || now < drawn.at
                    || (moving_casters && tick(now) > tick(drawn.at))
            }
        };
        if redraw {
            let (sx, sz) = shear(sun.beam());
            let texel = 2.0 * half / f64::from(resolution.max(1));
            let anchor = self.drawn.map_or(eye, |drawn| drawn.centre);
            // The new centre stands at the eye's height, and moves across by
            // whatever keeps every still point on the texel it had: a point
            // lands at `x - cx - sx (y - cy)`, so raising `cy` by `rise` is
            // made good by moving `cx` by `sx * rise`, and the rest of the way
            // to the eye goes in whole texels.
            let rise = eye.y - anchor.y;
            let whole = |d: f64| (d / texel).round() * texel;
            let centre = DVec3::new(
                anchor.x + sx * rise + whole(eye.x - anchor.x - sx * rise),
                eye.y,
                anchor.z + sz * rise + whole(eye.z - anchor.z - sz * rise),
            );
            self.drawn = Some(Drawn { sun, centre, half, resolution, at: now });
            self.stale = false;
        }
        let drawn = self.drawn.expect("a picture was just planned if there was none");
        Plan {
            view: LightView::around(drawn.sun.beam(), drawn.centre, render_origin, radius, resolution, strength),
            redraw,
        }
    }

    /// The geometry inside a world-space box changed: the next frame
    /// retakes the picture if anything in the box could cast into it.
    ///
    /// Asked whenever a chunk's mesh arrives or goes, so that a block
    /// broken in the shadows is gone from them on the next frame, and a
    /// chunk streamed in at the edge of a render distance of twenty-four
    /// does not cost a picture.
    pub fn touch(&mut self, min: DVec3, max: DVec3) {
        let Some(drawn) = self.drawn else {
            return;
        };
        if self.stale {
            return;
        }
        // Where the box's corners land in the picture's square, as `around`
        // lays it out, and whether any of the box is between the top of the
        // world and the lowest receiver.
        let (sx, sz) = shear(drawn.sun.beam());
        let mut low = DVec3::splat(f64::INFINITY);
        let mut high = DVec3::splat(f64::NEG_INFINITY);
        for corner in 0..8 {
            let point = DVec3::new(
                if corner & 1 == 0 { min.x } else { max.x },
                if corner & 2 == 0 { min.y } else { max.y },
                if corner & 4 == 0 { min.z } else { max.z },
            );
            let d = point - drawn.centre;
            let at = DVec3::new(d.x - sx * d.y, d.z - sz * d.y, point.y);
            low = low.min(at);
            high = high.max(at);
        }
        let h = drawn.half;
        let (bottom, top) = (drawn.centre.y - h, WORLD_TOP.max(drawn.centre.y + h));
        self.stale = high.x >= -h && low.x <= h && high.y >= -h && low.y <= h && high.z >= bottom && low.z <= top;
    }

    /// Forgets the picture: the map it was in has been made again, or the
    /// world it was of is gone.
    pub fn forget(&mut self) {
        *self = Self::default();
    }
}

/// Everything the shadow pass owns on the GPU. Built when the setting
/// goes on and dropped when it goes off, so a player who never turns it
/// on never pays for the sixteen megabytes of depth texture either.
pub struct ShadowMap {
    /// Kept alive for `view`, which is all anyone draws into.
    _texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub resolution: u32,
    /// Group 2 of the shadowed terrain pipelines: the depth picture and
    /// the comparison sampler it is read through.
    pub bind_group: wgpu::BindGroup,
    /// Solid terrain into the depth picture: back faces only. See the
    /// module note on acne for why that is the whole fix.
    pub solid_caster: wgpu::RenderPipeline,
    /// Leaves and the entities, cut out by their texture's alpha and
    /// drawn from both sides -- see [`LEAF_CASTER_CULL`] for why a canopy
    /// needs both.
    pub cutout_caster: wgpu::RenderPipeline,
    /// The same, **without the cut-out**: a leaf fills its cell. What the
    /// Hard step draws its crowns with; see [`leaf_caster_is_cut_out`].
    pub whole_leaf_caster: wgpu::RenderPipeline,
    /// The cut-out again, drawn from both sides: the animals and the
    /// falling blocks. See [`ENTITY_CASTER_CULL`].
    pub entity_caster: wgpu::RenderPipeline,
    /// Grass, flowers, ferns and crops, when [`PlantShadows::All`] asks: see
    /// `vs_shadow_plant` for why not the leaves' pipeline.
    pub plant_caster: wgpu::RenderPipeline,
    /// The terrain's own two pipelines, reading the shadow.
    pub solid: wgpu::RenderPipeline,
    pub cutout: wgpu::RenderPipeline,
    /// Where each caster chunk is, one entry per draw -- the shadow
    /// pass's own copy of `chunk_offsets`, because it draws a different
    /// set of chunks from the camera's in a different order.
    pub offsets: wgpu::Buffer,
    /// The shadow pass's sub-draws, for the same reason.
    pub indirect: wgpu::Buffer,
    /// Which cells round the player stop a fire's light, and the fires
    /// nearest the eye: group 2's other two bindings. See
    /// `engine::lamp_shadow`.
    pub lamp_cells: wgpu::Texture,
    pub lamp_heights: wgpu::Texture,
    pub lamp_buffer: wgpu::Buffer,
}

impl ShadowMap {
    /// Builds the texture and the four pipelines.
    ///
    /// The terrain shader is compiled again here rather than handed in.
    /// This runs once, when the setting is switched on, and a module
    /// passed from `GraphicsState::new` would have to be kept alive on
    /// the struct for the whole session for the sake of a switch most
    /// players never touch.
    ///
    /// **Compiled for the lighting step**, like the terrain's own
    /// pipelines: the shadowed receivers shade with the same `shade_lit`,
    /// and a shadow drawn with Simple's colours on a Balanced frame would
    /// be the one patch of ground lit by a different sun. The step also
    /// decides the filter -- High takes a second ring of taps (see
    /// `sunlit_share`) -- so the map is rebuilt when the step changes.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        device: &wgpu::Device,
        globals_layout: &wgpu::BindGroupLayout,
        texture_layout: &wgpu::BindGroupLayout,
        colour_format: wgpu::TextureFormat,
        depth_format: wgpu::TextureFormat,
        sample_count: u32,
        resolution: u32,
        offsets_bytes: u64,
        indirect_bytes: u64,
        lighting: crate::engine::lighting::Quality,
        atlas: crate::engine::texture::AtlasSplit,
    ) -> Self {
        use crate::engine::mesh::Vertex;

        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("shadow map"),
            size: wgpu::Extent3d {
                width: resolution,
                height: resolution,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        // **Linear, with a comparison**: the hardware compares the four
        // texels around the point and blends the four answers, which is
        // a 2x2 filtered lookup for the price of one call. Clamped, so a
        // lookup just off the map reads its edge rather than the far side
        // of it.
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("shadow sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });
        // The fires' half (`engine::lamp_shadow`): which cells round the
        // player stop light, and where the nearest fires are. In the same
        // group as the sun's map because they are read by the same entry
        // points and made and dropped by the same setting.
        use crate::engine::lamp_shadow;
        let lamp_side = lamp_shadow::SIDE as u32;
        let lamp_cells = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("lamp shadow cells"),
            size: wgpu::Extent3d { width: lamp_side, height: lamp_side, depth_or_array_layers: lamp_side },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D3,
            // Read with `textureLoad`, never filtered: a cell stops light or
            // it does not, and a blend of two cells is a wall made of fog.
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let lamp_view = lamp_cells.create_view(&wgpu::TextureViewDescriptor::default());
        let lamp_heights = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("lamp shadow heights"),
            size: wgpu::Extent3d { width: lamp_side, height: lamp_side, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            // Two bytes a column: the row over the highest thing in it, and
            // the row over the lowest. See `lamp_shadow::Volume::lows` for
            // the air under a crown the second one keeps the walk out of.
            format: wgpu::TextureFormat::Rg8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let heights_view = lamp_heights.create_view(&wgpu::TextureViewDescriptor::default());
        let lamp_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("lamp shadow lamps"),
            size: std::mem::size_of::<lamp_shadow::LampUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("shadow bind group layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D3,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("shadow bind group"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&lamp_view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: lamp_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(&heights_view),
                },
            ],
        });

        // **An older shader, for a before-and-after from one binary**: the
        // offscreen tools' `SHADOW_MAP_WGSL=<a copy of shader.wgsl>` builds
        // these pipelines from it, so a change to the shadowed entry points is
        // timed against what it replaced with nothing else different -- not
        // the build, not the scene, not the card's temperature an hour later.
        // Test builds only; the game compiles the text beside this file.
        #[cfg(test)]
        let text: std::borrow::Cow<'static, str> = match std::env::var("SHADOW_MAP_WGSL") {
            Ok(path) => std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}")).into(),
            Err(_) => include_str!("shader.wgsl").into(),
        };
        #[cfg(not(test))]
        let text: std::borrow::Cow<'static, str> = include_str!("shader.wgsl").into();
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("chunk shader, for the shadow pipelines"),
            source: wgpu::ShaderSource::Wgsl(atlas.specialise(lighting.specialise(&text))),
        });

        let caster_primitive = |cull_mode: Option<wgpu::Face>| wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode,
            polygon_mode: wgpu::PolygonMode::Fill,
            unclipped_depth: false,
            conservative: false,
        };
        let caster_depth = wgpu::DepthStencilState {
            format: FORMAT,
            depth_write_enabled: true,
            depth_compare: wgpu::CompareFunction::Less,
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        };
        let solid_caster_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("shadow solid caster layout"),
            bind_group_layouts: &[globals_layout],
            push_constant_ranges: &[],
        });
        let solid_caster = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("shadow solid caster"),
            layout: Some(&solid_caster_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_shadow",
                buffers: &[Vertex::layout(), Vertex::instance_layout()],
            },
            // Depth and nothing else: no fragment stage at all, so the
            // driver has nothing to run per texel.
            fragment: None,
            primitive: caster_primitive(SOLID_CASTER_CULL),
            depth_stencil: Some(caster_depth.clone()),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });
        let textured_caster_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("shadow cutout caster layout"),
                bind_group_layouts: &[globals_layout, texture_layout],
                push_constant_ranges: &[],
            });
        let cutout_caster = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("shadow cutout caster"),
            layout: Some(&textured_caster_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_shadow_cutout",
                buffers: &[Vertex::layout(), Vertex::instance_layout()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_shadow_cutout",
                targets: &[],
            }),
            primitive: caster_primitive(LEAF_CASTER_CULL),
            depth_stencil: Some(caster_depth.clone()),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });
        // The same vertex work -- a crown still needs the push behind
        // itself -- and no fragment stage at all, so a leaf fills its cell
        // in the picture. See `leaf_caster_is_cut_out`.
        let whole_leaf_caster = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("shadow whole-leaf caster"),
            layout: Some(&textured_caster_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_shadow_cutout",
                buffers: &[Vertex::layout(), Vertex::instance_layout()],
            },
            fragment: None,
            primitive: caster_primitive(LEAF_CASTER_CULL),
            depth_stencil: Some(caster_depth.clone()),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });
        // The animals, from both sides: see `ENTITY_CASTER_CULL`.
        let entity_caster = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("shadow entity caster"),
            layout: Some(&textured_caster_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_shadow_cutout",
                buffers: &[Vertex::layout(), Vertex::instance_layout()],
            },
            fragment: Some(wgpu::FragmentState { module: &shader, entry_point: "fs_shadow_cutout", targets: &[] }),
            primitive: caster_primitive(ENTITY_CASTER_CULL),
            depth_stencil: Some(caster_depth.clone()),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });

        // The sprites, from both sides -- a tuft's two planes each have one --
        // and through a vertex stage of their own. See `vs_shadow_plant`.
        let plant_caster = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("shadow plant caster"),
            layout: Some(&textured_caster_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_shadow_plant",
                buffers: &[Vertex::layout(), Vertex::instance_layout()],
            },
            fragment: Some(wgpu::FragmentState { module: &shader, entry_point: "fs_shadow_plant", targets: &[] }),
            primitive: caster_primitive(None),
            depth_stencil: Some(caster_depth),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });

        // The receivers: the game's `chunk_pipeline` and
        // `cutout_pipeline` state for state, with the shadowed entry
        // points and a third bind group. See `GraphicsState::new` for
        // why each of those states is what it is.
        let receiver_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("shadowed terrain layout"),
            bind_group_layouts: &[globals_layout, texture_layout, &layout],
            push_constant_ranges: &[],
        });
        let receiver = |label: &str, entry: &str, cull_mode: Option<wgpu::Face>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&receiver_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: "vs_main_shadowed",
                    buffers: &[Vertex::layout(), Vertex::instance_layout()],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: entry,
                    targets: &[Some(wgpu::ColorTargetState {
                        format: colour_format,
                        blend: Some(wgpu::BlendState::REPLACE),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    strip_index_format: None,
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode,
                    polygon_mode: wgpu::PolygonMode::Fill,
                    unclipped_depth: false,
                    conservative: false,
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: depth_format,
                    depth_write_enabled: true,
                    depth_compare: wgpu::CompareFunction::Less,
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState {
                    count: sample_count,
                    mask: !0,
                    alpha_to_coverage_enabled: false,
                },
                multiview: None,
            })
        };
        let solid = receiver("shadowed chunk pipeline", "fs_solid_shadowed", Some(wgpu::Face::Back));
        let cutout = receiver("shadowed cutout pipeline", "fs_cutout_shadowed", None);

        let offsets = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("shadow caster offsets"),
            size: offsets_bytes,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let indirect = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("shadow caster sub-draws"),
            size: indirect_bytes,
            usage: wgpu::BufferUsages::INDIRECT | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            _texture: texture,
            view,
            resolution,
            bind_group,
            solid_caster,
            cutout_caster,
            whole_leaf_caster,
            entity_caster,
            plant_caster,
            solid,
            cutout,
            offsets,
            indirect,
            lamp_cells,
            lamp_heights,
            lamp_buffer,
        }
    }

    /// Puts a volume's cells into the texture the rays walk, and its columns'
    /// heights into the one the sun's walk skips columns by. See
    /// `lamp_shadow::Volume::cells` and `heights` for the order.
    pub fn write_lamp_cells(&self, queue: &wgpu::Queue, cells: &[u8], heights: &[u8], lows: &[u8]) {
        let side = crate::engine::lamp_shadow::SIDE as u32;
        // The two columns' bytes woven together, which is what an Rg8 texture
        // is: built here rather than kept woven, because the walk's own copy
        // on the CPU reads them as two plain grids and a woven pair would put
        // a stride into every one of its tests.
        let woven: Vec<u8> = heights.iter().zip(lows).flat_map(|(&high, &low)| [high, low]).collect();
        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &self.lamp_heights,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &woven,
            wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(side * 2), rows_per_image: None },
            wgpu::Extent3d { width: side, height: side, depth_or_array_layers: 1 },
        );
        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &self.lamp_cells,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            cells,
            wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(side), rows_per_image: Some(side) },
            wgpu::Extent3d { width: side, height: side, depth_or_array_layers: side },
        );
    }

    /// Opens the depth-only pass into the map, cleared to "nothing in
    /// the way".
    pub fn begin_pass<'a>(
        &'a self,
        encoder: &'a mut wgpu::CommandEncoder,
        timestamp_writes: Option<wgpu::RenderPassTimestampWrites<'a>>,
    ) -> wgpu::RenderPass<'a> {
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("shadow pass"),
            color_attachments: &[],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &self.view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes,
            occlusion_query_set: None,
        })
    }

    /// Where a direction-group test should put its eye so that it keeps
    /// exactly the faces turned away from the sun -- the ones this pass
    /// draws -- for a chunk whose corner is at `chunk_min` (world).
    ///
    /// `renderer::solid_ranges_facing` decides which groups an eye can
    /// see; an eye far out along the beam sees precisely the faces whose
    /// normal points along it. Ten thousand blocks is far enough that a
    /// beam component of a sixth of a percent already puts the eye past
    /// a chunk's sixteen blocks, and when a component is smaller than
    /// that both groups on its axis are kept, which is the safe answer.
    pub fn group_eye(direction: Vec3, chunk_min: [f32; 3]) -> [f32; 3] {
        let centre = Vec3::from(chunk_min) + Vec3::new(8.0, 0.0, 8.0);
        (centre + direction * 10_000.0).into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY_SECONDS: f32 = 900.0;

    fn sun_at(time_of_day: f32) -> Vec3 {
        crate::engine::sky::Sky::new(time_of_day, DAY_SECONDS).sun_direction()
    }

    /// Where a frame-relative point lands on the map, in texels.
    fn texel_of(view: &LightView, relative: Vec3) -> glam::Vec2 {
        (view.project(relative).truncate() * 0.5 + 0.5) * RESOLUTION as f32
    }

    /// How far apart two points sit *inside* their texels, measured round
    /// the texel so 0.99 and 0.01 are close.
    fn phase_apart(a: glam::Vec2, b: glam::Vec2) -> f32 {
        let d = (a.fract() - b.fract()).abs();
        d.min(glam::Vec2::ONE - d).max_element()
    }

    /// The sun a shadowed terrain fragment is lit by, as a share of the full
    /// beam: `sun_sky * shadowed_lambert` in `shader.wgsl`, written out for a
    /// fragment with `sky` of the flood fill's sky over it, a vertex
    /// half-Lambert of `lambert`, `reach` of the shadows' strength, and
    /// `share` of the beam let through by the map or the walk. The shader's
    /// lines are asserted to be these by the test that reads it.
    fn shadowed_sun(sky: f32, lambert: f32, reach: f32, share: f32) -> f32 {
        const LAMBERT_FLOOR: f32 = 0.35;
        const SHADOW_FLOOR: f32 = 0.15;
        const AWAY_FLOOR: f32 = 0.25;
        const AWAY_BLEND: f32 = 0.3;
        let beam = (lambert - LAMBERT_FLOOR) / (1.0 - LAMBERT_FLOOR);
        let least = if sky <= 0.0 || reach <= 0.0 {
            lambert
        } else {
            let kept = AWAY_FLOOR + (SHADOW_FLOOR - AWAY_FLOOR) * smoothstep(0.0, AWAY_BLEND, beam);
            (LAMBERT_FLOOR + (kept - LAMBERT_FLOOR) * reach).min(lambert)
        };
        let shaded = if least >= lambert || beam <= 1e-4 { least } else { least + (1.0 - least) * beam * share };
        let open = sky + (if sky > 1e-4 { 1.0 } else { 0.0 } - sky) * reach;
        let sun_sky = (sky * least + open * (shaded - least).max(0.0)) / shaded.max(1e-4);
        sun_sky * shaded
    }

    #[test]
    fn the_shadow_floor_of_a_cave_follows_its_sky_and_not_the_open_sky() {
        // **The report**: "в пещерах проблемы с тенями". A room reached by a
        // tunnel has a few sixteenths of sky; the walk finds rock over every
        // fragment of it and lets none of the beam through, and the floor the
        // beam comes down to -- the light off the sky and the ground -- was
        // scaled by the *open* sky with the beam. Every wall of the room took
        // a sixth of the full sun and came out brighter than with no shadows
        // at all (`what_layers_plants_and_caves_cast`, `cave_dark`). A shadow
        // takes light away; it never gives it.
        let source = include_str!("shader.wgsl");
        for line in [
            "const LAMBERT_FLOOR: f32 = 0.35;",
            "const SHADOW_FLOOR: f32 = 0.15;",
            "const AWAY_FLOOR: f32 = 0.25;",
            "const AWAY_BLEND: f32 = 0.3;",
            "    let open = mix(sky, step(1e-4, sky), shadow_reach(in.view_distance));",
            "    let least = shadow_least(in.lambert, sky, in.view_distance);",
            "    return (sky * least + open * max(lambert - least, 0.0)) / max(lambert, 1e-4);",
            "    return min(mix(LAMBERT_FLOOR, kept, reach), lambert);",
        ] {
            assert!(source.contains(line), "shader.wgsl no longer says `{line}`, which this test is a copy of");
        }
        for sky in [1.0 / 15.0, 2.0 / 15.0, 0.3, 0.6, 1.0] {
            for lambert in [0.35, 0.4, 0.6, 0.8, 1.0] {
                for reach in [0.0, 0.25, 0.5, 1.0] {
                    let without = sky * lambert;
                    // In the dark, the sun gets no further than without shadows.
                    let blocked = shadowed_sun(sky, lambert, reach, 0.0);
                    assert!(
                        blocked <= without + 1e-6,
                        "sky {sky:.2}, lambert {lambert}, reach {reach}: a face the sun cannot reach took {blocked:.3} of it, {without:.3} without shadows"
                    );
                    // And a face the sun does reach has the whole beam over
                    // the floor wherever there is any sky over it: the fix for
                    // the roof that darkened sand twice is kept.
                    let lit = shadowed_sun(sky, lambert, 1.0, 1.0);
                    let floor = shadowed_sun(sky, lambert, 1.0, 0.0);
                    let beam = (lambert - 0.35) / 0.65;
                    assert!(
                        (lit - floor - (1.0 - floor / sky) * beam).abs() < 1e-4,
                        "sky {sky:.2}, lambert {lambert}: the sun's beam is scaled by the flood fill again"
                    );
                }
            }
        }
        // Under an open sky nothing changed: the shade is the old shade.
        assert!((shadowed_sun(1.0, 0.9, 1.0, 0.0) - 0.15).abs() < 0.02, "shade on open ground moved");
    }

    /// `vs_shadow_plant`'s push, in blocks of height: the receivers' lift
    /// times the tangent of the sun's elevation, no more than the leaves'.
    fn plant_push_blocks(sun: Vec3, lift: f32) -> f32 {
        let rise = (-sun.y).max(1e-3);
        let run = glam::Vec2::new(sun.x, sun.z).length().max(1e-3);
        (lift * rise / run).min(LEAF_PUSH_BLOCKS as f32)
    }

    #[test]
    fn a_plants_shadow_starts_at_its_foot_at_every_hour_the_sun_casts() {
        // **The report**: "у растений баги с тенями". A sprite was drawn into
        // the picture an eighth of a block lower than it stands, at every
        // hour, and the depth of this picture is height: a blade had to stand
        // that eighth, and the ground's own lift, above the ground to cast at
        // all. With the sun low that is a long way out -- half a block of
        // bare sand between a fireweed and its shadow at the golden hour, a
        // block at dusk. Measured through the picture's own matrix: a stem
        // standing at the origin, and the ground `d` blocks down-sun from it
        // looked up as `vs_main_shadowed` looks it up.
        let source = include_str!("shader.wgsl");
        assert!(
            source.contains(&format!("const LEAF_PUSH_BLOCKS: f32 = {LEAF_PUSH_BLOCKS:?};")),
            "shader.wgsl's leaf push is not this file's"
        );
        assert!(
            source.contains("let push = min(globals.shadow_bias.x * rise / run, LEAF_PUSH_BLOCKS);"),
            "shader.wgsl no longer pushes a plant the way this test measures"
        );
        let mut checked = 0;
        for step in 0..=40 {
            let t = 0.26 + step as f32 * 0.0118;
            let sun = sun_at(t);
            let elevation = -sun.y;
            let strength = strength(sun, 0.0);
            // Under about nine degrees the shadows are still fading in
            // (`strength`), and even the ground's own depth bias is a block
            // long there.
            if strength <= 0.0 || elevation < 0.15 {
                continue;
            }
            let view = LightView::new(sun, Vec3::new(0.5, 64.0, 0.5), Vec3::ZERO, RADIUS, RESOLUTION, strength);
            let (_, _, bias) = view.globals(RESOLUTION);
            let (lift, depth_bias) = (bias[0], bias[1]);
            let push = plant_push_blocks(sun, lift) * bias[2] / LEAF_PUSH_BLOCKS as f32;
            let away = glam::Vec2::new(sun.x, sun.z).normalize();
            let foot = Vec3::new(0.0, 64.0, 0.0);
            // Where the shadow of a stem a block tall begins on the ground.
            let starts = (1..400).map(|i| i as f32 * 0.005).find(|&d| {
                let ground = foot + Vec3::new(away.x, 0.0, away.y) * d;
                let looked = view.project(ground + Vec3::Y * lift);
                // The point of the stem on the same line of the beam.
                let height = lift + d * elevation / glam::Vec2::new(sun.x, sun.z).length();
                if height > 1.0 {
                    return false;
                }
                let stem = view.project(foot + Vec3::Y * height);
                looked.z - depth_bias > stem.z + push
            });
            let starts = starts.expect("a stem a block tall casts no shadow at all");
            // Three texels: the lift itself, a texel and a half, which every
            // receiver's shadow is short by, and the depth bias's share.
            let allowed = 3.0 * view.texel;
            assert!(
                starts <= allowed,
                "at {t:.3} (sun {:.1} degrees up) a plant's shadow starts {starts:.2} blocks from its foot; {allowed:.2} is a texel's slack",
                elevation.asin().to_degrees()
            );
            checked += 1;
        }
        assert!(checked > 20, "the day had only {checked} hours of shadow in it");
    }

    #[test]
    fn there_are_no_shadows_at_night_or_under_a_closed_sky() {
        assert_eq!(strength(sun_at(0.0), 0.0), 0.0, "midnight cast a shadow");
        assert_eq!(strength(sun_at(0.9), 0.0), 0.0, "the small hours cast a shadow");
        assert_eq!(strength(sun_at(0.5), 1.0), 0.0, "a storm at noon cast a shadow");
        assert!(strength(sun_at(0.5), 0.0) > 0.99, "noon in a clear sky is not full strength");
    }

    #[test]
    fn shadows_come_in_with_the_morning_and_thin_with_the_cloud() {
        let mut previous = -1.0;
        for step in 0..=20 {
            let t = 0.24 + step as f32 * 0.005;
            let now = strength(sun_at(t), 0.0);
            assert!(now >= previous, "the shadow weakened as the sun rose at {t}");
            previous = now;
        }
        let clear = strength(sun_at(0.4), 0.0);
        let cloudy = strength(sun_at(0.4), 0.5);
        assert!(cloudy < clear && cloudy > 0.0, "half a sky of cloud: {clear} -> {cloudy}");
    }

    #[test]
    fn a_still_block_stays_on_its_texel_while_the_camera_walks() {
        // The crawl the snapping exists to prevent: if the box followed
        // the camera smoothly, the point's position *within* its texel
        // would change every step and an edge through it would flicker.
        // Walked far enough here that the picture is retaken more than
        // once, because a new picture's centre has to land on the old grid.
        let sun = sun_at(0.35);
        let origin = Vec3::new(1_000_064.0, 0.0, -2_000_000.0);
        // Numbers an f32 holds exactly this far out: halves, not quarters.
        let block = Vec3::new(1_000_070.5, 71.0, -1_999_990.5);
        let mut cache = ShadowCache::default();
        let (mut first, mut pictures) = (None, 0);
        for step in 0..40 {
            // Steps of a fraction of a texel and of several, both.
            let eye = Vec3::new(1_000_060.0 + step as f32 * 0.037, 72.6, -1_999_995.0 + step as f32 * 0.61);
            let plan = cache.plan(sun, eye.as_dvec3(), origin, RADIUS, RESOLUTION, 1.0, false, f64::from(step) / 60.0);
            pictures += usize::from(plan.redraw);
            let at = texel_of(&plan.view, block - origin);
            let first = *first.get_or_insert(at);
            let drift = phase_apart(at, first);
            assert!(drift < 0.02, "step {step}: the block moved {drift} of a texel inside its texel");
        }
        assert!(pictures > 1, "the walk never left the margin, so it only tested one picture");
    }

    #[test]
    fn moving_the_render_origin_does_not_move_the_grid() {
        // The origin jumps by sixty-four blocks as the player walks. The
        // picture is kept across the jump and read through a camera
        // rebuilt for the new origin, and a block must be where it was.
        let sun = sun_at(0.62);
        let eye = Vec3::new(130.2, 80.0, -47.9);
        let block = Vec3::new(140.0, 70.0, -40.0);
        let mut cache = ShadowCache::default();
        let (a_origin, b_origin) = (Vec3::new(64.0, 0.0, -64.0), Vec3::new(128.0, 0.0, 0.0));
        let a = cache.plan(sun, eye.as_dvec3(), a_origin, RADIUS, RESOLUTION, 1.0, false, 0.0);
        let b = cache.plan(sun, eye.as_dvec3(), b_origin, RADIUS, RESOLUTION, 1.0, false, 1.0 / 60.0);
        assert!(!b.redraw, "a jump of the render origin retook the picture");
        let (a, b) = (texel_of(&a.view, block - a_origin), texel_of(&b.view, block - b_origin));
        assert!((a - b).abs().max_element() < 0.02, "the grid moved: {a} vs {b}");
    }

    #[test]
    fn under_a_moving_sun_the_grid_holds_still_between_steps_and_barely_moves_at_one() {
        // **The shimmer.** The picture turns with the sun, and the grid
        // used to be anchored at the world's zero: a million blocks out, a
        // sixtieth of a second of sun slid it a hundred texels under the
        // player, and every edge was rasterised at a new phase on every
        // frame. Now the sun moves in steps, nothing moves between them,
        // and a step turns the grid about the picture's centre -- so a
        // block a dozen blocks from the player moves a fraction of a texel.
        let origin = Vec3::new(1_000_000.0, 0.0, 1_000_000.0);
        let eye = Vec3::new(1_000_008.5, 72.0, 1_000_008.5);
        let near = Vec3::new(1_000_020.5, 70.0, 1_000_012.5);
        let mut cache = ShadowCache::default();
        let start = 0.4f32;
        let first = cache.plan(sun_at(start), eye.as_dvec3(), origin, RADIUS, RESOLUTION, 1.0, false, 0.0);
        let mut last = texel_of(&first.view, near - origin);
        let mut pictures = 0;
        // Three seconds at sixty frames: 1.2 degrees of sun.
        for frame in 1..180 {
            let t = start + frame as f32 / (DAY_SECONDS * 60.0);
            let plan = cache.plan(sun_at(t), eye.as_dvec3(), origin, RADIUS, RESOLUTION, 1.0, false, f64::from(frame) / 60.0);
            let at = texel_of(&plan.view, near - origin);
            let moved = (at - last).abs().max_element();
            if plan.redraw {
                pictures += 1;
                assert!(moved < 0.3, "frame {frame}: a step of the sun moved a block 12 blocks away by {moved} texels");
            } else {
                assert!(moved < 0.01, "frame {frame}: the grid moved under a sun that had not stepped: {moved}");
            }
            last = at;
        }
        assert!((10..=13).contains(&pictures), "1.2 degrees of sun took {pictures} pictures, not about twelve");
    }

    #[test]
    fn the_sun_the_shadows_are_drawn_from_is_never_far_from_the_true_one() {
        // A tenth of a degree is invisible in a shadow's direction; a
        // stepping that drifted further -- a wrong axis, a wrong wrap at
        // midnight -- would put every shadow at an angle to its caster.
        let mut key = SunKey::of(sun_at(0.26));
        for i in 0..4000 {
            let t = 0.26 + 0.48 * i as f32 / 4000.0;
            let sun = sun_at(t);
            key = key.follow(sun);
            let apart = key.beam().angle_between(sun.as_dvec3().normalize()).to_degrees();
            assert!(apart <= 0.08, "at {t} the shadows' sun is {apart} degrees from the sun");
        }
    }

    #[test]
    fn a_still_camera_under_a_still_sun_takes_the_picture_once() {
        let mut cache = ShadowCache::default();
        let origin = Vec3::new(64.0, 0.0, 64.0);
        let pictures = (0..600)
            .filter(|&frame| {
                let eye = Vec3::new(80.5, 70.0 + (frame % 7) as f32 * 0.01, 91.0);
                cache.plan(sun_at(0.45), eye.as_dvec3(), origin, RADIUS, RESOLUTION, 1.0, false, f64::from(frame) / 60.0).redraw
            })
            .count();
        assert_eq!(pictures, 1, "ten seconds of a player looking around took {pictures} pictures");
    }

    #[test]
    fn walking_retakes_the_picture_only_when_the_camera_leaves_the_margin() {
        let mut cache = ShadowCache::default();
        let origin = Vec3::new(0.0, 0.0, 0.0);
        let sun = sun_at(0.45);
        let mut pictures = 0;
        // Ten seconds at walking pace, 4.3 blocks a second.
        for frame in 0..600 {
            let eye = Vec3::new(8.0 + frame as f32 * 4.3 / 60.0, 70.0, 8.0);
            let plan = cache.plan(sun, eye.as_dvec3(), origin, RADIUS, RESOLUTION, 1.0, false, f64::from(frame) / 60.0);
            pictures += usize::from(plan.redraw);
            // ...and every frame's receivers are still inside the picture.
            for side in [-1.0, 1.0] {
                let edge = eye + Vec3::new(side * RADIUS * 0.98, 0.0, 0.0) - origin;
                let at = plan.view.project(edge);
                assert!(at.x.abs() <= 1.0 && at.y.abs() <= 1.0, "frame {frame}: a receiver fell off the picture: {at}");
            }
        }
        assert!((5..=8).contains(&pictures), "43 blocks of walking took {pictures} pictures");
    }

    /// **The ground around the camera is in the picture, and so is what
    /// shades it.** The turned camera kept a sphere; the oblique one keeps
    /// the ground within the radius at the camera's height, and a receiver
    /// above or below it slides across by `shear` times the height -- so
    /// the heights asked of here are the ones that slide no further than
    /// the margin's worth at each hour (see `LightView::around`).
    #[test]
    fn everything_around_the_camera_is_inside_the_map_with_its_casters() {
        let origin = Vec3::new(64.0, 0.0, 64.0);
        let drawn_at = Vec3::new(80.5, 70.0, 91.0);
        for t in [0.28, 0.35, 0.5, 0.65, 0.72] {
            let sun = sun_at(t);
            // The camera as far from the picture's centre as it can get
            // without the picture being retaken.
            let mut cache = ShadowCache::default();
            cache.plan(sun, drawn_at.as_dvec3(), origin, RADIUS, RESOLUTION, 1.0, false, 0.0);
            let eye = drawn_at + Vec3::new(1.0, 0.0, 1.0).normalize() * MARGIN * 0.99;
            let plan = cache.plan(sun, eye.as_dvec3(), origin, RADIUS, RESOLUTION, 1.0, false, 0.1);
            assert!(!plan.redraw, "the camera inside the margin retook the picture");
            let light = plan.view;
            let (sx, sz) = shear(SunKey::of(sun).beam());
            // The camera's walk takes all but a hundredth of the margin and
            // the radius's last fiftieth is the rest: the heights asked of
            // are the ones that slide a fifth of the margin.
            let height = (0.2 * f64::from(MARGIN) / sx.hypot(sz)).min(f64::from(RADIUS)) as f32;
            for i in 0..200 {
                // A scatter over the disc the receivers live in, at heights
                // across the band.
                let a = i as f32 * 2.399_963;
                let r = (i as f32 / 199.0).sqrt();
                let h = ((i * 7) % 200) as f32 / 199.0 * 2.0 - 1.0;
                let receiver = eye + Vec3::new(a.cos() * r * RADIUS * 0.98, h * height, a.sin() * r * RADIUS * 0.98);
                let at = light.project(receiver - origin);
                assert!(
                    at.x.abs() <= 1.0 && at.y.abs() <= 1.0 && (0.0..=1.0).contains(&at.z),
                    "at {t}, a receiver {} blocks away is off the map: {at}",
                    (receiver - eye).length()
                );
                // ...and whatever stands above it toward the sun, as
                // high as the world goes, is in the picture in front of
                // it rather than clipped by the near plane.
                if receiver.y < 200.0 {
                    let up_the_beam = (256.0 - receiver.y) / -sun.y;
                    let caster = receiver - sun * up_the_beam.min(150.0);
                    let seen = light.project(caster - origin);
                    assert!(seen.z >= 0.0 && seen.z < at.z, "at {t}, a caster was clipped: {seen}");
                }
            }
        }
    }

    #[test]
    fn a_mesh_change_in_the_shadows_retakes_the_picture_and_one_far_away_does_not() {
        let origin = Vec3::ZERO;
        let eye = Vec3::new(8.0, 70.0, 8.0);
        let sun = sun_at(0.3);
        let chunk = |x: f64, z: f64, top: f64| (DVec3::new(x, 0.0, z), DVec3::new(x + 16.0, top, z + 16.0));
        let mut cache = ShadowCache::default();
        cache.plan(sun, eye.as_dvec3(), origin, RADIUS, RESOLUTION, 1.0, false, 0.0);

        let (far_min, far_max) = chunk(4000.0, 4000.0, 90.0);
        cache.touch(far_min, far_max);
        assert!(!cache.plan(sun, eye.as_dvec3(), origin, RADIUS, RESOLUTION, 1.0, false, 0.1).redraw, "a chunk four kilometres off retook the picture");

        let (min, max) = chunk(0.0, 0.0, 90.0);
        cache.touch(min, max);
        assert!(cache.plan(sun, eye.as_dvec3(), origin, RADIUS, RESOLUTION, 1.0, false, 0.2).redraw, "a block broken under the player did not");

        // A hill 150 blocks toward a morning sun, out of the picture's
        // square but on the line its shadow comes down.
        let toward_sun = -Vec3::new(sun.x, 0.0, sun.z).normalize() * 150.0;
        let (hill_min, hill_max) = chunk(f64::from(toward_sun.x), f64::from(toward_sun.z), 250.0);
        cache.touch(hill_min, hill_max);
        assert!(cache.plan(sun, eye.as_dvec3(), origin, RADIUS, RESOLUTION, 1.0, false, 0.3).redraw, "a tall chunk toward a low sun did not");
    }

    #[test]
    fn moving_casters_retake_the_picture_thirty_times_a_second_whatever_the_frame_rate() {
        // Something moving in the world moves its shadow on every frame, and
        // the picture is retaken for it thirty times a second -- not on every
        // frame, and not at twenty. The first rule compared the interval
        // since the last picture with a thirtieth, and at sixty frames a
        // second a frame a hair early waited for a third: 45 pictures in two
        // seconds. So the clock here jitters, the way a real one does.
        let origin = Vec3::ZERO;
        let eye = Vec3::new(8.0, 70.0, 8.0);
        let sun = sun_at(0.45);
        // -1..1, from a fixed generator so the test is the same every run.
        let mut noise = 0x2545_f491_u32;
        let mut jitter = move || {
            noise ^= noise << 13;
            noise ^= noise >> 17;
            noise ^= noise << 5;
            f64::from(noise % 2001) / 1000.0 - 1.0
        };
        for fps in [240.0, 144.0, 60.0, 24.0] {
            let mut cache = ShadowCache::default();
            let period = 1.0 / fps;
            let frames = (2.0 * fps) as u32;
            let (mut pictures, mut drew_last) = (0, false);
            for frame in 0..frames {
                // A fifth of a frame either way, from a start that is not on
                // a tick.
                let now = 5.01 + f64::from(frame) * period + jitter() * period * 0.2;
                let drew = cache.plan(sun, eye.as_dvec3(), origin, RADIUS, RESOLUTION, 1.0, true, now).redraw;
                if fps > 60.0 && frame > 0 {
                    assert!(!(drew && drew_last), "{fps} fps: the picture was taken on two frames running at frame {frame}");
                }
                pictures += usize::from(drew);
                drew_last = drew;
            }
            if fps >= 30.0 {
                // Sixty ticks in two seconds, and the first picture.
                assert!((59..=62).contains(&pictures), "two seconds at {fps} fps with a herd took {pictures} pictures");
            } else {
                assert_eq!(pictures, frames as usize, "below thirty frames a second every frame is a new thirtieth");
            }
        }
    }

    /// **One crown, one shadow, at the step that promises a hard edge.**
    ///
    /// At [`Mode::Hard`] the sun is walked through the voxel volume, and
    /// there a leaf cell stops it *whole*: the volume holds a byte a cell
    /// and not a picture. The map answers wherever the walk cannot reach --
    /// past the volume's thirty-two cells, or past `SUN_COLUMNS` of run --
    /// and drawn with its alpha holes it turned the same crown into a lace
    /// that the comparison filter averaged into a smooth grey. One tree
    /// then cast a cell-sharp shadow ten blocks from the player and a soft
    /// blob forty blocks away, in one frame:
    /// "тени от листвы мягкие... а должна быть такой же чёткой по клеткам,
    /// как от блоков" (`what_a_canopy_casts` photographs both).
    ///
    /// So this states the agreement rather than the picture: whatever the
    /// volume does to a leaf, the picture the same step falls back on has
    /// to do as well.
    #[test]
    fn a_crown_stops_the_sun_whole_in_the_picture_wherever_it_does_in_the_volume() {
        use crate::engine::lamp_shadow::{SunRay, Volume};
        use primitive_shared::types::{BLOCK_AIR, BLOCK_LEAVES};
        // One layer of leaves across the volume, and a ray from under it.
        let volume = Volume::gather([0, 0, 0], |_, _, y0, column| {
            for (dy, slot) in column.iter_mut().enumerate() {
                *slot = if y0 + dy as i32 == 20 { BLOCK_LEAVES } else { BLOCK_AIR };
            }
        });
        assert_eq!(
            volume.sun_ray(Vec3::new(32.5, 4.5, 32.5), Vec3::new(0.02, 1.0, 0.03).normalize()),
            SunRay::Blocked,
            "the volume let the sun through a leaf, and everything below is about matching what it does"
        );
        assert!(
            !leaf_caster_is_cut_out(Mode::Hard),
            "the picture cuts a crown's holes out where the volume stops the sun with the whole cell"
        );
        assert!(leaf_caster_is_cut_out(Mode::Soft), "the Soft step lost the dapple under a tree");
    }

    /// **The Hard step never blends two texels of the picture together.**
    ///
    /// A filtered compare is a ramp across two texels, and a texel is a
    /// tenth of a block: on ground ten blocks away at a grazing angle that
    /// is eighteen screen pixels of gradient, against the one pixel the
    /// voxel walk gives for the same edge. Since the walk only reaches as
    /// far as the volume does, one crown threw a cell-sharp shadow near
    /// the player and a soft one past it -- "тени от листвы мягкие". The
    /// Hard step reads a texel and compares it itself; only Soft filters.
    #[test]
    fn the_hard_step_reads_one_texel_of_the_picture_and_the_soft_step_filters() {
        let source = include_str!("shader.wgsl");
        let start = source.find("fn sunlit_share(").expect("shader.wgsl has no `sunlit_share`");
        let end = source[start..].find("\n}").expect("`sunlit_share` never ends") + start;
        let body = &source[start..end];
        let hard = body.find("if (globals.shadow_bias.w > 0.5) {").expect("`sunlit_share` no longer splits on the step");
        let soft = body.find("} else {").expect("`sunlit_share` no longer has a soft branch");
        assert!(hard < soft, "the two branches swapped places");
        let (hard_branch, soft_branch) = (&body[hard..soft], &body[soft..]);
        assert!(
            hard_branch.contains("textureLoad(shadow_map, texel, 0)"),
            "the hard step no longer reads a texel: it is filtering again, which is the gradient the report was about"
        );
        assert!(!hard_branch.contains("textureSampleCompare"), "the hard step filters the picture");
        assert!(soft_branch.contains("textureSampleCompareLevel"), "the soft step stopped filtering, and its edge is its point");
    }

    #[test]
    fn a_canopy_casts_every_layer_of_itself_and_not_only_its_underside() {
        // **The leak through the leaves.** The mesher draws the plane two
        // leaf cells share once, as the lower cell's top face, so under a
        // sun that is up every plane inside a crown looks at the sun. The
        // leaves were drawn into the depth picture back faces only, like
        // the solid blocks -- and so a crown five deep cast one layer of
        // lace, its underside, and the ground under a tree was nearly as
        // bright as open ground.
        use primitive_shared::types::{Chunk, ChunkPos, BLOCK_AIR, BLOCK_LEAVES, CHUNK_VOLUME};
        let mut blocks = vec![BLOCK_AIR; CHUNK_VOLUME];
        for y in 80..85 {
            for z in 5..10 {
                for x in 5..10 {
                    blocks[Chunk::index(x, y, z)] = BLOCK_LEAVES;
                }
            }
        }
        let pos = ChunkPos::new(0, 0);
        let mut chunks = crate::logic::chunk_manager::ChunkManager::new(8);
        chunks.insert(Chunk { pos, blocks });
        let mut light = primitive_shared::lighting::LightMap::new();
        light.load_chunk(&chunks, pos);
        let mut cache = crate::engine::mesh::Neighbourhood::default();
        cache.fill(pos, &chunks, &light);
        let mut mesh = crate::engine::mesh::MeshBuffers::default();
        crate::engine::mesh::build_mesh(
            pos,
            &cache,
            &crate::engine::texture::FaceLayers::empty_for_test(),
            &primitive_shared::worldgen::WorldGen::new(1),
            &mut mesh,
        );
        let leaves = &mesh.indices[mesh.solid_index_count as usize..mesh.leaf_end as usize];
        assert!(!leaves.is_empty(), "the crown has no leaf faces");

        // What a pipeline with this cull mode keeps of a triangle wound
        // `facing` along the beam (positive: its front side looks away from
        // the sun). Counter-clockwise fronts, as every pipeline here.
        let keeps = |cull: Option<wgpu::Face>, facing: f32| match cull {
            None => true,
            Some(wgpu::Face::Front) => facing > 0.0,
            Some(wgpu::Face::Back) => facing < 0.0,
        };
        let beam = SunKey::of(sun_at(0.5)).beam().as_vec3();
        // A line down the noon sun through the middle of the crown, off
        // every diagonal, and the distinct planes it passes through.
        let through = Vec3::new(7.3, 82.5, 7.9);
        let mut kept_now = std::collections::HashSet::new();
        let mut kept_back_only = std::collections::HashSet::new();
        for triangle in leaves.chunks_exact(3) {
            let [a, b, c] = [0, 1, 2].map(|i| Vec3::from(mesh.vertices[triangle[i] as usize].position));
            let (e1, e2) = (b - a, c - a);
            let p = beam.cross(e2);
            let det = e1.dot(p);
            if det.abs() < 1e-6 {
                continue;
            }
            let s = through - a;
            let u = s.dot(p) / det;
            let v = beam.dot(s.cross(e1)) / det;
            if u < 0.0 || v < 0.0 || u + v > 1.0 {
                continue;
            }
            let normal = e1.cross(e2);
            let axis = if normal.x.abs() > normal.y.abs().max(normal.z.abs()) {
                0
            } else if normal.y.abs() > normal.z.abs() {
                1
            } else {
                2
            };
            let plane = (axis, (a[axis] * 16.0).round() as i32);
            let facing = normal.dot(beam);
            if keeps(LEAF_CASTER_CULL, facing) {
                kept_now.insert(plane);
            }
            if keeps(SOLID_CASTER_CULL, facing) {
                kept_back_only.insert(plane);
            }
        }
        assert!(
            kept_now.len() >= 6,
            "the noon sun crosses six planes of a five-deep crown and the leaf caster keeps {}",
            kept_now.len()
        );
        // Written down rather than left to the comment: this is what the
        // leaves were drawn with, and what it left of the crown.
        assert!(kept_back_only.len() <= 1, "back faces alone keep {} planes", kept_back_only.len());
    }

    #[test]
    fn a_shadow_falls_on_the_side_away_from_the_sun() {
        let origin = Vec3::ZERO;
        let eye = Vec3::new(8.0, 66.0, 8.0);
        for t in [0.3, 0.45, 0.55, 0.7] {
            let light = LightView::new(sun_at(t), eye, origin, RADIUS, RESOLUTION, 1.0);
            // The direction the picture was taken along, which is the
            // stepped sun's and the one a shadow lands along.
            let sun = light.direction;
            // A block's corner five up from the ground at y = 64, and the
            // point on the ground that shares its place in the map: that
            // is where its shadow lands.
            let caster = Vec3::new(8.0, 69.0, 8.0);
            let on_ground = caster + sun * (5.0 / -sun.y);
            let (a, b) = (light.project(caster), light.project(on_ground));
            assert!((a.truncate() - b.truncate()).length() < 1e-4, "not the same texel");
            assert!(b.z > a.z, "at {t} the ground is nearer the sun than what shades it");
            let toward_sun = Vec3::new(-sun.x, 0.0, -sun.z);
            assert!(
                (on_ground - caster).dot(toward_sun) < 0.0,
                "at {t} the shadow fell toward the sun"
            );
        }
    }

    /// **The mechanism of the smeared step, held as a property.** Every
    /// edge a block has along x or along z has to come out as a line of the
    /// picture's own grid -- one texel row or column along its whole length
    /// -- at any height and any hour. The turned camera laid those edges
    /// across the grid at thirty-odd degrees, and a one-block step's shadow,
    /// under four texels wide at 13:24, was voted over a staircase: a
    /// blotchy smear that did not follow the step.
    #[test]
    fn every_edge_a_block_has_along_x_or_z_is_a_line_of_the_pictures_grid() {
        let origin = Vec3::new(64.0, 0.0, -32.0);
        let eye = Vec3::new(80.3, 71.6, -20.9);
        for t in [0.27, 0.33, 0.45, 13.4 / 24.0, 0.66, 0.73] {
            let light = LightView::new(sun_at(t), eye, origin, RADIUS, RESOLUTION, 1.0);
            for height in [60.0, 72.0, 73.0, 95.0] {
                let at = |p: Vec3| texel_of(&light, p - origin);
                // An edge along x, five blocks long: its texel row is fixed.
                let (a, b) = (at(Vec3::new(70.0, height, -18.0)), at(Vec3::new(75.0, height, -18.0)));
                assert!((a.y - b.y).abs() < 1e-3, "at {t}, an edge along x at {height} crosses {} texel rows", (a.y - b.y).abs());
                // ...and one along z, its column.
                let (c, d) = (at(Vec3::new(77.0, height, -25.0)), at(Vec3::new(77.0, height, -20.0)));
                assert!((c.x - d.x).abs() < 1e-3, "at {t}, an edge along z at {height} crosses {} texel columns", (c.x - d.x).abs());
            }
        }
    }

    /// **The solid casters keep the faces turned away from the sun only if
    /// the picture winds a face turned to the sun counter-clockwise.**
    /// `SOLID_CASTER_CULL` culls front faces, and the pipeline calls a
    /// counter-clockwise triangle front. Laid out obliquely rather than
    /// through a turned camera, a picture that winds the other way would
    /// keep the lit faces instead -- every lit floor compared against itself.
    #[test]
    fn a_face_turned_to_the_sun_is_drawn_counter_clockwise_in_the_picture() {
        let origin = Vec3::ZERO;
        let eye = Vec3::new(8.0, 70.0, 8.0);
        for t in [0.28, 0.4, 0.5, 0.62, 0.72] {
            let light = LightView::new(sun_at(t), eye, origin, RADIUS, RESOLUTION, 1.0);
            let toward_sun = -light.direction;
            for face in crate::engine::mesh::faces() {
                let corners = face.corners.map(|c| Vec3::from(c) + Vec3::new(8.0, 70.0, 8.0));
                let normal = (corners[1] - corners[0]).cross(corners[2] - corners[1]).normalize();
                let facing = normal.dot(toward_sun);
                if facing.abs() < 0.05 {
                    continue;
                }
                let p = corners.map(|c| light.project(c).truncate());
                let area: f32 = (0..4).map(|k| p[k].perp_dot(p[(k + 1) % 4])).sum();
                assert_eq!(
                    area > 0.0,
                    facing > 0.0,
                    "at {t}, a face looking {normal:?} ({facing:+.2} toward the sun) is wound {area:+} in the picture"
                );
            }
        }
    }

    #[test]
    fn a_chunk_behind_the_camera_can_still_cast_in_front_of_it() {
        // The camera frustum is no guide to casters: a tree behind the
        // player puts its shadow at their feet in the evening.
        let origin = Vec3::ZERO;
        let eye = Vec3::new(8.0, 70.0, 8.0);
        let light = LightView::new(sun_at(0.7), eye, origin, RADIUS, RESOLUTION, 1.0);
        let behind = (Vec3::new(-24.0, 60.0, 0.0), Vec3::new(-8.0, 90.0, 16.0));
        assert!(light.might_shade(behind.0, behind.1));
        // ...while a chunk half a kilometre off along the map's side is
        // not asked to draw at all.
        let far = (Vec3::new(8.0, 60.0, 600.0), Vec3::new(24.0, 90.0, 616.0));
        assert!(!light.might_shade(far.0, far.1));
    }
}
