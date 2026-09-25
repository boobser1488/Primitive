//! The fog: what colour distance is, and where it starts.
//!
//! ## Why this is a module rather than four fields
//!
//! Fog is the one piece of this renderer that four other things all have
//! an opinion about, and until this file existed each of them held a
//! piece of it. The sky decided the colour, the settings decided the
//! range, `main` decided what happens under water, and the shaders
//! decided the curve -- and none of them could be read without the other
//! three, because the numbers only make sense together.
//!
//! They are together now. What a caller wants is one question -- *what
//! is the fog this frame* -- and it gets one answer.
//!
//! ## The invariant everything else leans on
//!
//! **The fog colour is what the world fades into, so it has to be what
//! is behind the world.** The frame is cleared to it, the sky's gradient
//! reaches it at the horizon, and the terrain shader mixes all the way
//! to it at `end`. Those three agreeing is the whole reason a limited
//! render distance does not read as a wall: terrain thins out into
//! exactly the colour that was already there.
//!
//! Break the agreement anywhere and the symptom is the same -- a visible
//! line drawn round the edge of the world -- which is why the colour has
//! one definition and three readers rather than three definitions.
//!
//! ## Under water it is not fog at all
//!
//! Air haze is a distance cue, and a player may switch it off. Water is
//! a *medium*: it absorbs red first and keeps absorbing with depth,
//! whether or not anybody wants a distance cue. So the two are separate
//! -- the shader applies absorption before the fog and independently of
//! the toggle -- and what this module supplies for the submerged case is
//! a much nearer range and the colour of the water.
//!
//! ## Under ground there was no distance at all
//!
//! The fade is built for the surface and it starts where the surface
//! needs it: at a render distance of ten, 88 blocks out. **No cave is
//! that big.** Measured on the generator, the longest straight line of
//! open air under ground in 140x140 blocks of seed 1337 is 23 blocks --
//! so the whole of a tunnel lies inside the near field, and the far end
//! of it was drawn at exactly the brightness of the wall two steps away.
//! A corridor with no depth in it, and where the fade *would* have
//! landed, had anything been far enough away, was the colour of the sky.
//!
//! So the colour is taken to black and the range is pulled in, by a
//! fraction that says how far under ground the eye is. Three things
//! about that fraction are deliberate:
//!
//! * **It is measured, not assumed from the height.** A depth threshold
//!   was the first idea and it is wrong in both directions: a ravine cut
//!   through a plain is thirty blocks down and open to the sky, and a
//!   cavern in a mountainside is a hundred blocks up and has never seen
//!   it. What is asked instead is whether anything opaque stands between
//!   the eye and the sky -- `types::blocks_the_sky`, the same rule the
//!   server's `climate::has_roof` shelters a player from rain with.
//! * **It is a share of nine columns, not one.** Asking only about the
//!   cell overhead makes the mouth of a cave a step function: one pace
//!   in and the world outside, which is right there in view through the
//!   opening, goes dark. Nine columns on a ring say *how enclosed* the
//!   place is, and the fade only starts once most of them are shut --
//!   so a cave mouth still looks out at daylight and it is the walk
//!   inward that closes the sky.
//! * **It settles over time.** Even nine columns flicker when a player
//!   walks under a ledge and out again, and a fog colour that flickers
//!   is far more noticeable than one that is slightly late.
//!
//! What this deliberately is *not*: any of volumetric fog, a visibility
//! set, or a light-aware haze that torches push back. All three want the
//! shader to know where a pixel is relative to the openings, which is a
//! per-pixel question needing per-pixel data. This is one colour and two
//! distances a frame, which the shader already takes.

use glam::Vec3;

use crate::engine::sky::Sky;
use crate::engine::water::WaterTint;
use crate::logic::chunk_manager::{ChunkManager, NEIGHBOUR_OFFSETS};
use crate::settings::ClientSettings;
use primitive_shared::types::{blocks_the_sky, CHUNK_SIZE_Y};

// **The colour deep water closes in to used to be a constant here**, one
// teal for every water in the world: `(0.055, 0.154, 0.209)`, and before
// that `(0.10, 0.28, 0.38)` while `fs_sky` painted 55% of it and the
// terrain's murk trended to 55% of it too -- so everything the fog had
// finished with, eighteen blocks out, landed on a colour brighter than what
// was behind it, and the far bed and the underside of the surface stood out
// of the water as flat bright bands with the darker sky between them. The
// 55% was the colour a swimmer had been looking at all along, so it became
// the colour, and both shaders read it unscaled.
//
// It is `water::WaterTint::murk` now -- the water's own colour, at the
// luminance that constant had, so every measurement taken of a temperate
// lake still holds and only the hue moved. What has *not* changed is the
// rule that put the number here in the first place: the fog, the sky pass
// under water and the terrain's murk are one colour or they are bands.

/// Where the underwater fog starts, as a fraction of where it ends.
///
/// Much nearer than the fifth or so air uses. Under water there is no
/// clear near field: the medium is between the eye and everything,
/// including what is an arm's length away.
const UNDERWATER_START: f32 = 0.15;

/// What distance is, under ground.
///
/// Black, and exactly black. The alternative considered was a very dark
/// stone grey, on the theory that a floor of 0.02 or so hides banding in
/// the fade; it does not, because the fade runs into rock that is itself
/// at the ambient floor and the two are within a value of each other.
/// What a non-zero floor does do is show up in the one place the fog
/// colour is not fog -- the frame is cleared to it and the sky's
/// gradient is built from it -- as a grey wash in the gap of a cave
/// mouth, which reads as fog on a clear day.
const UNDERGROUND: Vec3 = Vec3::ZERO;

/// Where the fade ends under ground, in blocks.
///
/// **Twenty-four, and it is short because caves are short.** The first
/// try was forty-eight, on the reasoning that a chamber should have a
/// far side; it does nothing at all. Measured on the generator itself,
/// over every standing spot with rock overhead in 140x140 blocks of
/// seed 1337, **the longest straight line of open air under ground is
/// 23 blocks**. A fade that finishes at 48 finishes past the far wall
/// of every cave in that world -- photographed from the chamber under
/// the mountain, the frame moved by at most 9 values out of 255, on
/// 0.15% of its samples. That is not a mechanic, it is a rounding
/// error.
///
/// At 24 the same corridor (`/tp 30 33 -26`, looking east down 21
/// blocks of tunnel) changes on 40% of its samples, and the far end
/// goes to black while the near floor keeps its texture -- which is the
/// thing that was asked for.
///
/// The floor under the number is the light the player carries. A torch
/// reaches 15 blocks (`MAX_LIGHT`), the fade begins at a third of the
/// range and is quadratic, so at the edge of torchlight it is 18% of
/// the way to black: **what you have lit, you can see.** At 16 -- also
/// photographed -- the tunnel is gone by 12 blocks, which is inside the
/// torchlight, and walking becomes guessing which of three black
/// openings has a floor.
const UNDERGROUND_END: f32 = 24.0;

/// Where it begins, as a fraction of where it ends.
///
/// Later than water's and much earlier than air's. Rock is not a medium
/// -- there is nothing between the eye and the wall two metres away, and
/// dimming it would look like grease on the lens -- but the black has to
/// arrive well before the end or the fade reads as a curtain hung at a
/// fixed distance. A third of 24 is eight blocks, which leaves sixteen
/// of ramp: the near half of a room keeps its texture and the far half
/// goes.
const UNDERGROUND_START: f32 = 0.34;

/// The colour of woodsmoke filling a room: a warm grey, the particles'
/// darkest puff (`particles::SMOKE_GREYS`) lifted to the brightness of a
/// room a fire is lighting. Not black: a smoky hut is a hut you can still
/// see the fire in, dimly.
const SMOKE: Vec3 = Vec3::new(0.30, 0.28, 0.26);

/// How far a player can see through the thickest smoke, in blocks. Four:
/// the other side of a small hut is a shape, and the door is a lighter
/// patch -- which is the thing to find.
const SMOKE_END: f32 = 4.0;

/// The colour of a mist over sodden ground at first light: nearly white
/// and a shade cold, because it is lit by a sun that is not up yet.
const MIST: Vec3 = Vec3::new(0.80, 0.83, 0.86);

/// How far a player can see through the thickest of it, in blocks.
///
/// **Thirty-two, which is a couple of chunks**: far enough to keep
/// walking and near enough that the wood on the other side of a bog is
/// gone. A mist that closed to the smoke's four blocks would be a room
/// out of doors, and a player would stop rather than go on -- and a
/// weather that makes the answer "wait an hour" is a chore rather than a
/// decision.
const MIST_END: f32 = 32.0;

/// How far from the eye the ring of columns is asked about the sky, in
/// blocks.
///
/// Six. It has to clear the *width of a cave mouth* -- the whole point
/// of sampling more than one column -- and stop short of the width of a
/// building, because a player standing in their own house is under a
/// roof by every test in this file and would rather not be told the
/// world outside their window is a cave. Six puts the ring outside
/// anything up to about thirteen blocks across, which is a house; a hall
/// bigger than that reads as underground, and looks it.
const SAMPLE_RADIUS: i32 = 6;

/// How much of the ring has to be shut before the sky starts to go.
///
/// Five columns of the nine. Below that the place is a mouth, an
/// overhang or a ledge, all of which are outdoors with something over
/// them, and all of which have daylight in view -- darkening the fog
/// there would darken the daylight, because there is one fog colour per
/// frame and the sky is drawn from it.
const ENCLOSED_FROM: f32 = 0.55;

/// How long the change takes to arrive, as a time constant in seconds.
///
/// Sampled per frame, applied per second: the fraction moves a share of
/// the way each frame, `1 - exp(-dt/tau)`, so a machine at 30 fps and a
/// machine at 300 make the same journey in the same *time*. Doing this
/// per frame instead -- the obvious `f += (target - f) * 0.05` -- ties
/// how fast a cave gets dark to how fast the computer is.
///
/// Three quarters of a second: a walk into a cave darkens over about two
/// seconds, which is slow enough that stepping under a ledge and out
/// again produces nothing worth looking at, and quick enough that it
/// never feels like the game noticed late.
const SETTLE_SECONDS: f32 = 0.75;

/// How far under ground the eye is, between frames.
///
/// Kept by the caller across frames because it is the *only* part of the
/// fog with a memory -- see [`Underground::advance`] for why it has to
/// have one.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Underground {
    fraction: f32,
}

impl Underground {
    /// Looks around, and moves the fraction a frame's worth toward what
    /// it found.
    ///
    /// The sampling is nine columns of at most sixty-three cells, which
    /// is one chunk lookup per column and a walk through memory that is
    /// already hot -- the mesher read the same chunks this frame.
    ///
    /// **Measured rather than assumed: 1.4 us a call**, release build,
    /// standing on open ground so that every one of the nine columns
    /// runs to the top of the world -- the worst case there is, since a
    /// roof stops the walk at the first opaque cell. That is 0.008% of a
    /// frame at 60 fps, which is why this is not budgeted and not run on
    /// an interval. An interval would be a second time constant fighting
    /// the one below it, for a saving too small to find.
    pub fn advance(&mut self, chunks: &ChunkManager, eye: Vec3, dt: f32) -> f32 {
        self.approach(Self::enclosure(chunks, eye), dt)
    }

    /// The smoothing on its own, so a test can drive it without a world.
    fn approach(&mut self, target: f32, dt: f32) -> f32 {
        // Clamped because `dt` is wall time and a frame that waited on a
        // disk, a resume or a breakpoint would otherwise jump the fog
        // straight to its target -- which is the flicker this smoothing
        // exists to prevent, arriving by the back door.
        let dt = dt.clamp(0.0, 1.0);
        let step = 1.0 - (-dt / SETTLE_SECONDS).exp();
        self.fraction += (target.clamp(0.0, 1.0) - self.fraction) * step;
        self.fraction = self.fraction.clamp(0.0, 1.0);
        self.fraction
    }

    /// How shut in this spot is: the share of the ring with a roof over
    /// it, curved so that a mouth is outdoors and only a closed place is
    /// fully under ground.
    fn enclosure(chunks: &ChunkManager, eye: Vec3) -> f32 {
        let (gx, gy, gz) = (
            eye.x.floor() as i32,
            eye.y.floor() as i32,
            eye.z.floor() as i32,
        );
        let mut roofed = 0u32;
        for (dx, dz) in std::iter::once((0, 0)).chain(
            NEIGHBOUR_OFFSETS
                .iter()
                .map(|(dx, dz)| (dx * SAMPLE_RADIUS, dz * SAMPLE_RADIUS)),
        ) {
            if Self::roofed(chunks, gx + dx, gy, gz + dz) {
                roofed += 1;
            }
        }
        let share = roofed as f32 / (NEIGHBOUR_OFFSETS.len() + 1) as f32;
        // Smoothstep rather than a straight ramp so that the last column
        // to close does not land the fog on full black with one step of
        // a foot: the curve is flat at both ends, which is where the
        // player is standing still and looking.
        let t = ((share - ENCLOSED_FROM) / (1.0 - ENCLOSED_FROM)).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    }

    /// Is anything opaque between this cell and the sky?
    ///
    /// The walk goes to the top of the world rather than stopping a
    /// fixed distance up, which is where it differs from
    /// `climate::has_roof` -- and the difference is cost, not meaning.
    /// The server pays for that walk per player per sample and cuts it
    /// off at sixteen; this is one eye once a frame, over a world only
    /// sixty-four cells tall. Cutting it off would say "outdoors" about
    /// the middle of a cavern whose ceiling happens to be high, which is
    /// the one place a player is most sure they are not.
    ///
    /// An unloaded column counts as open sky, the same answer climate
    /// gives and for a stricter reason: the alternative is that terrain
    /// arriving late paints the world black for as long as it takes.
    fn roofed(chunks: &ChunkManager, gx: i32, gy: i32, gz: i32) -> bool {
        let Some(column) = chunks.column(gx, gz) else {
            return false;
        };
        ((gy + 1)..CHUNK_SIZE_Y as i32).any(|y| blocks_the_sky(column.block(y)))
    }
}

/// The fog for one frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fog {
    /// What distance is the colour of, and what the frame is cleared to.
    pub color: Vec3,
    /// Where it begins, in blocks.
    pub start: f32,
    /// Where it is complete, in blocks. Terrain past this is exactly
    /// `color`, which is why it can be culled -- see the renderer.
    pub end: f32,
    /// Whether the player has it switched on at all.
    ///
    /// Off means the *air* haze is off. It does not turn off the water,
    /// which is not a preference.
    pub enabled: bool,
    /// Whether the eye is under water.
    pub underwater: bool,
    /// How much of the sky is shut out where the eye is, 0..1.
    ///
    /// Carried on the frame's fog rather than left behind in
    /// `go_underground` because one other answer depends on it: see
    /// `cull_distance`, where the short range a cave gets is exactly
    /// what makes throwing chunks away unsafe.
    underground: f32,
    /// The sunset toward the sun: `Sky::horizon_glow` for the player's
    /// lighting step, or zero where there is no sky to glow.
    ///
    /// **Part of the fog, not only of the sky**, because the fog is what
    /// the world fades into and the sky's horizon has to be the same
    /// colour as the fade in every direction. A glow drawn in the sky and
    /// not in the fog puts a bright band exactly where the terrain ends;
    /// so both shaders take the fog colour and mix this into it by the
    /// same function of direction, and the edge of the world stays
    /// invisible toward the sun as well as away from it.
    pub glow: glam::Vec4,
    /// Light scattered around the sun (`Sky::sun_haze`), added to the fog
    /// colour in the sun's direction for the same reason as `glow`.
    pub haze: glam::Vec4,
}

impl Fog {
    /// The fog for this frame, from the sky, the settings and where the
    /// player's head is.
    /// `underwater` is `None` in the air and `Some(water)` when the head
    /// is in one -- **and which water it is matters**, because the murk is
    /// that water's own colour now and not one teal for the whole world.
    /// See `water::WaterTint::murk`.
    pub fn for_frame(
        settings: &ClientSettings,
        sky: &Sky,
        render_distance_chunks: i32,
        enabled: bool,
        underwater: Option<WaterTint>,
    ) -> Self {
        let submerged = underwater.is_some();
        let (start, end) = if submerged {
            let end = settings.underwater_fog_distance;
            (end * UNDERWATER_START, end)
        } else {
            settings.fog_range(render_distance_chunks)
        };

        // Under water there is no horizon to glow: the colour is the
        // water's, and a sunset mixed into it would put orange in a lake.
        let (glow, haze) = if submerged {
            (glam::Vec4::ZERO, glam::Vec4::ZERO)
        } else {
            (sky.horizon_glow(settings.lighting), sky.sun_haze(settings.lighting))
        };
        Self {
            color: Self::color(sky, underwater, settings.lighting),
            start,
            end,
            enabled,
            underwater: submerged,
            underground: 0.0,
            glow,
            haze,
        }
    }

    /// Pulls the fade in to where the world actually stops.
    ///
    /// **The fog's job is to be finished before the terrain is.** The
    /// range comes from the render distance the player chose, and that
    /// figure is the radius along the axes; the streamer keeps a *disc*
    /// of that radius measured in whole chunks (see
    /// `ChunkManager::inside`), which reaches a little less far into the
    /// diagonals. Left alone, the fade would still be a tenth from done
    /// where the last chunk ends, and the gap under it is open sky --
    /// the exact line round the edge of the world that the whole module
    /// note is about.
    ///
    /// So the reach is handed in and the fade is clamped to it. It costs
    /// the diagonal corner of the view, which is terrain the player
    /// never had: what was there before this was drawn *past* the fog.
    ///
    /// Under water this does nothing, and should not: that range is
    /// metres of murk rather than a horizon, and it is nearer than any
    /// disc.
    pub fn clamp_to(&mut self, reach_blocks: f32) {
        if !reach_blocks.is_finite() || reach_blocks <= 0.0 || reach_blocks >= self.end {
            return;
        }
        self.end = reach_blocks;
        // The near edge follows, keeping the same shape of fade rather
        // than a fog that starts after it has finished. A band of at
        // least a few blocks, or a step reads as a wall.
        self.start = self.start.min(self.end - 8.0).max(0.0);
    }

    /// Takes the fog under ground by `fraction` (see [`Underground`]).
    ///
    /// A second pass over an already-built fog rather than an argument
    /// to [`for_frame`](Self::for_frame), which is how `clamp_to` works
    /// and for the same reason: what is under ground is a fact about the
    /// *world around the eye*, and `for_frame` is given the sky and the
    /// settings and nothing that could answer it. The offscreen tools in
    /// the renderer build a fog with no world at all, and this way they
    /// do not have to pass a zero they have no opinion about.
    ///
    /// Three things it refuses to do:
    ///
    /// * **Nothing at all with the fog switched off.** The colour is not
    ///   only the fog's: the frame is cleared to it and the sky's whole
    ///   gradient is built from it, so blackening it with the haze
    ///   turned off would black out a clear sky -- and F would have
    ///   stopped being a switch.
    /// * **It never pushes the fade further out.** A player at a render
    ///   distance of two already has fog nearer than this; walking into
    ///   a cave must not hand them extra view.
    /// * **Under water it takes the colour and leaves the range.** The
    ///   water is the medium and it decides how far anything can be
    ///   seen; what the rock decides is that there is no daylight coming
    ///   through the water to be green, which is the difference between
    ///   a flooded cave and a lake at noon.
    pub fn go_underground(&mut self, fraction: f32) {
        if !self.enabled {
            return;
        }
        let fraction = if fraction.is_finite() {
            fraction.clamp(0.0, 1.0)
        } else {
            return;
        };
        self.color = self.color.lerp(UNDERGROUND, fraction);
        // The sunset goes with the daylight it belongs to. Left alone it
        // would be the one colour that survives into a cave, and the far
        // end of a tunnel facing west would glow orange out of the black.
        self.glow.w *= 1.0 - fraction;
        self.haze *= 1.0 - fraction;
        self.underground = fraction;
        if self.underwater {
            return;
        }
        let end = self.end + (self.end.min(UNDERGROUND_END) - self.end) * fraction;
        let start = self.start + (end * UNDERGROUND_START - self.start) * fraction;
        self.end = end;
        self.start = start.min(end - 1.0).max(0.0);
    }

    /// Fills the air with the smoke of a fire in a closed room, by
    /// `thickness` 0..1 (`ServerMessage::Smoke`).
    ///
    /// **Whether or not the haze is switched on**, which is the one way this
    /// is not `go_underground`: smoke is a medium, like water, and a player
    /// who turned distance haze off did not turn off the air they are
    /// choking on -- it is the only warning the room gives before the breath
    /// meter does. It pulls the fade in and never pushes it out, and it
    /// leaves the water alone: a head under water is not in the smoke.
    pub fn fill_with_smoke(&mut self, thickness: f32) {
        if self.underwater || !thickness.is_finite() || thickness <= 0.0 {
            return;
        }
        let thickness = thickness.clamp(0.0, 1.0);
        self.color = self.color.lerp(SMOKE, thickness);
        self.glow.w *= 1.0 - thickness;
        self.haze *= 1.0 - thickness;
        let end = if self.enabled { self.end } else { self.end.max(SMOKE_END * 16.0) };
        let end = end + (end.min(SMOKE_END) - end) * thickness;
        self.end = end;
        self.start = (self.start * (1.0 - thickness)).min(end - 1.0).max(0.0);
        // Off, the shader would not fade at all: smoke turns it on.
        self.enabled = true;
    }

    /// Lays the dawn mist of a bog over the view, 0..1
    /// (`weather::dawn_mist`).
    ///
    /// **Smoke's machinery, at a tenth of its strength and in another
    /// colour.** A room full of smoke takes the view to four blocks and
    /// is a warning; a mist takes it to [`MIST_END`], which is far
    /// enough to walk in and near enough that a bog at first light is a
    /// place you can get lost in. Pale and slightly cold rather than
    /// sooty, because what a mist is made of is the air itself.
    ///
    /// Like the smoke it only ever pulls the fade in, and it leaves the
    /// water alone: a head under water is not in the mist either. Unlike
    /// the smoke it does **not** force the fade on for a player who
    /// turned distance haze off -- nothing hangs on a mist (see
    /// `weather::dawn_mist` on why it draws and does not decide), so a
    /// setting somebody chose is left alone.
    pub fn lie_as_mist(&mut self, thickness: f32) {
        if self.underwater || !self.enabled || !thickness.is_finite() || thickness <= 0.0 {
            return;
        }
        let thickness = thickness.clamp(0.0, 1.0);
        self.color = self.color.lerp(MIST, thickness);
        self.glow.w *= 1.0 - 0.5 * thickness;
        let end = self.end + (self.end.min(MIST_END) - self.end) * thickness;
        self.end = end;
        self.start = (self.start * (1.0 - thickness)).min(end - 1.0).max(0.0);
    }

    /// What distance is the colour of.
    ///
    /// Tinted toward the sky rather than being a grey, so distant
    /// terrain dissolves into the horizon instead of into a band. That
    /// is the whole trick to making a limited render distance not look
    /// like one.
    ///
    /// Desaturated a little on the way, because fog is not sky: air
    /// scatters everything, so a long look through it trends toward its
    /// own brightness rather than keeping the sky's colour at full
    /// strength.
    fn color(
        sky: &Sky,
        underwater: Option<WaterTint>,
        lighting: crate::engine::lighting::Quality,
    ) -> Vec3 {
        if let Some(water) = underwater {
            // **As dark as the hour.** This was `UNDERWATER` and nothing
            // else, and everything it is mixed with is not a constant: the
            // bed, the lid and the terrain are lit by the sky, and at
            // midnight they are black. So the murk -- up to 85% of the
            // colour a swimmer sees, and all of the sky pass under water --
            // stayed the teal of noon while the world above went dark, and a
            // lake at night was the brightest thing in it: a glowing cyan
            // box under a black surface. Scaled by how bright the sky is
            // now against a clear day, which is what the fog in the air
            // already follows, and by the same number the weather and the
            // moon move. Noon in plain water is `UNDERWATER`'s luminance
            // exactly, so the day, and every measurement taken of it, is as
            // bright as it was.
            //
            // **And it is this water's colour and not one teal for the
            // world.** `UNDERWATER` was a constant, which meant a swimmer
            // in a peat bog and a swimmer in a glacier lake were in the
            // same water -- the one place a colour by biome shows most,
            // because under water the murk is most of the frame. The hue
            // comes off the same palette the surface is painted from
            // (`water::WaterTint::murk`), which is what stops the lid a
            // swimmer is looking up at being one water and the haze under
            // it another.
            let luma = |c: Vec3| c.dot(Vec3::new(0.2126, 0.7152, 0.0722));
            let now = luma(sky.sky_color_for(lighting)) / luma(crate::engine::sky::DAY_SKY);
            return water.murk() * now.clamp(0.0, 1.0);
        }

        let sky = sky.sky_color_for(lighting);
        let grey = Vec3::splat(sky.length() / 3f32.sqrt());
        sky.lerp(grey, 0.15)
    }

    /// How far a chunk may be before none of it can change a pixel.
    ///
    /// Past `end` the fog is complete, and the frame was *cleared* to
    /// the same colour -- so a chunk out there produces exactly what is
    /// already in the buffer. The renderer culls against this, and it is
    /// not a trade: the picture is identical.
    ///
    /// `None` where nothing is invisible at distance. With the fog off
    /// there is no such distance -- under water included, where the murk
    /// still dims the far terrain but no longer lands it on the sky.
    ///
    /// **Under water, with the fog on, there is one now.** It used to be
    /// `None` there as well, because a submerged chunk finished on the fog
    /// colour while the sky behind it was painted at 55% of it, so a culled
    /// chunk would have left a dark hole. The note that said so also said
    /// nothing was lost because the frustum threw almost everything away,
    /// and that part was never true: the frustum reaches the whole streamed
    /// disc, and every chunk in front of a swimmer was drawn to land on one
    /// flat colour eighteen blocks out. With the colours made one
    /// (`UNDERWATER`), what lies past `end` is exactly the sky behind it.
    ///
    /// **And `None` under ground, which is the one that took working
    /// out.** The claim above -- that a chunk past `end` produces what
    /// is already in the buffer -- holds because `end` has always been
    /// at or beyond where the streamed world stops (`clamp_to`), so the
    /// cull threw away nothing that was ever drawn. A cave's range is
    /// twenty-four blocks inside a world loaded to a hundred and sixty,
    /// and there the cull would remove real walls -- and what is behind
    /// a removed wall is not the fog colour. It is the sky pass, which
    /// carries the sun, the moon, the clouds and the stars, and the fog
    /// colour multiplies none of them. The symptom would be a sun-shaped
    /// hole in a rock face, sixty blocks down.
    pub fn cull_distance(&self) -> Option<f32> {
        (self.enabled && self.underground <= 0.0).then_some(self.end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use primitive_shared::types::{
        BlockId, Chunk, ChunkPos, BLOCK_AIR, BLOCK_LEAVES, BLOCK_STONE, BLOCK_WATER, CHUNK_SIZE_X,
        CHUNK_SIZE_Z, CHUNK_VOLUME,
    };

    fn sky_at(time: f32) -> Sky {
        Sky::new(time, 600.0)
    }

    /// A 3x3 patch of loaded chunks around the origin, all air, with a
    /// floor -- everything under an open sky until a test roofs part of
    /// it over.
    ///
    /// Nine chunks rather than one because the ring of samples reaches
    /// six blocks out, and a fixture one chunk wide would answer "not
    /// loaded" -- which is "open sky" -- for a third of the ring
    /// wherever the eye stood near an edge. That is the fixture lying
    /// about the thing under test.
    fn open_world() -> ChunkManager {
        let mut chunks = ChunkManager::new(4);
        for cz in -1..=1 {
            for cx in -1..=1 {
                let mut chunk = Chunk {
                    pos: ChunkPos::new(cx, cz),
                    blocks: vec![BLOCK_AIR; CHUNK_VOLUME],
                };
                for lz in 0..CHUNK_SIZE_Z {
                    for lx in 0..CHUNK_SIZE_X {
                        chunk.set(lx, 0, lz, BLOCK_STONE);
                    }
                }
                chunks.insert(chunk);
            }
        }
        chunks
    }

    /// Lays `block` over a square of `half_width` blocks either side of
    /// the origin, at height `y`.
    fn roof_over(chunks: &mut ChunkManager, half_width: i32, y: usize, block: BlockId) {
        let mut patched: Vec<primitive_shared::packed::PackedChunk> = Vec::new();
        for cz in -1..=1 {
            for cx in -1..=1 {
                let pos = ChunkPos::new(cx, cz);
                let mut chunk = chunks.get(pos).expect("fixture chunk").clone();
                for lz in 0..CHUNK_SIZE_Z {
                    for lx in 0..CHUNK_SIZE_X {
                        let gx = cx * CHUNK_SIZE_X as i32 + lx as i32;
                        let gz = cz * CHUNK_SIZE_Z as i32 + lz as i32;
                        if gx.abs() <= half_width && gz.abs() <= half_width {
                            chunk.set(lx, y, lz, block);
                        }
                    }
                }
                patched.push(chunk);
            }
        }
        for chunk in patched {
            chunks.insert(chunk);
        }
    }

    /// Where the fraction settles, given a world -- the smoothing run
    /// out rather than sampled once, because one frame of it is by
    /// design nowhere near the answer.
    fn settled(chunks: &ChunkManager, eye: Vec3) -> f32 {
        let mut under = Underground::default();
        // Six seconds of frames: eight time constants, which is as
        // settled as an exponential gets before the float stops caring.
        let mut at = 0.0;
        for _ in 0..360 {
            at = under.advance(chunks, eye, 1.0 / 60.0);
        }
        at
    }

    #[test]
    fn a_deep_cave_is_under_ground_and_a_ravine_in_the_open_is_not() {
        // The whole reason this is not a height test. Both eyes are at
        // the same depth; one has rock over it and one has sky.
        let mut roofed = open_world();
        roof_over(&mut roofed, 30, 40, BLOCK_STONE);
        assert!(
            settled(&roofed, Vec3::new(0.5, 10.0, 0.5)) > 0.99,
            "rock overhead in every direction is as under ground as it gets"
        );
        assert_eq!(
            settled(&open_world(), Vec3::new(0.5, 10.0, 0.5)),
            0.0,
            "a cut in the ground open to the sky is not a cave"
        );
    }

    #[test]
    fn a_canopy_is_not_a_roof_and_a_pane_of_water_is_not_either() {
        // The same rule the server shelters a player from rain with:
        // a forest is outdoors, and so is the bottom of a pond. Get this
        // wrong and every wood in the game goes black at midday.
        let mut leaves = open_world();
        roof_over(&mut leaves, 30, 40, BLOCK_LEAVES);
        assert_eq!(settled(&leaves, Vec3::new(0.5, 10.0, 0.5)), 0.0);

        let mut pond = open_world();
        roof_over(&mut pond, 30, 40, BLOCK_WATER);
        assert_eq!(settled(&pond, Vec3::new(0.5, 10.0, 0.5)), 0.0);
    }

    #[test]
    fn a_hut_is_not_a_cave_but_a_hall_is() {
        // A player under their own roof is looking out of a window at a
        // world that has to still be there. The line is the width of the
        // ring: a nine-block hut is a hut, and a hall thirty across is
        // somewhere the sky genuinely cannot be seen from.
        let mut hut = open_world();
        roof_over(&mut hut, 4, 20, BLOCK_STONE);
        assert_eq!(settled(&hut, Vec3::new(0.5, 10.0, 0.5)), 0.0);

        let mut hall = open_world();
        roof_over(&mut hall, 15, 20, BLOCK_STONE);
        assert!(settled(&hall, Vec3::new(0.5, 10.0, 0.5)) > 0.99);
    }

    #[test]
    fn the_mouth_of_a_cave_still_looks_out_at_daylight() {
        // Half the ring open is a mouth, an overhang or a ledge, and
        // there is one fog colour a frame -- so blackening it there
        // would black out the sky that is visible through the opening.
        let mut mouth = open_world();
        // Roofed on one side of the eye only.
        let mut patched: Vec<primitive_shared::packed::PackedChunk> = Vec::new();
        for cz in -1..=1 {
            for cx in -1..=1 {
                let pos = ChunkPos::new(cx, cz);
                let mut chunk = mouth.get(pos).expect("fixture chunk").clone();
                for lz in 0..CHUNK_SIZE_Z {
                    for lx in 0..CHUNK_SIZE_X {
                        let gx = cx * CHUNK_SIZE_X as i32 + lx as i32;
                        if gx < 0 {
                            chunk.set(lx, 20, lz, BLOCK_STONE);
                        }
                    }
                }
                patched.push(chunk);
            }
        }
        for chunk in patched {
            mouth.insert(chunk);
        }
        assert_eq!(settled(&mouth, Vec3::new(0.5, 10.0, 0.5)), 0.0);
    }

    #[test]
    fn walking_into_a_cave_darkens_over_seconds_rather_than_frames() {
        // A fog that switched would flicker under every ledge; a fog
        // that fades by a fixed share per *frame* would darken twice as
        // fast on a machine drawing twice as many.
        let mut slow = Underground::default();
        let mut fast = Underground::default();
        let mut after_a_second_at_30 = 0.0;
        let mut after_a_second_at_240 = 0.0;
        for _ in 0..30 {
            after_a_second_at_30 = slow.approach(1.0, 1.0 / 30.0);
        }
        for _ in 0..240 {
            after_a_second_at_240 = fast.approach(1.0, 1.0 / 240.0);
        }
        assert!(
            (after_a_second_at_30 - after_a_second_at_240).abs() < 0.01,
            "one second of walking is one second of walking: {after_a_second_at_30} vs {after_a_second_at_240}"
        );
        assert!(
            after_a_second_at_30 < 0.95,
            "a second should not be most of the way there"
        );

        // A frame that stalled -- a load, a resume, a breakpoint -- must
        // not be a jump cut.
        let mut stalled = Underground::default();
        assert!(
            stalled.approach(1.0, 30.0) < 0.95,
            "a long frame is still a frame"
        );
    }

    #[test]
    fn out_of_doors_the_fog_is_not_touched_at_all() {
        // The surface is the case that must not move: a fraction of
        // zero has to leave the frame bit-identical, or every meadow in
        // the game pays for a mechanic about caves.
        let settings = ClientSettings::default();
        for hour in [0.0, 0.25, 0.5, 0.75] {
            for underwater in [None, Some(WaterTint::PLAIN)] {
                let plain = Fog::for_frame(&settings, &sky_at(hour), 12, true, underwater);
                let mut same = plain;
                same.go_underground(0.0);
                assert_eq!(same, plain, "at {hour} the open air moved");
            }
        }
    }

    #[test]
    fn under_ground_the_distance_is_black_and_much_nearer() {
        let settings = ClientSettings::default();
        let surface = Fog::for_frame(&settings, &sky_at(0.5), 12, true, None);
        let mut cave = surface;
        cave.go_underground(1.0);
        assert!(cave.end < surface.end, "the cave has to close the world in");
        assert!(cave.start < cave.end, "the fog starts after it ends");
        assert!(
            cave.color.length() < 0.01,
            "distance under ground is the colour of nothing, not of the sky"
        );

        // Half way in it is half way there, rather than either end.
        let mut halfway = surface;
        halfway.go_underground(0.5);
        assert!(halfway.end < surface.end && halfway.end > cave.end);
        assert!(halfway.color.length() < surface.color.length());
        assert!(halfway.color.length() > cave.color.length());
    }

    #[test]
    fn a_cave_never_hands_back_more_view_than_the_open_air_had() {
        // At a short render distance the surface fog is already nearer
        // than the cave's own range, and walking into a cave must not be
        // a way to see further than the setting allows.
        let settings = ClientSettings::default();
        for chunks in [2, 4, 8, 16, 24] {
            let surface = Fog::for_frame(&settings, &sky_at(0.5), chunks, true, None);
            let mut cave = surface;
            cave.go_underground(1.0);
            assert!(
                cave.end <= surface.end,
                "at {chunks} chunks the cave pushed the fade out to {} from {}",
                cave.end,
                surface.end
            );
            assert!(cave.start >= 0.0 && cave.start < cave.end);
        }
    }

    #[test]
    fn the_fog_switch_is_still_a_switch_under_ground() {
        // The colour is not only the fog's -- the frame is cleared to it
        // and the sky is drawn from it -- so a black one with the haze
        // switched off would be a black sky nobody asked for.
        let settings = ClientSettings::default();
        let off = Fog::for_frame(&settings, &sky_at(0.5), 12, false, None);
        let mut still_off = off;
        still_off.go_underground(1.0);
        assert_eq!(still_off, off, "F is a switch, and it was off");
    }

    #[test]
    fn a_flooded_cave_is_dark_water_at_the_range_of_water() {
        // Two claims about the same frame: the water decides how far
        // anything can be seen, and the rock decides that no daylight is
        // arriving to make it green.
        let settings = ClientSettings::default();
        let lake = Fog::for_frame(&settings, &sky_at(0.5), 12, true, Some(WaterTint::PLAIN));
        let mut flooded = lake;
        flooded.go_underground(1.0);
        assert_eq!(flooded.end, lake.end, "the water's range is the water's");
        assert_eq!(flooded.start, lake.start);
        assert!(flooded.color.length() < lake.color.length());
    }

    #[test]
    fn night_on_the_surface_and_a_cave_at_noon_are_not_the_same_picture() {
        // Midnight sky is already almost black, so if the *colour* were
        // the whole mechanic it would be invisible after dark. What says
        // "cave" at any hour is that the distance closes in: at night on
        // the surface the far hills are still drawn, and underground
        // they are not.
        let settings = ClientSettings::default();
        let midnight = Fog::for_frame(&settings, &sky_at(0.0), 12, true, None);
        let mut cave = Fog::for_frame(&settings, &sky_at(0.5), 12, true, None);
        cave.go_underground(1.0);
        assert!(
            cave.end < midnight.end * 0.5,
            "a cave has to be a nearer world than a dark one: {} vs {}",
            cave.end,
            midnight.end
        );

        let mut cave_at_night = midnight;
        cave_at_night.go_underground(1.0);
        assert!(cave_at_night.end < midnight.end);
    }

    #[test]
    fn there_is_no_sunset_in_a_lake_or_at_the_end_of_a_tunnel() {
        // The glow is daylight on a horizon, and neither place has one.
        let settings = ClientSettings {
            lighting: crate::engine::lighting::Quality::High,
            ..ClientSettings::default()
        };
        let sunset = sky_at(0.75);
        let open = Fog::for_frame(&settings, &sunset, 12, true, None);
        assert!(open.glow.w > 0.5 && open.haze.length() > 0.1, "the open air lost its sunset");

        let lake = Fog::for_frame(&settings, &sunset, 12, true, Some(WaterTint::PLAIN));
        assert_eq!((lake.glow, lake.haze), (glam::Vec4::ZERO, glam::Vec4::ZERO), "orange in a lake");

        let mut tunnel = open;
        tunnel.go_underground(1.0);
        assert_eq!(tunnel.glow.w, 0.0, "the far end of a tunnel glows");
        assert_eq!(tunnel.haze, glam::Vec4::ZERO);
    }

    #[test]
    fn under_water_the_range_collapses_and_the_colour_changes() {
        let settings = ClientSettings::default();
        let air = Fog::for_frame(&settings, &sky_at(0.5), 12, true, None);
        let water = Fog::for_frame(&settings, &sky_at(0.5), 12, true, Some(WaterTint::PLAIN));

        assert!(water.end < air.end, "water should close the world in");
        assert!(water.start < water.end);
        assert_ne!(water.color, air.color);
        assert_eq!(water.color, WaterTint::PLAIN.murk());
        // ...and it is *this* water and not one colour for every water.
        let marsh = WaterTint { chill: 0.35, silt: 0.0, depth: 1.0 };
        let bog = Fog::for_frame(&settings, &sky_at(0.5), 12, true, Some(marsh));
        assert_ne!(bog.color, water.color, "a marsh and a lake are the same murk");
    }

    /// **A lake at night is as dark as the night over it.** The colour
    /// under water was a constant, and the murk mixes up to 85% of every
    /// fragment toward it: at midnight the world above was black and the
    /// water a bright teal box. Held as the property -- under water is never
    /// brighter than the same hour's air -- and with noon left exactly where
    /// every other measurement of the water was taken.
    #[test]
    fn under_water_at_night_is_no_brighter_than_the_night_over_it() {
        let settings = ClientSettings::default();
        for quality in crate::engine::lighting::Quality::ALL {
            let settings = ClientSettings { lighting: quality, ..settings.clone() };
            let noon = Fog::for_frame(&settings, &sky_at(0.5), 12, true, Some(WaterTint::PLAIN));
            assert_eq!(noon.color, WaterTint::PLAIN.murk(), "noon moved at {quality:?}");
            for hour in [0.0, 0.1, 0.9, 0.95] {
                let water = Fog::for_frame(&settings, &sky_at(hour), 12, true, Some(WaterTint::PLAIN));
                let air = Fog::for_frame(&settings, &sky_at(hour), 12, true, None);
                assert!(
                    water.color.length() <= air.color.length(),
                    "at {hour} ({quality:?}) the water {:?} is brighter than the air {:?}",
                    water.color,
                    air.color
                );
                assert!(water.color.length() < noon.color.length() * 0.25, "at {hour} the water is lit like day");
            }
        }
    }

    #[test]
    fn the_colour_follows_the_sky_rather_than_being_a_grey() {
        // The whole trick: terrain has to fade into the horizon, so the
        // fog is the sky's own colour and changes with the hour.
        let settings = ClientSettings::default();
        let noon = Fog::for_frame(&settings, &sky_at(0.5), 12, true, None);
        let midnight = Fog::for_frame(&settings, &sky_at(0.0), 12, true, None);
        assert_ne!(noon.color, midnight.color);
        assert!(noon.color.length() > midnight.color.length(), "night is darker");
    }

    #[test]
    fn distance_culling_is_offered_only_where_it_is_free() {
        let settings = ClientSettings::default();
        let plain = Fog::for_frame(&settings, &sky_at(0.5), 12, true, None);
        assert_eq!(plain.cull_distance(), Some(plain.end));

        // Off: nothing is invisible at distance any more.
        assert_eq!(
            Fog::for_frame(&settings, &sky_at(0.5), 12, false, None).cull_distance(),
            None
        );
        // Under water the terrain and the sky finish on one colour now
        // (`UNDERWATER`), so the cull is free there too -- which is what
        // stopped a swimmer drawing the whole disc to paint it flat. The
        // GPU half of that claim is
        // `under_water_the_terrain_past_the_fog_is_the_colour_of_the_sky_behind_it`.
        let lake = Fog::for_frame(&settings, &sky_at(0.5), 12, true, Some(WaterTint::PLAIN));
        assert_eq!(lake.cull_distance(), Some(lake.end));
        // ...but not with F pressed: the murk still dims the far bed, and
        // without the fog it never lands on the sky's colour.
        assert_eq!(
            Fog::for_frame(&settings, &sky_at(0.5), 12, false, Some(WaterTint::PLAIN)).cull_distance(),
            None
        );
        // ...and not in a flooded cave, for the cave's reason below.
        let mut flooded = lake;
        flooded.go_underground(1.0);
        assert_eq!(flooded.cull_distance(), None);
        // ...and under ground they do not agree either, in the way that
        // shows: the culled wall would be replaced by the sky pass, sun
        // and clouds and all, sixty blocks below the grass.
        let mut cave = plain;
        cave.go_underground(1.0);
        assert_eq!(cave.cull_distance(), None);
        // Even a little way under, because a little way under is already
        // a range shorter than the world that is loaded.
        let mut ledge = plain;
        ledge.go_underground(0.05);
        assert_eq!(ledge.cull_distance(), None);
    }

    #[test]
    fn the_fade_never_outlives_the_terrain() {
        // At a render distance of eight the streamed disc stops 107 blocks
        // out at its nearest, and a fade that ran to 121 would have open
        // sky under its last eighth. The range is laid out against that
        // reach now, so it is finished there before any clamp is asked.
        let settings = ClientSettings::default();
        let mut fog = Fog::for_frame(&settings, &sky_at(0.5), 8, true, None);
        let reach = ChunkManager::reach_blocks(8);
        assert!(fog.end <= reach, "the fade ends at {} past the disc at {reach}", fog.end);
        // The clamp still matters where something nearer ends the world:
        // the menu's patch of scenery is eighty blocks across.
        let unclamped = fog.end;
        fog.clamp_to(60.0);
        assert!(fog.end < unclamped, "the fade was not pulled in");
        assert_eq!(fog.end, 60.0);
        assert!(fog.start < fog.end, "the fog starts after it ends");
        assert!(fog.start >= 0.0);

        // A reach further out than the fade already is changes nothing:
        // the player's own setting is the shorter of the two and stays.
        let mut wide = Fog::for_frame(&settings, &sky_at(0.5), 8, true, None);
        wide.clamp_to(10_000.0);
        assert_eq!(wide.end, unclamped);
        // ...and neither does nonsense.
        let mut junk = Fog::for_frame(&settings, &sky_at(0.5), 8, true, None);
        junk.clamp_to(f32::NAN);
        junk.clamp_to(-5.0);
        assert_eq!(junk.end, unclamped);
    }

    #[test]
    fn the_fog_leaves_most_of_the_distance_clear_and_is_done_where_the_world_ends() {
        // **"24 looks like 10 to 15, and the fog hides half of it."** The
        // fog began at 55% of `render distance x 16` -- 211 blocks of the
        // 362 the disc reaches at 24 -- so the tint started thirteen chunks
        // out. Stated as the two things a render distance has to mean, at
        // every distance the row offers: nothing is tinted before the
        // share of the reach the settings name, and the fade is complete
        // exactly where the streamed world ends -- not before it, which
        // throws away terrain that was loaded, and not after it, which is
        // the line round the edge of the world.
        let settings = ClientSettings::default();
        for chunks in 4..=crate::settings::MAX_RENDER_DISTANCE {
            let reach = ChunkManager::reach_blocks(chunks);
            let fog = Fog::for_frame(&settings, &sky_at(0.5), chunks, true, None);
            assert_eq!(fog.end, reach, "at {chunks} chunks the fog ends at {} and the world at {reach}", fog.end);
            assert!(
                fog.start >= reach * settings.fog_start_share - 1e-3,
                "at {chunks} chunks the fog begins at {} of {reach}",
                fog.start
            );
            assert!(fog.start < fog.end);
        }
        // At the report's own distance, in the report's terms: the fog does
        // not begin before the three-quarter mark.
        let at_24 = Fog::for_frame(&settings, &sky_at(0.5), 24, true, None);
        assert!(at_24.start / 16.0 > 16.0, "the fog begins {} chunks out at 24", at_24.start / 16.0);
    }

    #[test]
    fn an_old_settings_file_does_not_bring_the_old_fog_back() {
        // Every file ever saved says `fog_start_ratio = 0.55`. Those keys
        // mean a share of a distance the world never reaches, and read as
        // the new share they would put the old fog straight back -- so
        // they are a different name and are ignored.
        let old: ClientSettings = toml::from_str(
            "render_distance_chunks = 24\nfog_start_ratio = 0.550000011920929\nfog_end_ratio = 0.949999988079071\n",
        )
        .expect("a file from before the share must still parse");
        assert_eq!(old.fog_start_share, ClientSettings::default().fog_start_share);
    }

    #[test]
    fn the_range_widens_with_the_render_distance() {
        // Fog that ends at a fixed distance is fog that hides the world
        // on a machine that could draw it.
        let settings = ClientSettings::default();
        let near = Fog::for_frame(&settings, &sky_at(0.5), 4, true, None);
        let far = Fog::for_frame(&settings, &sky_at(0.5), 16, true, None);
        assert!(far.end > near.end);
        assert!(far.start > near.start);
    }

    #[test]
    fn smoke_closes_the_view_in_and_colours_it_even_with_the_haze_off() {
        let settings = ClientSettings::default();
        let sky = sky_at(12.0);
        let clear = Fog::for_frame(&settings, &sky, 12, true, None);
        let mut smoky = clear;
        smoky.fill_with_smoke(1.0);
        assert!(smoky.end <= SMOKE_END + 1e-3, "full smoke still sees {} blocks", smoky.end);
        assert!((smoky.color - SMOKE).length() < 1e-3);
        let mut off = Fog::for_frame(&settings, &sky, 12, false, None);
        off.fill_with_smoke(1.0);
        assert!(off.enabled && off.end <= SMOKE_END + 1e-3, "switching haze off hid the smoke");
        let mut none = clear;
        none.fill_with_smoke(0.0);
        assert_eq!(none, clear, "no smoke changed the fog");
        let mut under = Fog::for_frame(&settings, &sky, 12, true, Some(WaterTint::PLAIN));
        let water = under;
        under.fill_with_smoke(1.0);
        assert_eq!(under, water, "smoke got under the water");
    }

    #[test]
    fn the_dawn_mist_closes_the_view_without_closing_it_to_a_room() {
        let settings = ClientSettings::default();
        let sky = sky_at(12.0);
        let clear = Fog::for_frame(&settings, &sky, 12, true, None);
        let mut misty = clear;
        misty.lie_as_mist(1.0);
        assert!(misty.end < clear.end, "the mist did not close the view at all");
        assert!(
            misty.end > SMOKE_END * 4.0,
            "the mist closed in like a smoky hut: {} blocks",
            misty.end
        );
        // Half as thick is nearer than nothing and further than all of
        // it: a mist that stepped on would be a wall between two frames.
        let mut half = clear;
        half.lie_as_mist(0.5);
        assert!(half.end > misty.end && half.end < clear.end);
        // Nothing hangs on a mist, so a player who turned the haze off
        // keeps it off -- unlike the smoke, which is a warning.
        let mut off = Fog::for_frame(&settings, &sky, 12, false, None);
        let was = off;
        off.lie_as_mist(1.0);
        assert_eq!(off, was, "the mist turned the distance haze back on");
        let mut under = Fog::for_frame(&settings, &sky, 12, true, Some(WaterTint::PLAIN));
        let water = under;
        under.lie_as_mist(1.0);
        assert_eq!(under, water, "the mist got under the water");
    }
}
