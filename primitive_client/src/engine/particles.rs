//! Particles: a few hundred small things with a position, a velocity
//! and a lifetime.
//!
//! ## Why the game has one now
//!
//! Because everything it draws that is *small and moving* was faked
//! separately, and each fake was only ever right for the one thing it
//! was written for. The rain was eight sliding sheets, then four hundred
//! quads placed by a hash -- a pattern that moves rather than water that
//! falls, so it went through roofs, ignored the wind, and stopped dead
//! at the ground with nothing to show it had landed. A block coming
//! apart had nothing at all. Sparks over a fire had nothing at all.
//!
//! One pool answers all of it: **state, integrated**. A particle knows
//! where it is and where it is going; the world tells it when it has hit
//! something; the pool draws whatever is still alive as camera-facing
//! quads in the terrain vertex format, so it gets the same texture
//! array, the same fog and the same cutout pass as everything else.
//!
//! ## What it costs
//!
//! One `Vec` of plain data, an integration step per particle per frame,
//! and one block lookup for the ones that collide. Rain at full
//! intensity is about six hundred of them: a few hundred microseconds,
//! against a frame that spends milliseconds meshing chunks. There is a
//! hard cap (`MAX`) rather than a promise, because the one failure mode
//! a particle system has is being asked for a million of them.
//!
//! ## What it is not
//!
//! It is not on the GPU, and it does not need to be. A compute-shader
//! pool is the answer at a hundred thousand particles; at six hundred
//! the upload is smaller than one tree and the CPU step disappears into
//! the noise -- and the version that runs on the CPU is the version
//! whose collisions can ask the world a question.

use glam::Vec3;

use primitive_shared::lighting::LightMap;
use primitive_shared::types::BlockId;

use crate::engine::mesh::pack_light;
use crate::engine::texture::{FaceLayers, EXTRA_RAIN, EXTRA_SNOW};
use crate::logic::chunk_manager::ChunkManager;

/// One corner of a particle.
///
/// **Its own vertex, and the UV is why.** A terrain vertex packs its
/// texture coordinate into two bits -- corner to corner is all a block
/// face has ever needed -- and a chip of a broken block needs a
/// *sixteenth*: one texel of the block it came off, rather than the
/// whole picture of it shrunk to a speck. See `particles.wgsl`.
///
/// The same shape the hand and the dropped items use, so the shader
/// unpacks the light word the same way.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ParticleVertex {
    pub position: [f32; 3],
    pub uv: [f32; 2],
    /// Texture layer in the top half, the terrain's light word in the
    /// bottom.
    pub packed: u32,
    pub tint: [f32; 4],
}

impl ParticleVertex {
    pub const ATTRS: [wgpu::VertexAttribute; 4] = wgpu::vertex_attr_array![
        0 => Float32x3,
        1 => Float32x2,
        2 => Uint32,
        3 => Float32x4,
    ];

    pub fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRS,
        }
    }
}

/// The most particles alive at once.
///
/// A ceiling rather than a target: rain at full intensity asks for about
/// six hundred, and everything else in the game together asks for a few
/// dozen. What this stops is the pathological case -- a player breaking
/// a hundred blocks a second with a burst on each -- turning into a
/// frame-time cliff.
pub const MAX: usize = 1_200;

/// What a particle is a picture of.
///
/// A small enum rather than a texture layer, because the layer is not
/// the whole answer: a chip of stone is drawn from the block's own face,
/// and a raindrop is stretched along the way it is going while a spark
/// is square. The look decides both.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Look {
    /// A streak of rain, drawn long in the direction of travel.
    Rain,
    /// A flake, drawn square and small.
    Snow,
    /// What a drop leaves when it lands.
    Splash,
    /// A chip of a block, wearing **one texel** of that block's own
    /// texture.
    ///
    /// The texel is carried with it (`Particle::texel`) rather than
    /// picked at draw time, because a chip that resampled every frame
    /// would shimmer through the whole palette of the block as it fell.
    Chip(BlockId),
    /// A spark over a fire.
    Ember,
    /// A drop of blood off something that has just been hit.
    ///
    /// **Its own colour, not its own picture.** The atlas is 218 of 256
    /// block layers full and a new drawing costs one of them for ever;
    /// what a drop of blood needs from a texture is a round edge, and
    /// the snowflake already is one. So it wears the flake and a dark
    /// red tint, and the *shade* of red is carried per particle in
    /// `Particle::texel` -- see `BLOOD`. A single flat red reads as a
    /// swarm of identical dots, which is the same fault the chips of a
    /// broken block were written to avoid.
    Blood,
    /// What a drop of blood leaves where it lands: a mark on the
    /// ground that stays for the best part of a minute.
    ///
    /// **Not a block, and it never touches the world.** Blood is the
    /// client's own drawing of something the server told it happened,
    /// and writing a stain into the world would be the client inventing
    /// terrain -- it would have to be sent, saved, lit, meshed and
    /// broken, and it would still be gone the moment somebody rejoined.
    /// A particle is the whole of what a stain needs: a position, a
    /// lifetime and a quad.
    ///
    /// **The flake again, and no new layer.** The same trade blood and
    /// smoke already make (the atlas has a ceiling of 256 pictures):
    /// what a stain needs from a texture is a round blob with a soft
    /// edge, and the snowflake is one. What makes it a stain rather
    /// than a drop is the pose -- it lies flat on the ground instead of
    /// turning to face the camera -- and the palette, which is `BLOOD`
    /// darkened.
    Stain,
    /// A puff of smoke off a fire that is alight.
    ///
    /// **The snowflake again, under a different palette** -- the same
    /// trade blood makes and for the same reason: the atlas has a
    /// ceiling of 256 layers and what smoke needs from a picture is a
    /// round blob with a soft edge, which the flake already is. A dark
    /// grey over a nearly-white flake comes out the colour of woodsmoke,
    /// and the alpha is where the whole effect lives.
    ///
    /// Unlike everything else here it fades over the *whole* of its
    /// life rather than in the last fifth (see `Look::tint`), because a
    /// plume that held its opacity and then blinked would read as a
    /// string of grey beads rather than as smoke thinning out.
    Smoke,
    /// What a drop of blood becomes in water: a thin red cloud that spreads
    /// and is gone in about a second.
    ///
    /// **Reported as "кровь должно растворятся под водой".** A drop is a
    /// thing that falls through air, and in water it was the same drop
    /// falling at the same speed through the lake and laying its mark on the
    /// bed -- specks sinking like shot and a stain on the sand under two metres
    /// of water. Blood in water neither falls nor stays: it clouds. So a drop
    /// that is in water, born there or fallen in, stops being a drop
    /// (`Particles::update`), and so does a mark the water has come over.
    ///
    /// The flake again under the blood's own reds, and for the smoke's reason
    /// it swells and thins over the whole of its life rather than holding its
    /// colour and blinking out.
    Cloud,
    /// Broken white water over a rapid: a fleck thrown up off the surface
    /// and carried downstream on the current (`Particles::rapids`).
    ///
    /// **The flake once more, white and thin**, for the smoke's reason: what
    /// foam needs from a picture is a soft round blob, and the atlas has no
    /// layer to spare for one. What makes it foam is where it is born -- on
    /// the surface of water running faster than a swimmer -- and that it
    /// travels with the river, so a rapid reads from the bank as a white
    /// streak moving one way, which is the thing a player has to see before
    /// they decide not to swim there.
    Foam,
}

impl Look {
    /// Which layer of the texture array it wears.
    fn layer(self, layers: &FaceLayers) -> u32 {
        match self {
            Look::Rain | Look::Splash => layers.extra(EXTRA_RAIN),
            // The flake is a round blob with a soft edge, which is what
            // a drop of anything wants and what the rain's picture --
            // a straight streak seven texels tall -- is not.
            Look::Snow | Look::Blood | Look::Smoke | Look::Stain | Look::Cloud | Look::Foam => {
                layers.extra(EXTRA_SNOW)
            }
            // **A spark is a dot of light, not a flame in miniature.** It
            // wore the flame's own picture, squeezed to a few hundredths of
            // a block -- and at that size the picture is still a picture:
            // a player saw the fire's texture flying up off the fire in
            // little squares. The soft round blob, tinted by `colour`
            // from yellow to a dying red, is what a spark is.
            Look::Ember => layers.extra(EXTRA_SNOW),
            // The top face, which is the one a player has been looking
            // at while they mined it.
            // ...except off a carcass, which has no picture of its own in
            // the block table: the chips are the animal's hide while it
            // wears one, and flesh once it has been skinned. The table's
            // picture for the row is a stand-in that costs no layer, and
            // it flew off a struck boar as pale flakes.
            Look::Chip(block) => match primitive_shared::animals::Species::of_carcass(block) {
                Some(species) if primitive_shared::animals::butchering_stage(block) == 0 => {
                    layers.animal(species, 0)
                }
                Some(_) => layers.layer_for_face(
                    primitive_shared::types::BLOCK_RAW_MEAT,
                    crate::engine::texture::FACE_TOP,
                ),
                None => layers.layer_for_face(block, crate::engine::texture::FACE_TOP),
            },
        }
    }

    /// The light word it is drawn with, or `None` for one taken from the
    /// cells it is in (`Particles::build_into`).
    ///
    /// **It was the open sky for everything but an ember**, on the reasoning
    /// that rain in a cave is rain that is not there. True of the rain, and
    /// wrong of everything that is not weather: a boar struck in a cave, a
    /// block broken down a mine and a blow inside a hut at noon were all drawn
    /// as bright as a field, red and grey specks glowing in the dark
    /// (`particle_repro`, the hut). So the weather keeps the sky -- it only
    /// falls where the sky is, and sampled it would go dark in the eaves it
    /// falls past -- an ember keeps its own light, and everything else is lit
    /// where it is, the way an animal or a dropped stack is.
    fn light(self) -> Option<u32> {
        match self {
            Look::Ember => Some(pack_light(15, 15, 3, 0)),
            Look::Rain | Look::Snow | Look::Splash => Some(pack_light(15, 0, 3, 0)),
            _ => None,
        }
    }

    /// What its own colour does to the texture it wears, and how it
    /// fades. `fade` is one for most of a life and runs to zero at the
    /// end of it.
    ///
    /// `left` is the *whole* remaining fraction of the life, one at
    /// birth and zero at death, and only smoke reads it. Two ramps
    /// rather than one because they answer opposite complaints: a chip
    /// of stone that thinned out from the moment it was struck would
    /// never be seen at all, and a puff of smoke that held its opacity
    /// until the last fifth is a grey bead that vanishes.
    ///
    /// `shade` is `Particle::texel.0`, which only `Look::Blood` reads:
    /// see `BLOOD` for why a burst is four reds rather than one.
    fn tint(self, fade: f32, left: f32, shade: u8) -> [f32; 4] {
        match self {
            // **Dark, and never more than a third opaque.** Smoke has
            // to be legible against a bright sky and it must not become
            // a grey wall over the fire a player is crouched at
            // feeding: the plume works by many thin puffs overlapping,
            // and a single one you can see through is what makes the
            // stack of them read as volume rather than as a sticker.
            // Squared, so the puff spends most of its life on its way
            // out instead of being solid and then gone.
            Look::Smoke => {
                let [r, g, b] = SMOKE_GREYS[shade as usize % SMOKE_GREYS.len()];
                [r, g, b, SMOKE_ALPHA * left * left]
            }
            // **Opaque until it is gone**, rather than fading from the
            // first frame the way the water does. Blood that is
            // translucent for the whole of its half second reads as a
            // pink haze; what it should read as is specks, and then
            // nothing.
            Look::Blood => {
                let [r, g, b] = BLOOD[shade as usize % BLOOD.len()];
                [r, g, b, fade.min(1.0)]
            }
            // **Darker than the drop that made it, and it holds its
            // colour until it goes.** Blood on the ground is blood that
            // has soaked in: a stain drawn in the spray's own red reads
            // as wet paint, and one that thinned from the moment it
            // landed would never be seen at all -- these live the best
            // part of a minute and the player is usually looking
            // somewhere else for the first ten seconds of it. `fade` is
            // one for four fifths of a life and runs to zero over the
            // last fifth, which is a mark drying out rather than
            // blinking off.
            Look::Stain => {
                let [r, g, b] = BLOOD[shade as usize % BLOOD.len()];
                [
                    r * STAIN_DARKEN,
                    g * STAIN_DARKEN,
                    b * STAIN_DARKEN,
                    STAIN_ALPHA * fade.min(1.0),
                ]
            }
            // **Thin, and thinner as it spreads**: the blood's own reds,
            // lifted a little because most of a cloud has water between it
            // and the eye, never more than half opaque and squared like
            // smoke, so it spends its second on the way out.
            Look::Cloud => {
                let [r, g, b] = BLOOD[shade as usize % BLOOD.len()];
                [r * 1.3, g * 1.3 + 0.02, b * 1.3 + 0.02, CLOUD_ALPHA * left * left]
            }
            // **White, and thinning over its whole life** like a cloud:
            // a fleck of spray that held its opacity and blinked out
            // would read as confetti.
            Look::Foam => [0.93, 0.96, 1.0, FOAM_ALPHA * left],
            // A spark is hotter than the picture of a flame it is cut
            // from, and it dims to red as it dies rather than shrinking
            // to nothing while still yellow.
            Look::Ember => [1.0, 0.55 + 0.45 * fade, 0.25 * fade, fade],
            // Water takes a little of the sky rather than being grey.
            Look::Rain | Look::Splash => [0.72, 0.82, 1.0, 0.55 + 0.45 * fade],
            _ => [1.0, 1.0, 1.0, fade.min(1.0)],
        }
    }
}

/// One particle.
///
/// Plain data on purpose: the pool is a `Vec` of these and the step is a
/// loop over it, which is the layout that makes a few hundred of them
/// cost nothing.
#[derive(Debug, Clone, Copy)]
pub struct Particle {
    pub position: glam::DVec3,
    pub velocity: Vec3,
    /// Seconds left. A particle is dead at zero.
    pub life: f32,
    /// How long it started with, so the drawing can fade or shrink it.
    pub total: f32,
    /// Half-width, in blocks.
    pub size: f32,
    pub look: Look,
    /// Blocks per second per second, downward. Zero for smoke, full for
    /// a chip of stone.
    pub gravity: f32,
    /// How much speed is left after a second of flight, as a fraction.
    /// One for a stone chip, well under one for anything drifting.
    pub drag: f32,
    /// Which texel of its texture it wears, as a column and a row.
    ///
    /// Only a `Chip` reads it; everything else wears its whole picture,
    /// which for a raindrop or a spark *is* one small thing rather than
    /// a tiled surface.
    pub texel: (u8, u8),
    /// Does the world stop it?
    ///
    /// Off for most of them and on for the ones where it is the whole
    /// point: rain that fell through a roof was the thing that made the
    /// old sheet-of-rain read as a texture rather than as weather.
    pub collides: bool,
}

/// A lit fire the search has found, and when it next does something.
///
/// **Countdowns rather than a coin flipped every frame**, and that is
/// not a refinement -- it is the difference between the rate in the
/// constant and the rate on the screen. A per-frame trial against
/// `rate * dt` asks a three-shift generator for a rare event at a fixed
/// stride, and this one obliged by never producing one: a campfire
/// measured over four seconds at sixty frames threw *no* sparks where
/// twelve were due, and smoked at half the rate it was asked for. A
/// countdown emits exactly what it is given, at any frame rate, and
/// needs no generator at all.
struct Hearth {
    cell: (i32, i32, i32),
    /// Seconds until the next spark, and until the next puff of smoke.
    spark_in: f32,
    smoke_in: f32,
}

/// The two axes a camera-facing particle is laid out along, for this
/// camera: its right, and its up.
///
/// **The camera's own up, tilted with its pitch -- not the world's.** It was
/// `(camera.right_horizontal(), Vec3::Y)`: a quad standing upright in the
/// world and turning round the vertical to follow the player. Level with the
/// eye that faces the camera. Looked down on it does not: its height on the
/// screen goes with the cosine of the pitch, so at the pitch a player breaks
/// the block at their feet (about seventy degrees down) a chip was a third
/// of its height, and straight down a chip, a drop off the player's own chest
/// and a raindrop were hairlines or nothing -- the particles "drawn wrong" in
/// exactly the place a player looks at them hardest. `particle_repro`'s
/// `looking_at_the_feet` is the picture. Built from the aim rather than the
/// shaken view, because the shake is a fraction of a degree and a particle
/// that trembled with it would be a particle nobody could look at.
///
/// **One function, called by the frame and by every test and tool that
/// asks what the frame draws**, so that "what `lib.rs` passes" is a
/// thing a test can hold rather than a pair of arguments copied into
/// it and left behind when the call site changes.
pub fn billboard_axes(camera: &crate::engine::camera::Camera) -> (Vec3, Vec3) {
    let right = camera.right_horizontal();
    let up = right.cross(camera.forward()).normalize_or(Vec3::Y);
    (right, up)
}

/// The pool.
#[derive(Default)]
pub struct Particles {
    live: Vec<Particle>,
    /// The state of the cheap generator below. Not `rng::Rng` -- that
    /// lives in the server crate, and a scatter that is only ever looked
    /// at does not need a good one.
    seed: u32,
    /// Lit hearths near the player. See `find_hearths` for why a fire
    /// has to be remembered rather than rediscovered.
    hearths: Vec<Hearth>,
    /// How many columns of river surface `rapids` owes a look at: the
    /// fraction of a sample the last frame's rate left over, so the rate in
    /// `FOAM_SAMPLES_PER_SECOND` is the rate at any frame rate.
    foam_owed: f32,
}

impl Particles {
    pub fn new() -> Self {
        Self {
            live: Vec::with_capacity(256),
            seed: 0x1234_5678,
            hearths: Vec::new(),
            foam_owed: 0.0,
        }
    }

    /// How many are alive. Shown in the F3 panel, which is where a
    /// number like this earns its keep: "the frame rate fell and there
    /// are nine hundred particles" is a sentence somebody can act on.
    pub fn len(&self) -> usize {
        self.live.len()
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn is_empty(&self) -> bool {
        self.live.is_empty()
    }

    /// Drops one in, if there is room.
    ///
    /// Silently refused at the cap rather than making room: the
    /// alternative is evicting somebody else's particle, and a burst
    /// that deletes the rain is a burst that makes the weather flicker.
    pub fn emit(&mut self, particle: Particle) {
        if self.live.len() >= MAX {
            return;
        }
        self.live.push(particle);
    }

    /// A number in 0..1. Cheap, and repeatable only in the sense that it
    /// does not matter.
    fn random(&mut self) -> f32 {
        // xorshift32, which is three shifts and enough for a scatter.
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        (self.seed >> 8) as f32 / (1 << 24) as f32
    }

    fn between(&mut self, low: f32, high: f32) -> f32 {
        low + self.random() * (high - low)
    }

    /// One step: move everything, collide what collides, bury the dead.
    pub fn update(&mut self, chunks: &ChunkManager, dt: f32) {
        let dt = dt.clamp(0.0, 0.1);
        // Splashes are collected and added after the loop: a particle
        // cannot push into the list it is being iterated out of.
        let mut splashes: Vec<Particle> = Vec::new();
        // ...and so are the marks blood leaves, for the same reason and
        // one more: a stain is given a size, a shade and a turn out of
        // the generator, which lives in the pool being iterated.
        let mut stains: Vec<Vec3> = Vec::new();

        for particle in &mut self.live {
            particle.life -= dt;
            if particle.life <= 0.0 {
                continue;
            }
            // **Blood in water clouds** -- see `Look::Cloud`. Asked of the
            // cell the drop is in before it moves, so one born under the
            // surface is a cloud from its first frame and one that falls in
            // stops where it crossed; a mark the water has come over goes the
            // same way. One lookup a drop and a mark: a drop already asks the
            // world where it is going, and there are `STAIN_MAX` marks at most.
            if matches!(particle.look, Look::Blood | Look::Stain) && liquid_at(chunks, particle.position.as_vec3()) {
                dissolve(particle);
            }
            particle.velocity.y -= particle.gravity * dt;
            if particle.drag < 1.0 {
                particle.velocity *= particle.drag.powf(dt);
            }
            let step = particle.velocity * dt;
            let next = particle.position + step.as_dvec3();

            // **Water stops a raindrop and nothing else.** The
            // collision test asks `is_collidable`, and water is not --
            // a player swims through it -- so every drop of a shower
            // fell *through* the lake it was falling into and burst on
            // the bed, two metres down, where the splash is a pale
            // fleck under the surface. A lake in the rain is the one
            // place a shower is most obviously a shower, and it was the
            // one place nothing happened.
            //
            // Only the rain, and only the rain: a chip of stone
            // thrown into a pond should sink, which is what the
            // collidable test already says.
            let landed = particle.collides
                && match particle.look {
                    Look::Rain => surface_at(chunks, next.as_vec3()),
                    _ => solid_at(chunks, next.as_vec3()),
                };
            if landed {
                match particle.look {
                    // A drop that lands *is* a splash: it stops being a
                    // drop and leaves two small ones where it hit. That
                    // is what makes rain read as falling on something
                    // rather than as passing in front of it.
                    Look::Rain => {
                        particle.life = 0.0;
                        for _ in 0..2 {
                            splashes.push(Particle {
                                position: particle.position,
                                velocity: Vec3::ZERO,
                                life: 0.22,
                                total: 0.22,
                                size: 0.05,
                                look: Look::Splash,
                                texel: (0, 0),
                                gravity: 6.0,
                                drag: 1.0,
                                collides: false,
                            });
                        }
                    }
                    // **A drop of blood that lands leaves a mark.**
                    // The same shape as the rain above -- the drop
                    // stops being a drop -- and it is where the whole
                    // effect comes from: nothing here has to know who
                    // was hurt or where the ground is, because the
                    // drops were already falling and the ground is
                    // whatever they hit. A stain emitted at the animal
                    // instead would need a downward search per blow and
                    // would still land inside the hillside the animal
                    // was standing against.
                    //
                    // Only if it came down on the *top* of something:
                    // see `landing_face`. A drop that hit a wall slides
                    // to a stop the way a chip of stone does.
                    Look::Blood => match landing_face(chunks, particle.position.as_vec3(), next.as_vec3()) {
                        Some(on) => {
                            particle.life = 0.0;
                            stains.push(on);
                        }
                        None => {
                            particle.velocity *= 0.25;
                            particle.velocity.y = 0.0;
                        }
                    },
                    // Everything else settles: it loses most of its
                    // speed against the surface and slides to a stop,
                    // which is what a chip of stone does.
                    _ => {
                        particle.velocity *= 0.25;
                        particle.velocity.y = 0.0;
                        continue;
                    }
                }
                continue;
            }
            particle.position = next;
        }

        self.live.retain(|particle| particle.life > 0.0);
        for at in stains {
            self.stain(at);
        }
        for splash in splashes {
            // Sideways, so two splashes from one drop are not one
            // splash drawn twice.
            let (x, z) = (self.between(-1.0, 1.0), self.between(-1.0, 1.0));
            let mut splash = splash;
            splash.velocity = Vec3::new(x * 1.2, self.between(1.0, 2.2), z * 1.2);
            self.emit(splash);
        }
    }

    /// Keeps the weather stocked around the player.
    ///
    /// **Emission is a rate, not a pattern.** The old rain was four
    /// hundred quads whose positions were a hash of their index -- the
    /// same drops every frame, moved. These are spawned above the player
    /// at a rate, fall on their own, and die where they land, so a roof
    /// keeps them off, wind blows them sideways, and what is under a
    /// tree is dry.
    pub fn weather(
        &mut self,
        intensity: f32,
        snowing: bool,
        at: Vec3,
        wind: Vec3,
        dt: f32,
    ) {
        if intensity <= 0.0 {
            return;
        }
        let wanted = (RAIN_PER_SECOND * intensity * dt) as usize + 1;
        for _ in 0..wanted.min(64) {
            if self.live.len() >= MAX {
                return;
            }
            let (rx, rz) = (self.between(-1.0, 1.0), self.between(-1.0, 1.0));
            let (look, velocity, size, life) = if snowing {
                (
                    Look::Snow,
                    Vec3::new(wind.x * 0.6, -SNOW_SPEED, wind.z * 0.6),
                    0.055,
                    CEILING / SNOW_SPEED,
                )
            } else {
                (
                    Look::Rain,
                    Vec3::new(wind.x, -RAIN_SPEED, wind.z),
                    0.035,
                    CEILING / RAIN_SPEED * 1.4,
                )
            };
            // Spawned in a ring above the player, high enough to be seen
            // falling and low enough that most of them land in sight -- and
            // **the ring is put upwind by half of what the wind will carry
            // the drop**, so that what the player is standing in is the
            // middle of the fall rather than its downwind edge.
            //
            // It used to be a ring straight overhead, which was right while
            // the wind was a two-and-a-half-block drift. Under the world's
            // real wind a drop crosses several blocks before it lands, and a
            // ring overhead meant the upwind half of the view was clear sky
            // in a downpour: the rain looked like it was starting a few
            // paces away, which is the one thing weather must not look like.
            let position = Vec3::new(
                at.x + rx * SCATTER - velocity.x * life * 0.5,
                at.y + CEILING,
                at.z + rz * SCATTER - velocity.z * life * 0.5,
            );
            self.emit(Particle {
                position: position.as_dvec3(),
                velocity,
                life,
                total: life,
                size,
                look,
                texel: (0, 0),
                gravity: if snowing { 0.0 } else { 6.0 },
                drag: if snowing { 0.85 } else { 1.0 },
                collides: true,
            });
        }
    }

    /// A block coming apart: a handful of chips of it, thrown outward.
    pub fn block_broken(&mut self, at: (i32, i32, i32), block: BlockId) {
        let centre = Vec3::new(at.0 as f32 + 0.5, at.1 as f32 + 0.5, at.2 as f32 + 0.5);
        for _ in 0..CHIPS {
            // Every number first, then the particle: the emitter borrows
            // the pool and the generator is part of it.
            let spread = Vec3::new(
                self.between(-0.5, 0.5),
                self.between(-0.5, 0.5),
                self.between(-0.5, 0.5),
            );
            let velocity = Vec3::new(
                self.between(-2.2, 2.2),
                self.between(1.5, 4.5),
                self.between(-2.2, 2.2),
            );
            let life = self.between(0.5, 1.1);
            let size = self.between(0.045, 0.085);
            // **One texel of the block, picked per chip.** Whole-texture
            // chips read as a wall shrunk to a speck -- mortar lines and
            // all -- where what a broken block throws is *grit the
            // colour of the block*.
            let texel = (
                (self.random() * TEXTURE_GRID as f32) as u8,
                (self.random() * TEXTURE_GRID as f32) as u8,
            );
            self.emit(Particle {
                position: (centre + spread * 0.8).as_dvec3(),
                velocity,
                life,
                total: life,
                size,
                look: Look::Chip(block),
                texel,
                gravity: 16.0,
                drag: 1.0,
                collides: true,
            });
        }
    }

    /// **A fishing float on the water**: one chip of the rod's own picture,
    /// the texel its float is painted in ([`FLOAT_TEXEL`]), held where it is
    /// for a little over a frame and asked for again the next.
    ///
    /// A particle rather than a model, because a float is a speck at the
    /// distance a line is cast and everything a speck needs -- a position, a
    /// colour and the light where it is -- a chip already has. A model would
    /// be a new entry in the entity pass for a thing the server never sends.
    /// `dt` sets how long it lives, so a slow frame does not leave a gap in
    /// which the float blinks out.
    pub fn float(&mut self, at: Vec3, dt: f32) {
        let life = (dt * 2.0).clamp(0.03, 0.25);
        self.emit(Particle {
            position: at.as_dvec3(),
            velocity: Vec3::ZERO,
            life,
            total: life,
            size: 0.11,
            look: Look::Chip(primitive_shared::types::BLOCK_FISHING_ROD),
            texel: FLOAT_TEXEL,
            gravity: 0.0,
            drag: 1.0,
            collides: false,
        });
    }

    /// **The ring a fish makes taking the bait**: a few beads of water
    /// pushed out sideways off the float, thrown flat and low.
    ///
    /// Deliberately the catch's splash with the height taken out of it and
    /// a quarter of the beads: a dip has to be *seen* at forty paces and
    /// must not be mistaken for the fish coming out, which is the one loud
    /// thing fishing does.
    pub fn float_ring(&mut self, at: Vec3) {
        for _ in 0..5 {
            let angle = self.between(0.0, std::f32::consts::TAU);
            let speed = self.between(0.5, 1.1);
            let velocity = Vec3::new(angle.cos() * speed, self.between(0.15, 0.5), angle.sin() * speed);
            let life = self.between(0.25, 0.45);
            let size = self.between(0.04, 0.07);
            self.emit(Particle {
                position: at.as_dvec3(),
                velocity,
                life,
                total: life,
                size,
                look: Look::Foam,
                texel: (0, 0),
                gravity: 9.0,
                drag: 1.0,
                collides: false,
            });
        }
    }

    /// The water breaking where a fish came up: a ring of foam thrown up off
    /// the float. The river's own spray (`Look::Foam`), so a catch is white
    /// water the way a rapid is.
    pub fn catch_splash(&mut self, at: Vec3) {
        for _ in 0..14 {
            let angle = self.between(0.0, std::f32::consts::TAU);
            let speed = self.between(0.6, 1.6);
            let velocity = Vec3::new(angle.cos() * speed, self.between(1.2, 2.6), angle.sin() * speed);
            let life = self.between(0.35, 0.7);
            let size = self.between(0.05, 0.1);
            self.emit(Particle {
                position: at.as_dvec3(),
                velocity,
                life,
                total: life,
                size,
                look: Look::Foam,
                texel: (0, 0),
                gravity: 9.0,
                drag: 1.0,
                collides: false,
            });
        }
    }

    /// A blow landing: a short spray of blood at the point of it.
    ///
    /// **Thrown in every direction rather than away from the attacker**,
    /// and that is not laziness -- it is what the client actually
    /// knows. An animal bleeds because the server said its hurt flash
    /// went up, and that message says who was hurt and not by whom or
    /// from where; the player bleeds off a `Health` message, which says
    /// even less. A spray with a direction in it would be a spray
    /// pointing somewhere invented, and a wrong direction reads worse
    /// than none: it says the blow came from over there, and it did
    /// not.
    ///
    /// Short and cheap on purpose. `BLOOD_DROPS` of them, gone in
    /// something under a second, because this is a thing that happens
    /// once a second while a wolf is on you and it must not be able to
    /// crowd the rain out of `MAX`.
    pub fn blood(&mut self, at: Vec3) {
        self.spray(at, BLOOD_DROPS);
    }

    /// `drops` drops of blood at `at`: the burst of a blow, or the one or
    /// two a cut drips. What `ServerMessage::Blood` becomes.
    ///
    /// **A drip is not a small burst.** A drop off a cut runs down and falls;
    /// thrown up and outward like a blow's it reads as a tiny spray every
    /// second, which is the fountain again at a lower rate. So anything short
    /// of half a blow's worth leaves at a fifth of the speed.
    ///
    /// Capped at a blow's worth: this comes off a socket, and a count there is
    /// a claim.
    pub fn spray(&mut self, at: Vec3, drops: usize) {
        let thrown = if drops * 2 >= BLOOD_DROPS { 1.0 } else { DRIP_THROW };
        for _ in 0..drops.min(BLOOD_DROPS) {
            let velocity = Vec3::new(
                self.between(-1.8, 1.8),
                self.between(0.6, 2.6),
                self.between(-1.8, 1.8),
            ) * thrown;
            // **Long enough to reach the ground, and no longer.**
            // It was 0.28..0.6, which was chosen when a drop's whole
            // job was to be a spray at the edge of vision -- and under
            // it two thirds of a burst died in mid-air, a metre up.
            // That is invisible while blood is only a spray and it is
            // the entire effect now that a drop leaves a mark where it
            // lands: half of the marks were simply never made. From a
            // chest a metre and a bit up, thrown upward first, the fall
            // takes about two thirds of a second at the gravity below.
            let life = self.between(0.5, 0.95);
            let size = self.between(0.028, 0.055);
            // The palette index rides in the texel, which is a field
            // only a chip reads. A second field on `Particle` for it
            // would be four bytes on every raindrop in a storm.
            let shade = (self.random() * BLOOD.len() as f32) as u8;
            self.emit(Particle {
                position: at.as_dvec3(),
                velocity,
                life,
                total: life,
                size,
                look: Look::Blood,
                texel: (shade, 0),
                // Heavier than a spark and lighter than a stone chip:
                // it should arc and land inside its own half second,
                // not hang in the air like smoke.
                gravity: 14.0,
                drag: 1.0,
                // It stops where it lands. A drop that fell through the
                // floor of the cave you were bitten in is the same
                // fault the rain was rebuilt to fix.
                collides: true,
            });
        }
    }

    /// One mark on the ground, where a drop of blood landed.
    ///
    /// **The budget is here and not in `MAX`.** Everything else in this
    /// pool lives for a moment; a stain lives for the best part of a
    /// minute, so it is the one look that accumulates, and a fight
    /// against a pack of wolves would otherwise spend the whole pool on
    /// the floor and stop the rain. At the ceiling the *faintest* stain
    /// is the one recycled -- the one nearest the end of its own fade,
    /// which is the only one that can disappear without anybody seeing
    /// it go. Refusing the new one instead would mean a blow that leaves
    /// no mark, which is the fault this whole thing exists to fix.
    fn stain(&mut self, at: Vec3) {
        let life = self.between(STAIN_LIFE.0, STAIN_LIFE.1);
        let size = self.between(STAIN_SIZE.0, STAIN_SIZE.1);
        let shade = (self.random() * BLOOD.len() as f32) as u8;
        // Which way round the blob is stamped. A flake is round and
        // eight of them stamped at the same angle still read as a
        // pattern -- the same reason a chip takes a random texel of its
        // block. It rides in the field only a chip reads, beside the
        // shade, so a stain costs no more bytes than a raindrop.
        let turn = (self.random() * 256.0) as u8;
        // **The whole mark has to fit on the face it landed on.**
        // Photographed on the plaza of the test world: a drop that
        // landed a few centimetres from the edge of the block the
        // player was standing on left a disc with a third of itself
        // hanging past the edge, over the drop to the grass below --
        // a decal in mid-air, which is exactly what `landing_face` is
        // careful not to make and what a quad wider than its own
        // clearance makes anyway. The pass has no way to clip one
        // surface to another, so the mark is nudged in instead: at most
        // a sixth of a block, only ever near an edge, and nobody can
        // tell a spatter from a spatter shifted by two texels.
        //
        // The reach of a *turned* square is its half-width by root two,
        // and the floor stops the clamp inverting if the sizes above
        // are ever opened up.
        let reach = (size * std::f32::consts::SQRT_2).min(0.49);
        let inside = |v: f32| {
            let cell = v.floor();
            v.clamp(cell + reach, cell + 1.0 - reach)
        };
        let at = Vec3::new(inside(at.x), at.y, inside(at.z));
        let mark = Particle {
            position: at.as_dvec3(),
            velocity: Vec3::ZERO,
            life,
            total: life,
            size,
            look: Look::Stain,
            texel: (shade, turn),
            // It has landed. Nothing moves it, nothing stops it again.
            gravity: 0.0,
            drag: 1.0,
            collides: false,
        };
        let mut standing = 0;
        let mut faintest = (usize::MAX, f32::MAX);
        for (index, particle) in self.live.iter().enumerate() {
            if particle.look != Look::Stain {
                continue;
            }
            standing += 1;
            if particle.life < faintest.1 {
                faintest = (index, particle.life);
            }
        }
        if standing >= STAIN_MAX {
            if faintest.0 != usize::MAX {
                self.live[faintest.0] = mark;
            }
            return;
        }
        self.emit(mark);
    }

    /// A spark off a fire. One at a time, called on a timer by whoever
    /// owns the fire.
    pub fn ember(&mut self, at: Vec3) {
        let life = self.between(0.6, 1.4);
        let offset = Vec3::new(self.between(-0.2, 0.2), 0.1, self.between(-0.2, 0.2));
        let velocity = Vec3::new(
            self.between(-0.3, 0.3),
            self.between(0.8, 1.8),
            self.between(-0.3, 0.3),
        );
        let size = self.between(0.03, 0.06);
        self.emit(Particle {
            position: (at + offset).as_dvec3(),
            velocity,
            life,
            total: life,
            size,
            look: Look::Ember,
            texel: (0, 0),
            // Rising, so gravity is *negative*: hot air is the whole
            // reason a spark goes up rather than down.
            gravity: -1.2,
            drag: 0.6,
            collides: false,
        });
    }

    /// A puff of smoke off a fire. One at a time, on a timer, like the
    /// sparks.
    ///
    /// **It starts above the flame, not in it.** A puff born inside the
    /// cell of the fire is a grey blob over the one thing in the scene
    /// a player is looking at when they are close enough to see the
    /// puff at all -- and the fire is a screen you open by looking at
    /// it. `mesh::FLAME_TOP` puts the tip of the flame at 0.98 of the
    /// cell, so birth just above that is smoke coming off the fire with
    /// nothing of the fire behind it.
    ///
    /// ...with `room` blocks of open air over the floor of its cell, and
    /// whether one was made.
    ///
    /// **The report: "if a block stands above a campfire it turns black".**
    /// A puff does not collide (see `collides` below), and it is lit by the
    /// light *in the cell it is in* (`entities::sampled_light`). Born at
    /// `SMOKE_BIRTH_HEIGHT` under a block, it was born inside that block --
    /// where the light is nought -- and rose through it for a second: a
    /// cloud of quads the size of a hand, a third opaque each and every one
    /// of them black, poking out of every face of the block over the fire.
    /// From standing height that is a block gone black, and it was.
    ///
    /// So the room is measured once a puff, which is one column read and
    /// not a lookup per particle per frame, and the puff is given no more
    /// life than it takes to reach the ceiling at its fastest. The rejected
    /// fix was collision for smoke, which is the budget the note on
    /// `collides` already refused; the other rejected fix was lighting smoke
    /// by the fire's own cell, which would have made the cloud bright
    /// instead of black and left it standing inside the block.
    fn smoke_under(&mut self, at: Vec3, room: f32) -> bool {
        // Where the tallest puff's top would be at the moment it is born.
        let swollen = SMOKE_SIZE.1 * (1.0 + SMOKE_GROWTH);
        let clear = room - SMOKE_BIRTH_HEIGHT - swollen;
        if clear <= 0.0 {
            return false;
        }
        let life = self
            .between(SMOKE_LIFE.0, SMOKE_LIFE.1)
            .min(seconds_to_rise(clear));
        let offset = Vec3::new(
            self.between(-0.16, 0.16),
            SMOKE_BIRTH_HEIGHT,
            self.between(-0.16, 0.16),
        );
        // Sideways speed is what makes a plume spread as it goes rather
        // than rise as a column: every puff leaves on its own slightly
        // different heading, and `drag` bleeds that off, so the cone
        // opens quickly near the fire and slowly above it.
        let velocity = Vec3::new(
            self.between(-0.45, 0.45),
            self.between(0.7, 1.2),
            self.between(-0.45, 0.45),
        );
        let size = self.between(SMOKE_SIZE.0, SMOKE_SIZE.1);
        // Which of `SMOKE_GREYS` this puff wears, carried in the field
        // only a chip reads -- the same seat blood's shade rides in,
        // and for the same reason: a fifth field on `Particle` would be
        // four bytes on every raindrop in a storm.
        let shade = (self.random() * SMOKE_GREYS.len() as f32) as u8;
        self.emit(Particle {
            position: (at + offset).as_dvec3(),
            velocity,
            life,
            total: life,
            size,
            look: Look::Smoke,
            texel: (shade, 0),
            // Negative, like a spark's, and gentler: smoke is buoyant
            // for as long as it is warm, and with the drag below this
            // settles at about a block a second -- a rise a player can
            // watch rather than a jet.
            gravity: -0.45,
            drag: 0.75,
            // **It does not collide, and that is a budget decision.**
            // Collision is a block lookup per particle per frame, and
            // this is the one particle in the game that lives for
            // seconds rather than for a moment. Smoke under a roof
            // going through it is a fault nobody has photographed; a
            // phone paying for a hundred lookups a frame is one that
            // shows up in every frame.
            collides: false,
        });
        true
    }

    /// Sparks and smoke over whatever is burning nearby.
    ///
    /// **Found by sampling, and then remembered.** The client has no
    /// list of fires -- they are blocks like any other -- and walking
    /// every cell around the player once a frame to look for one would
    /// cost more than the particles do. So a few columns are read each
    /// frame and every lit hearth in them is *kept*, in `hearths`,
    /// until it goes out or the player walks away from it.
    ///
    /// The memory is not an optimisation, it is the mechanism. Sampling
    /// alone made emission a coincidence: a fire occupies one cell out
    /// of the thousands in range, so the chance of any given frame
    /// picking it is under a percent, and the spark rate that reads as
    /// `EMBERS_PER_SECOND` in the source came out at about one spark
    /// every twenty seconds in the world -- which is why a lit campfire
    /// looked inert. With the cell held, the rate in the constant is
    /// the rate on the screen, and the smoke can be a steady plume
    /// instead of a stutter.
    pub fn fires(&mut self, chunks: &ChunkManager, at: Vec3, dt: f32) {
        self.find_hearths(chunks, at, dt);

        // Smoke lives for seconds, so a hamlet of fires could hold a
        // few hundred puffs at once. Counted once here rather than
        // capped per fire: what matters is the total on the screen and
        // the total in the vertex buffer.
        let mut smoking = self
            .live
            .iter()
            .filter(|particle| particle.look == Look::Smoke)
            .count();

        for index in 0..self.hearths.len() {
            let (x, y, z) = self.hearths[index].cell;
            let centre = Vec3::new(x as f32 + 0.5, y as f32, z as f32 + 0.5);
            let distance = (centre - at).length();

            self.hearths[index].spark_in -= dt;
            if self.hearths[index].spark_in <= 0.0 {
                self.ember(centre + Vec3::new(0.0, 0.35, 0.0));
                // Reset from now rather than by adding the interval on:
                // a fire that has been off screen for a minute owes no
                // minute of sparks, and paying them all in one frame is
                // a firework.
                let wait = self.wait_for(EMBERS_PER_SECOND);
                self.hearths[index].spark_in = wait;
            }

            // **Fewer puffs the further off the fire is.** A plume
            // forty metres away is a smudge a few pixels wide, and
            // paying full rate for it buys nothing an eye can see while
            // costing exactly as much as the fire at your feet. This is
            // the whole of the distance budget: emission, not culling,
            // so nothing ever pops -- a plume simply thins as you walk
            // away from it, which is what a plume does.
            self.hearths[index].smoke_in -= dt;
            if self.hearths[index].smoke_in > 0.0 {
                continue;
            }
            let nearness = 1.0 - (distance / SMOKE_RANGE).clamp(0.0, 1.0);
            let rate = SMOKE_PER_SECOND * (SMOKE_FAR_SHARE + (1.0 - SMOKE_FAR_SHARE) * nearness);
            let wait = self.wait_for(rate);
            self.hearths[index].smoke_in = wait;
            if distance <= SMOKE_RANGE && smoking < SMOKE_MAX {
                // **Only as far as the air goes.** See `headroom`: a
                // puff that would rise into a block is not born, or dies
                // under it, rather than being drawn black inside it.
                let room = headroom(chunks, (x, y, z));
                if self.smoke_under(centre, room) {
                    smoking += 1;
                }
            }
        }
    }

    /// White water on the rapids near the player: columns of river surface
    /// round `at` looked at a few dozen a second, and where the water there
    /// runs faster than `worldgen::RAPID_SPEED` a few flecks of foam thrown
    /// up off it and carried away on the current. `current` is
    /// `WorldGen::river_current`, which this module has no generator to ask.
    ///
    /// **Sampled, not searched.** The rapids are not remembered anywhere
    /// and need not be: a column picked at random that is a rapid throws
    /// foam, and over a second the columns picked cover the water in view,
    /// so a rapid is white and a lazy reach beside it is not -- for a couple
    /// of block reads a sample, and the generator only where there is
    /// water at the river's level with air over it.
    ///
    /// Rejected: whitening the water's own faces in the mesher. It is the
    /// better picture and a mesh that depends on the generator's current is
    /// a remesh of every river chunk whenever the rule is tuned, for a
    /// texture the atlas has no room for; this is forty lines and costs
    /// nothing where there is no river.
    pub fn rapids(&mut self, chunks: &ChunkManager, at: Vec3, current: impl Fn(f32, f32, f32) -> (f32, f32), dt: f32) {
        use primitive_shared::worldgen::{RAPID_SPEED, SEA_LEVEL};
        self.foam_owed = (self.foam_owed + dt.clamp(0.0, 0.1) * FOAM_SAMPLES_PER_SECOND).min(FOAM_SAMPLES_PER_SECOND);
        let surface = SEA_LEVEL - 1;
        while self.foam_owed >= 1.0 {
            self.foam_owed -= 1.0;
            let x = (at.x + self.between(-FOAM_RANGE, FOAM_RANGE)).floor() as i32;
            let z = (at.z + self.between(-FOAM_RANGE, FOAM_RANGE)).floor() as i32;
            let wet = chunks.block_at(x, surface, z).is_some_and(primitive_shared::types::is_liquid);
            let open = chunks.block_at(x, surface + 1, z).is_some_and(|b| !primitive_shared::types::is_liquid(b) && !primitive_shared::types::is_collidable(b));
            if !wet || !open {
                continue;
            }
            let (fx, fz) = (x as f32 + self.random(), z as f32 + self.random());
            let (cx, cz) = current(fx, surface as f32 + 0.5, fz);
            let speed = cx.hypot(cz);
            if speed < RAPID_SPEED || !speed.is_finite() {
                continue;
            }
            // Wilder the faster it runs: more flecks, thrown higher.
            let wild = (speed / RAPID_SPEED).min(2.5);
            for _ in 0..(FOAM_BURST as f32 * wild) as usize {
                let life = self.between(0.5, 1.1);
                let velocity = Vec3::new(
                    cx + self.between(-0.4, 0.4),
                    self.between(0.3, 0.9) * wild,
                    cz + self.between(-0.4, 0.4),
                );
                let position = Vec3::new(fx + self.between(-0.4, 0.4), surface as f32 + 0.9, fz + self.between(-0.4, 0.4));
                let size = self.between(0.06, 0.12) * wild.sqrt();
                self.emit(Particle {
                    position: position.as_dvec3(),
                    velocity,
                    life,
                    total: life,
                    size,
                    look: Look::Foam,
                    texel: (0, 0),
                    gravity: 4.0,
                    drag: 0.6,
                    collides: false,
                });
            }
        }
    }

    /// How long to wait before doing something that happens `rate`
    /// times a second.
    ///
    /// Jittered by a third either way, because two fires side by side
    /// on exact intervals spark in lockstep -- which reads as one fire
    /// with a stutter rather than as two.
    fn wait_for(&mut self, rate: f32) -> f32 {
        let jitter = self.between(0.66, 1.34);
        jitter / rate.max(0.01)
    }

    /// Keeps `hearths` up to date: forgets the ones that have gone out
    /// or been left behind, and reads a few columns looking for new
    /// ones.
    ///
    /// Columns rather than loose cells, and that is `ChunkManager`'s
    /// arithmetic rather than a preference: a single-cell lookup hashes
    /// the chunk position, while a column hashes once and then indexes.
    /// Reading thirteen cells of one column therefore costs about what
    /// one scattered cell costs, and a fire is a thing that stands on
    /// the ground -- so a column through the player's own band of
    /// heights is exactly the shape of the search.
    ///
    /// **A budget per second, not per frame**, for the reason every
    /// other emitter here is: a search that reads four columns a frame
    /// sweeps its circle three times faster at 60 fps than at 20, so a
    /// phone would find the fire ten seconds after a desktop did. The
    /// per-frame ceiling is there so a hitch asks for a burst rather
    /// than for a thousand columns at once.
    ///
    /// **Biased toward the player.** The offset is a uniform direction
    /// scaled by a *second* random number, which piles the samples up
    /// near the middle -- so the fire at your feet is found within a
    /// few frames and the one across the clearing takes a second or
    /// two, which is the order anybody notices them in.
    fn find_hearths(&mut self, chunks: &ChunkManager, at: Vec3, dt: f32) {
        let eye = at.y.floor() as i32;
        self.hearths.retain(|hearth| {
            let (x, y, z) = hearth.cell;
            // A burning pit kiln or log pile smokes as a hearth does
            // (`pit::smokes`), and without it a kiln alight for an hour
            // was a heap of glowing logs with no plume over it.
            let still_here = chunks
                .block_at(x, y, z)
                .is_some_and(|block| primitive_shared::hearth::is_lit(block) || primitive_shared::pit::smokes(block));
            let dx = x as f32 + 0.5 - at.x;
            let dz = z as f32 + 0.5 - at.z;
            // A little further than the search, so a fire found at the
            // edge of it is not dropped and refound as the player
            // shifts their weight.
            still_here && dx * dx + dz * dz < FIRE_KEEP * FIRE_KEEP
        });

        let columns = ((COLUMNS_PER_SECOND * dt).ceil() as usize).min(COLUMN_BURST);
        for _ in 0..columns {
            if self.hearths.len() >= MAX_HEARTHS {
                return;
            }
            let reach = self.random();
            let x = (at.x + self.between(-FIRE_RANGE, FIRE_RANGE) * reach).floor() as i32;
            let z = (at.z + self.between(-FIRE_RANGE, FIRE_RANGE) * reach).floor() as i32;
            let Some(column) = chunks.column(x, z) else {
                continue;
            };
            for y in eye - FIRE_BELOW..=eye + FIRE_ABOVE {
                let here = column.block(y);
                if !primitive_shared::hearth::is_lit(here) && !primitive_shared::pit::smokes(here) {
                    continue;
                }
                let cell = (x, y, z);
                if self.hearths.iter().any(|known| known.cell == cell) {
                    continue;
                }
                // A fresh fire smokes at once rather than after a full
                // interval: a plume that took a second to start would
                // make walking into view of a camp look like the camp
                // lighting up as you arrived.
                let (spark_in, smoke_in) = (self.wait_for(EMBERS_PER_SECOND), 0.0);
                self.hearths.push(Hearth {
                    cell,
                    spark_in,
                    smoke_in,
                });
            }
        }
    }

    /// Appends every live particle to a terrain mesh, as quads facing
    /// the camera.
    ///
    /// `right` and `up` are the camera's own axes: a particle is a flat
    /// picture and the only orientation that never shows it edge-on is
    /// the one that faces the viewer. The rain is the exception and
    /// makes the case for the rest -- a drop is stretched along the way
    /// it is *going*, because that is what a falling drop looks like to
    /// an eye that cannot resolve it.
    ///
    /// **Measured from `origin`, the frame's render origin, like every
    /// other thing that reaches the card.** The particles live in world
    /// coordinates -- the collisions ask the world about them -- and the
    /// pass used to upload them that way, on the reasoning that its
    /// shader "uses `view_proj` directly". It does, and `view_proj` has
    /// since been built around the origin (`Camera::view_proj_about`),
    /// so a world position fed to it was drawn *the origin further on*.
    /// The origin is the player's own floored position from the first
    /// frame of a world, so every drop of blood, spark and raindrop was
    /// drawn the player's altitude straight up and the player's x and z
    /// across: seventy blocks overhead at a spawn near the middle of the
    /// map, and fogged out of sight anywhere else -- which is why the
    /// player saw blood in the sky "a couple of times" and not always.
    /// The shader's fog distance is measured against the origin-relative
    /// `camera_pos` too, so it was wrong in the same way and is right in
    /// the same way now.
    ///
    /// `right` and `up` are `billboard_axes` for the camera, which says why
    /// they are the camera's own tilted up and not the world's; `light` is the
    /// world's light map, which everything that is not weather or a spark is
    /// lit from (see `Look::light`).
    #[allow(clippy::too_many_arguments)]
    pub fn build_into(
        &self,
        origin: Vec3,
        right: Vec3,
        up: Vec3,
        layers: &FaceLayers,
        light: &LightMap,
        vertices: &mut Vec<ParticleVertex>,
        indices: &mut Vec<u32>,
    ) {
        for particle in &self.live {
            let layer = particle.look.layer(layers);
            let word = particle.look.light().unwrap_or_else(|| {
                let (sky, block) = crate::logic::entities::sampled_light(particle.position, light);
                pack_light(sky, block, 3, 0)
            });
            let centre = (particle.position - origin.as_dvec3()).as_vec3();

            // **Smaller as it goes**, rather than blinking off at full
            // size: the last fifth of a life is where a chip settles
            // into the grass and a splash soaks in.
            let fade = (particle.life / (particle.total * 0.2).max(1e-3)).clamp(0.0, 1.0);
            let left = (particle.life / particle.total.max(1e-3)).clamp(0.0, 1.0);
            // **Smoke goes the other way: it swells as it cools.** A
            // puff that shrank as it rose would read as a spark going
            // out, and the widening is most of what says a plume is
            // rising rather than sliding upward -- so the tail-end
            // shrink everything else uses is skipped for it, and the
            // fading is done entirely in the alpha.
            let size = match particle.look {
                Look::Smoke => particle.size * (1.0 + SMOKE_GROWTH * (1.0 - left)),
                // ...and blood in water spreads the same way, faster.
                Look::Cloud => particle.size * (1.0 + CLOUD_GROWTH * (1.0 - left)),
                // **A stain does not shrink as it goes.** The
                // tail-end shrink is a chip settling into the grass or
                // a splash soaking in, and both are things that are
                // *leaving*; a mark on the ground stays the size it
                // was made and dries out instead, which is done
                // entirely in the alpha. Shrinking one reads as the
                // ground healing over.
                Look::Stain => particle.size,
                _ => particle.size * fade,
            };
            let tint = particle.look.tint(fade, left, particle.texel.0);

            let (across, along) = match particle.look {
                Look::Rain => {
                    // Along its own path, and long: the streak is the
                    // drop's travel over the time an eye integrates.
                    let direction = particle.velocity.normalize_or_zero();
                    // **Across it in the picture**: square to the way it
                    // falls and to the way the camera looks. It was square to
                    // the world's up, which for straight rain is the fall
                    // itself, so the streak stood in the world like a fence
                    // post and was seen edge-on from above.
                    let toward = up.cross(right);
                    let across = direction.cross(toward);
                    if across.length_squared() > 1e-4 {
                        (across.normalize() * size, direction * -(size * 14.0))
                    } else {
                        // Falling straight down the line of sight: a streak
                        // seen end-on is a dot.
                        (right * size, up * size)
                    }
                }
                // **Flat on the ground, and it does not turn to face
                // anybody.** Every other particle here is a speck seen
                // from wherever the player happens to be, so facing the
                // camera is the one orientation that never shows it
                // edge-on. A stain is a mark *on a surface*: laid in
                // the camera's plane it stands up out of the ground and
                // spins as the player walks round it, which is the one
                // thing a stain must never do. So it is built in world
                // axes, turned about the vertical by its own angle --
                // see `Particles::stain`.
                Look::Stain => {
                    let turn = particle.texel.1 as f32 / 256.0 * std::f32::consts::TAU;
                    let (sin, cos) = turn.sin_cos();
                    (
                        Vec3::new(cos, 0.0, sin) * size,
                        Vec3::new(-sin, 0.0, cos) * size,
                    )
                }
                _ => (right * size, up * size),
            };

            // Which corner of the picture this particle wears. A chip
            // takes one texel of its block; everything else takes the
            // whole of its own small picture.
            let (u0, v0, u1, v1) = match particle.look {
                Look::Chip(_) => {
                    let step = 1.0 / TEXTURE_GRID as f32;
                    let u = particle.texel.0 as f32 * step;
                    let v = particle.texel.1 as f32 * step;
                    (u, v, u + step, v + step)
                }
                _ => (0.0, 0.0, 1.0, 1.0),
            };

            let base = vertices.len() as u32;
            for &(dx, dy, u, v) in &[
                (-1.0f32, 1.0f32, u0, v0),
                (1.0, 1.0, u1, v0),
                (1.0, -1.0, u1, v1),
                (-1.0, -1.0, u0, v1),
            ] {
                let corner = centre + across * dx + along * dy;
                vertices.push(ParticleVertex {
                    position: [corner.x, corner.y, corner.z],
                    uv: [u, v],
                    packed: (layer << 16) | word,
                    tint,
                });
            }
            // Both windings, so a particle is there whichever side of it
            // the camera ends up on -- which for something this small
            // happens constantly as it tumbles past.
            indices.extend_from_slice(&[
                base,
                base + 1,
                base + 2,
                base,
                base + 2,
                base + 3,
                base,
                base + 2,
                base + 1,
                base,
                base + 3,
                base + 2,
            ]);
        }
    }
}

impl Particles {
    /// Puts every particle that has a water surface between it and the eye
    /// at the front of `indices`, and says how many indices that is.
    ///
    /// `indices` is what `build_into` wrote, from its start: twelve a
    /// particle, in the order of the pool. Anything written after them --
    /// the small life rides the same buffer -- is left where it is.
    ///
    /// **Why the particle pass is split round the water.** Water is blended
    /// and writes no depth, so a particle drawn after it is depth-tested
    /// against the ground alone -- and a speck on the far side of a surface
    /// passes that test and lands *on top of* the water, at full strength,
    /// as if nothing stood between. Under a lake in the rain that is every
    /// streak of the shower falling on the surface over the swimmer's head,
    /// drawn crisp and white across the underside of the water: the stripes
    /// a player reported under water ("под водой полосы какие то"). From the
    /// bank it is the blood clouding in the pond drawn over the pond.
    ///
    /// Three ways to put the water between them were weighed:
    ///
    /// * *Let the water write depth.* Then nothing behind a surface is drawn
    ///   at all, which is the bed of every lake, and the underside of the
    ///   sea seen by a swimmer is a lid over nothing.
    /// * *Draw every particle before the water.* Right for these, wrong for
    ///   all the rest: rain over a lake seen from the shore would have the
    ///   lake blended over it, the drops drowned in blue in mid-air.
    /// * **Split by side (chosen).** The eye and the particle each answer
    ///   one question -- under the drawn surface or not -- and the ones that
    ///   answer differently from the eye go before the water, so the water
    ///   is composited over them exactly as it is over the bed. One lookup a
    ///   particle, on a pool of a few hundred, and one more draw.
    ///
    /// Side rather than ray: a speck in an air pocket under a sea is on the
    /// eye's side by this test and behind water on the ray. That is a cave
    /// under a lake with a fire in it, seen from the lake, and it is drawn as
    /// it always was.
    pub fn behind_water_first(&self, chunks: &ChunkManager, eye: Vec3, indices: &mut [u32]) -> u32 {
        let eye_under = under_water_at(chunks, eye);
        let count = (self.live.len() * INDICES_PER_PARTICLE).min(indices.len());
        let (mut behind, mut rest) = (Vec::new(), Vec::new());
        for (particle, quad) in self.live.iter().zip(indices[..count].chunks(INDICES_PER_PARTICLE)) {
            if under_water_at(chunks, particle.position.as_vec3()) != eye_under {
                behind.extend_from_slice(quad);
            } else {
                rest.extend_from_slice(quad);
            }
        }
        let split = behind.len() as u32;
        behind.extend_from_slice(&rest);
        indices[..count].copy_from_slice(&behind);
        split
    }
}

/// The indices `build_into` writes for one particle: a quad, both windings.
const INDICES_PER_PARTICLE: usize = 12;

/// Is this point below the drawn surface of water?
///
/// Asked of the surface the mesher draws (`fluid::covers_with_above`)
/// rather than of the cell (`liquid_at`), because the question is which
/// side of that surface a thing is seen from: a raindrop stops at the top
/// of the cell it lands in, a hair above the water, and belongs on the
/// air's side of it.
pub fn under_water_at(chunks: &ChunkManager, at: Vec3) -> bool {
    let (x, y, z) = (at.x.floor() as i32, at.y.floor() as i32, at.z.floor() as i32);
    let Some(block) = chunks.block_at(x, y, z) else {
        return false;
    };
    let above = chunks.block_at(x, y + 1, z).unwrap_or(primitive_shared::types::BLOCK_AIR);
    primitive_shared::fluid::covers_with_above(block, above, at.y - y as f32)
}

/// Anything a raindrop bursts on: ground, or the top of water.
///
/// Its own function beside `solid_at` rather than a flag on it, because
/// the two questions are different and only one of them is about
/// standing on something.
fn surface_at(chunks: &ChunkManager, at: Vec3) -> bool {
    let (x, y, z) = (
        at.x.floor() as i32,
        at.y.floor() as i32,
        at.z.floor() as i32,
    );
    chunks.block_at(x, y, z).is_some_and(|block| {
        primitive_shared::types::is_collidable(block) || primitive_shared::types::is_liquid(block)
    })
}

/// Is there something solid at this point?
///
/// Unloaded chunks read as *empty* rather than as solid, which is the
/// opposite of what the animals do and right for the opposite reason: an
/// animal must not walk off the edge of the world, and a raindrop that
/// bursts on terrain that has not arrived is a splash in mid-air.
fn solid_at(chunks: &ChunkManager, at: Vec3) -> bool {
    let (x, y, z) = (
        at.x.floor() as i32,
        at.y.floor() as i32,
        at.z.floor() as i32,
    );
    chunks
        .block_at(x, y, z)
        .is_some_and(primitive_shared::types::is_collidable)
}

/// The top of whatever a falling drop has just landed on, or `None` if
/// it did not land on a top at all.
///
/// **Where a mark on the ground is allowed to be.** `solid_at` answers
/// "is this point inside something", which is all a drop needs to stop
/// and nowhere near enough to lay a decal: a drop that hit the *side* of
/// a boulder would put its stain on the boulder's roof, a metre above
/// where anything touched it, and one that hit a ceiling would put it
/// there too. So two things are asked. It has to have been going *down*
/// and to have crossed the top face during this step -- which is the
/// same as saying it came from the free air above -- and the mark is
/// then laid on that face rather than at the point of contact.
///
/// The face, not the cell's roof: a block that fills half its cell (a
/// path, a layer of ash, a slab -- `types::block_layers`) is stood on at
/// its own height, and the collision test above cannot see that because
/// it only knows the id. A stain on the roof of a half block would hang
/// four inches over it.
fn landing_face(chunks: &ChunkManager, from: Vec3, to: Vec3) -> Option<Vec3> {
    if to.y >= from.y {
        return None;
    }
    let cell = (to.x.floor() as i32, to.y.floor() as i32, to.z.floor() as i32);
    let block = chunks.block_at(cell.0, cell.1, cell.2)?;
    if !primitive_shared::types::is_collidable(block) {
        return None;
    }
    // **A frame is not a floor.** A drying rack is a full cell to
    // everything that asks "is this solid" -- the drop stops on it, the
    // player walks round it -- and it is four poles with daylight
    // between them (`mesh::rack_block`). Photographed on the plaza of
    // the test world: a mark laid on the top of a rack's cell is a red
    // disc bridging the gap between two poles with grass showing a
    // metre below it. `collision_depth` is the table's own answer to
    // "does this stand narrower than its cell", so it is what is asked.
    if primitive_shared::types::collision_depth(block).is_some() {
        return None;
    }
    let top = cell.1 as f32
        + primitive_shared::types::block_layers(block) as f32
            / primitive_shared::types::LAYERS_PER_BLOCK as f32;
    if from.y < top {
        return None;
    }
    // **Not under water.** A mark on a lake bed is the other half of what
    // `Look::Cloud` cures, and a drop that crossed the surface and the floor
    // in one step would otherwise still leave one.
    if chunks
        .block_at(cell.0, (top + STAIN_LIFT).floor() as i32, cell.2)
        .is_some_and(primitive_shared::types::is_liquid)
    {
        return None;
    }
    Some(Vec3::new(to.x, top + STAIN_LIFT, to.z))
}

/// Is this point in water, or anything else that flows? What a drop of blood
/// asks to know whether it is still a drop. See `Look::Cloud`.
fn liquid_at(chunks: &ChunkManager, at: Vec3) -> bool {
    chunks
        .block_at(at.x.floor() as i32, at.y.floor() as i32, at.z.floor() as i32)
        .is_some_and(primitive_shared::types::is_liquid)
}

/// A drop of blood, or a mark, in water: it stops being a drop and clouds.
///
/// Most of its speed goes into the water at once and what is left bleeds off
/// within a fraction of a second, so it spreads where it went in rather than
/// sinking. It keeps its shade, which rides in `texel.0` for a drop and a mark
/// alike.
fn dissolve(particle: &mut Particle) {
    particle.look = Look::Cloud;
    particle.velocity *= CLOUD_KEEPS;
    particle.gravity = 0.0;
    particle.drag = CLOUD_DRAG;
    particle.collides = false;
    particle.size = particle.size.max(CLOUD_SIZE);
    particle.life = CLOUD_LIFE;
    particle.total = CLOUD_LIFE;
}

/// What a drop of blood becomes in water. See `Look::Cloud`.
///
/// **About a second, and gone**: long enough that a blow landed on a fish or a
/// swimmer is seen, short enough that a fight in the shallows does not become a
/// red fog. It keeps a tenth of the speed it went in with, and `CLOUD_DRAG` is
/// what a whole second leaves of that; it grows to three and a half times its
/// size, starting no smaller than a small stain, and is never more than half
/// opaque -- a cloud, not a drop.
const CLOUD_LIFE: f32 = 1.0;
const CLOUD_KEEPS: f32 = 0.1;
const CLOUD_DRAG: f32 = 0.02;
const CLOUD_SIZE: f32 = 0.07;
const CLOUD_GROWTH: f32 = 2.5;
const CLOUD_ALPHA: f32 = 0.5;

/// How many columns of water round the player `Particles::rapids` looks at
/// a second, and how far out: two hundred and forty over a square
/// forty-eight blocks a side is every column in view looked at about once in
/// ten seconds -- and a rapid thirty blocks long and five wide is looked at
/// fifteen times a second and throws foam forty-odd times, which is a white
/// streak rather than a flicker, for five hundred block reads a second.
const FOAM_SAMPLES_PER_SECOND: f32 = 240.0;
const FOAM_RANGE: f32 = 24.0;
/// Flecks a rapid column throws at the rapid speed, more when faster.
const FOAM_BURST: usize = 3;
/// The most opaque a fleck of foam is: a speck of spray, not a snowball.
const FOAM_ALPHA: f32 = 0.75;

/// How many drops a second fall at full intensity.
///
/// Six hundred alive at a time, near enough: they live about a second
/// each. Enough to read as rain in the middle distance, which is the
/// only place rain is ever read.
const RAIN_PER_SECOND: f32 = 600.0;

/// How far out from the player they are scattered, and how far above.
const SCATTER: f32 = 9.0;
const CEILING: f32 = 11.0;

const RAIN_SPEED: f32 = 22.0;
const SNOW_SPEED: f32 = 1.8;

/// How many chips a broken block throws.
const CHIPS: usize = 12;

/// How many drops one blow throws.
///
/// **Fewer than a broken block, and deliberately.** A block comes apart
/// once; a blow lands every time a wolf closes, and the burst has to be
/// something the eye reads at the edge of vision without becoming the
/// thing it is looking at. Eight is enough to be a spray and few enough
/// that a fight of a dozen blows is a hundred particles against a cap
/// of twelve hundred.
const BLOOD_DROPS: usize = 8;

/// How much of a blow's throw a drip off a cut leaves with. See
/// `Particles::spray`.
const DRIP_THROW: f32 = 0.2;

/// What blood is, as multipliers on the flake it wears.
///
/// **Four reds and not one.** A burst of identical dots reads as a
/// texture rather than as blood -- the same reason a chip of a broken
/// block takes one texel of it at random. Dark, because the flake is
/// very nearly white and a bright red over white is a cherry; the
/// darkest of these against the flake's 246,250,254 comes out about
/// 79,5,8, which is a colour blood is and a colour nothing else in this
/// game is.
///
/// Red first by a long way in every one of them, and there is a test
/// that says so: these are multiplied by a picture drawn in a colour
/// that was chosen for snow, and a palette that let the blue through
/// would be a spray of mauve.
const BLOOD: [[f32; 3]; 4] = [
    [0.32, 0.02, 0.03],
    [0.44, 0.04, 0.04],
    [0.58, 0.07, 0.06],
    [0.24, 0.01, 0.02],
];

/// How long a mark on the ground lasts, in seconds.
///
/// **Half a minute to three quarters of one**, which is the answer to
/// "seconds or minutes": long enough that a player who was bitten,
/// backed off and came back still finds the ground where it happened,
/// short enough that a week-old meadow is not a slaughterhouse. Nothing
/// remembers it -- walk away and come back and the marks are gone,
/// because they were never in the world (see `Look::Stain`).
///
/// Scattered rather than fixed, so the marks of one blow do not all
/// vanish in the same frame: eight dots blinking out together is a
/// thing the eye catches, eight fading over ten seconds is not.
const STAIN_LIFE: (f32, f32) = (28.0, 46.0);

/// How wide a mark is, as a half-width in blocks.
///
/// A hand's breadth to a foot across. Big enough to read from standing
/// height, small enough that the eight of one blow are a spatter rather
/// than a pool -- which is the difference between "something bled here"
/// and "a bucket of paint was dropped here".
const STAIN_SIZE: (f32, f32) = (0.055, 0.115);

/// How far a mark floats above the face it lies on.
///
/// **A fiftieth of a block, and the number is a depth-buffer decision.**
/// The particle pass tests depth with `Less` against the terrain that is
/// already drawn (it writes none of its own -- see `particle_pipeline`),
/// so a quad exactly in the ground's plane is a quad whose depth is
/// whatever the last bit of the arithmetic says, differently for every
/// pixel: it flickers, in patches, and moves as the player does. Lifted
/// it always wins. A fiftieth is a third of a texel -- far enough to
/// clear the noise at the far edge of a lit room, near enough that a
/// mark seen from a standing player's eye is on the floor and not
/// hovering over it.
const STAIN_LIFT: f32 = 0.02;

/// The most marks on the ground at once.
///
/// Sixty-four quads: a twentieth of `MAX` and about six blows' worth of
/// spatter. A cap on the *look* rather than on the pool, because this is
/// the one look here that outlives the moment it was made -- see
/// `Particles::stain` for what happens at the ceiling.
const STAIN_MAX: usize = 64;

/// How much darker a mark is than the spray that made it, and how
/// opaque it is at its strongest.
///
/// **Measured off the screen, not off the arithmetic.** At 0.55 the
/// marks photographed on the meadow at 130,48,43 -- a brick red, which
/// is what fresh paint looks like rather than what blood in grass looks
/// like. Two fifths brings them to 100-115 red: dark enough to read
/// as soaked in, light enough not to be a hole in the ground. (The
/// number that comes out of multiplying the palette by the flake is
/// half of that; whatever the pass does with the rest of it does the
/// same thing to every particle in the game, so the photograph is the
/// measurement that counts.)
///
/// Not fully opaque, so the grass and the grain of the ground read
/// through the mark and it lies *on* the floor instead of being a hole
/// cut in it.
const STAIN_DARKEN: f32 = 0.42;
const STAIN_ALPHA: f32 = 0.85;

/// How many texels across a block texture is, for the purpose of
/// picking one.
///
/// Sixteen, which is what the stock pack is. A pack drawn at 32 would
/// have its chips take one *quarter* of a texel each -- still a flat
/// colour off the right block, which is all a chip has to be, so this is
/// a constant rather than something threaded through from the texture
/// manager.
const TEXTURE_GRID: u32 = 16;

/// The texel of `tools/fishing_rod.png` the float is painted in: red, near
/// the hook end of the line. [`Particles::float`] wears it, so the float on
/// the water is the float in the pack.
pub const FLOAT_TEXEL: (u8, u8) = (14, 11);

/// How many columns are read a second when hunting for a fire, the most
/// one frame may ask for, how far out they reach, and how much of the
/// height around the player they cover.
///
/// Two hundred and forty columns of thirteen cells is about three
/// thousand block reads a second through two hundred and forty hash
/// lookups -- of the order the twelve scattered cells a frame this
/// replaced cost, and it sweeps a thirty-metre circle in under twenty
/// seconds while finding anything close in a fraction of one. See
/// `find_hearths` for the bias that buys the second half of that.
const COLUMNS_PER_SECOND: f32 = 240.0;
const COLUMN_BURST: usize = 24;
const FIRE_RANGE: f32 = 30.0;
const FIRE_BELOW: i32 = 5;
const FIRE_ABOVE: i32 = 7;

/// How far a fire found earlier is kept.
///
/// Wider than the search, and the gap is hysteresis: a fire sitting
/// exactly on the rim would otherwise be dropped and refound as the
/// player shifted their weight, and its plume would come and go.
const FIRE_KEEP: f32 = FIRE_RANGE + 4.0;

/// How many fires are kept at once.
///
/// A cap on the work, not on the world: eight is more than any camp has
/// and it bounds both the per-frame upkeep (one block read each) and
/// the particles, since every remembered fire is emitting.
const MAX_HEARTHS: usize = 8;

/// How often a fire throws a spark, and a puff of smoke.
///
/// **Both are now what they say.** Under the old sampler the effective
/// rate was the constant times the chance of the sample landing on the
/// fire's own cell, which was under a percent -- so six a second was
/// one every twenty seconds and a campfire threw no sparks at all. With
/// the cell remembered these are real rates, and they were turned down
/// to match: three sparks a second is a fire, six was a forge bellows.
const EMBERS_PER_SECOND: f32 = 3.0;
const SMOKE_PER_SECOND: f32 = 7.0;

/// How far a fire smokes, and how much of the rate is left at that
/// edge.
///
/// The plume has to be visible from across a clearing -- that is where
/// smoke earns its keep, as the thing that says somebody is camped over
/// there -- so the range is well beyond the sparks. What falls off with
/// distance is the *rate*: at the rim a fire emits a third as often,
/// which is a plume that still reads at the size it is drawn and costs
/// a third as many particles.
const SMOKE_RANGE: f32 = FIRE_RANGE;
const SMOKE_FAR_SHARE: f32 = 0.33;

/// The most puffs alive at once, over every fire together.
///
/// Smoke lives for seconds where everything else here lives for a
/// moment, so it is the one look that can accumulate. Ninety-six quads
/// is under a tenth of `MAX` and about a thousandth of what one chunk
/// of terrain draws, and it is the number a phone is being protected
/// from: a hamlet of eight fires cannot cost more than one.
const SMOKE_MAX: usize = 160;

/// How long a puff lasts, where it is born relative to the fire's cell,
/// how grey it is, how opaque at its strongest, and how much it swells
/// over its life.
///
/// The greys are multipliers on a picture drawn for snow -- 246,250,254
/// -- so 0.17 of it is about 42,42,44: woodsmoke against a bright sky,
/// dark enough to read against cloud and light enough not to be a hole.
/// Faintly blue rather than neutral, because a flat grey over a blue
/// sky reads as brown.
///
/// **Four of them and not one**, the same trade blood makes: a plume is
/// the same small round picture stamped twenty times, and twenty stamps
/// of one colour read as a pattern where twenty of four read as smoke.
/// The lightest is nearly twice the darkest, which is the difference
/// between the edge of a plume and its middle.
const SMOKE_LIFE: (f32, f32) = (2.2, 3.6);
/// How big a puff is born, smallest and largest, before `SMOKE_GROWTH`.
const SMOKE_SIZE: (f32, f32) = (0.13, 0.2);
/// How far over a fire's floor smoke is looked for a ceiling, in blocks.
/// Past it a puff has faded before it could arrive (`SMOKE_LIFE` at the
/// fastest rise is under seven blocks).
const SMOKE_HEADROOM_MAX: i32 = 8;

/// How much open air there is over the floor of `cell`, in blocks, up to
/// `SMOKE_HEADROOM_MAX`: the height of the first block above it that the
/// sky cannot pass (`types::blocks_the_sky`, the fog's and the rain's own
/// ceiling), counted from the bottom of the fire's cell.
///
/// A cell nobody has loaded is taken as open. A puff that fades early over
/// the edge of the world costs nothing; one refused there would be a plume
/// that switches on when a chunk arrives.
fn headroom(chunks: &ChunkManager, (x, y, z): (i32, i32, i32)) -> f32 {
    let Some(column) = chunks.column(x, z) else {
        return SMOKE_HEADROOM_MAX as f32;
    };
    for above in 1..=SMOKE_HEADROOM_MAX {
        if primitive_shared::types::blocks_the_sky(column.block(y + above)) {
            return above as f32;
        }
    }
    SMOKE_HEADROOM_MAX as f32
}

/// The shortest time a puff can take to rise `distance` blocks: its
/// fastest launch (1.2 a second) with its lift (`gravity` -0.45) and no
/// drag at all, which is faster than any puff ever goes -- so a life cut to
/// this is a puff that is gone before it reaches the ceiling, never one
/// that arrives.
fn seconds_to_rise(distance: f32) -> f32 {
    const V: f32 = 1.2;
    const A: f32 = 0.45;
    ((V * V + 2.0 * A * distance.max(0.0)).sqrt() - V) / A
}
const SMOKE_BIRTH_HEIGHT: f32 = 1.06;
const SMOKE_GREYS: [[f32; 3]; 4] = [
    [0.13, 0.135, 0.15],
    [0.17, 0.175, 0.19],
    [0.21, 0.215, 0.23],
    [0.25, 0.255, 0.275],
];
const SMOKE_ALPHA: f32 = 0.34;
const SMOKE_GROWTH: f32 = 2.2;

#[cfg(test)]
mod tests {
    use super::*;
    use primitive_shared::lighting::LightMap;
    use primitive_shared::types::{BLOCK_AIR, BLOCK_STONE};

    fn world_with_floor() -> ChunkManager {
        crate::logic::physics::tests::floor_world()
    }

    fn drop_at(position: Vec3) -> Particle {
        Particle {
            position: position.as_dvec3(),
            velocity: Vec3::new(0.0, -20.0, 0.0),
            life: 2.0,
            total: 2.0,
            size: 0.04,
            look: Look::Rain,
            texel: (0, 0),
            gravity: 0.0,
            drag: 1.0,
            collides: true,
        }
    }

    #[test]
    fn a_particle_falls_and_then_expires() {
        let chunks = ChunkManager::new(4);
        let mut particles = Particles::new();
        let mut drop = drop_at(Vec3::new(0.5, 40.0, 0.5));
        drop.collides = false;
        drop.life = 0.1;
        particles.emit(drop);

        particles.update(&chunks, 0.05);
        assert_eq!(particles.len(), 1, "it died halfway through its life");
        assert!(particles.live[0].position.y < 40.0, "it did not fall");

        particles.update(&chunks, 0.1);
        assert!(particles.is_empty(), "it outlived its lifetime");
    }

    #[test]
    fn rain_bursts_on_what_it_lands_on() {
        // **The thing a sheet of rain could never do.** A drop stops at
        // the ground, and what is left is a splash rather than a drop
        // that carried on through the floor.
        let chunks = world_with_floor();
        let mut particles = Particles::new();
        // Just above the floor `floor_world` builds -- stone below ten
        // -- falling fast enough to cross it in one step.
        particles.emit(drop_at(Vec3::new(0.5, 10.4, 0.5)));
        particles.update(&chunks, 0.1);

        assert!(
            particles.live.iter().all(|p| p.look != Look::Rain),
            "the drop went through the floor"
        );
        assert!(
            particles.live.iter().any(|p| p.look == Look::Splash),
            "it landed and left nothing"
        );
    }

    /// **What is across a water surface from the eye is drawn before the
    /// water.** The blended pass writes no depth, so a particle drawn after
    /// it lands on top of it: a swimmer under a lake in the rain saw every
    /// streak of the shower over their head drawn crisp across the underside
    /// of the water -- "под водой полосы". Checked on what decides the order,
    /// the front of the index list, from both sides of the surface, with a
    /// splash the rain itself left on the lake among the particles.
    #[test]
    fn particles_across_a_water_surface_from_the_eye_come_before_the_rest() {
        let chunks = crate::logic::physics::tests::world_of(|y| match y {
            0..=9 => primitive_shared::types::BLOCK_STONE,
            10..=13 => primitive_shared::types::BLOCK_WATER,
            _ => primitive_shared::types::BLOCK_AIR,
        });
        let mut particles = Particles::new();
        // A shower falling on the lake: the drop bursts on the surface and
        // what it leaves is what a swimmer looks up at.
        particles.emit(drop_at(Vec3::new(0.5, 14.4, 0.5)));
        particles.update(&chunks, 0.1);
        let mut high = drop_at(Vec3::new(3.5, 17.0, 3.5));
        high.collides = false;
        particles.emit(high);
        let mut cloud = drop_at(Vec3::new(2.5, 12.0, 2.5));
        cloud.collides = false;
        cloud.look = Look::Cloud;
        particles.emit(cloud);
        let in_water: Vec<bool> = particles.live.iter().map(|p| under_water_at(&chunks, p.position.as_vec3())).collect();
        assert!(in_water.iter().any(|&w| w) && in_water.iter().any(|&w| !w), "the fixture needs both sides: {in_water:?}");

        let layers = FaceLayers::empty_for_test();
        for (eye, eye_in_water) in [(Vec3::new(1.5, 12.5, 1.5), true), (Vec3::new(1.5, 15.6, 1.5), false)] {
            let (mut vertices, mut indices) = (Vec::new(), Vec::new());
            particles.build_into(Vec3::ZERO, Vec3::X, Vec3::Y, &layers, &LightMap::new(), &mut vertices, &mut indices);
            let mut before = indices.clone();
            let split = particles.behind_water_first(&chunks, eye, &mut indices) as usize;
            let expected = in_water.iter().filter(|&&w| w != eye_in_water).count() * INDICES_PER_PARTICLE;
            assert_eq!(split, expected, "eye in water: {eye_in_water}");
            for (range, across) in [(0..split, true), (split..indices.len(), false)] {
                for quad in indices[range].chunks(INDICES_PER_PARTICLE) {
                    let particle = quad[0] as usize / 4;
                    assert!(quad.iter().all(|&i| i as usize / 4 == particle), "a quad was torn apart: {quad:?}");
                    assert_eq!(
                        in_water[particle] != eye_in_water,
                        across,
                        "particle {particle} at {:?} is on the wrong side of the water pass (eye in water: {eye_in_water})",
                        particles.live[particle].position
                    );
                }
            }
            before.sort_unstable();
            indices.sort_unstable();
            assert_eq!(before, indices, "reordering lost or invented indices");
        }
    }

    #[test]
    fn rain_bursts_on_a_lake_rather_than_on_its_bed() {
        // **The one place a shower is most obviously a shower.** The
        // collision test asks `is_collidable`, which water is not, so
        // every drop fell through the lake and burst on the bottom of
        // it -- pale flecks two metres under the surface, and a flat
        // dead sheet of water in a downpour.
        let chunks = crate::logic::physics::tests::world_of(|y| match y {
            0..=9 => primitive_shared::types::BLOCK_STONE,
            10..=13 => primitive_shared::types::BLOCK_WATER,
            _ => primitive_shared::types::BLOCK_AIR,
        });
        let mut particles = Particles::new();
        particles.emit(drop_at(Vec3::new(0.5, 14.4, 0.5)));
        particles.update(&chunks, 0.1);
        assert!(
            particles.live.iter().all(|p| p.look != Look::Rain),
            "the drop went into the lake"
        );
        let splashes: Vec<&Particle> =
            particles.live.iter().filter(|p| p.look == Look::Splash).collect();
        assert!(!splashes.is_empty(), "it hit the water and left nothing");
        assert!(
            splashes.iter().all(|p| p.position.y > 13.0),
            "the splash is under the surface: {:?}",
            splashes.iter().map(|p| p.position.y).collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_roof_keeps_the_rain_off() {
        // The other half of the same rule, and the one a player notices:
        // rain that fell through a roof was what made the old version
        // read as a texture in front of the camera rather than as
        // weather in the world.
        let chunks = world_with_floor();
        let mut particles = Particles::new();
        particles.emit(drop_at(Vec3::new(0.5, 10.4, 0.5)));
        for _ in 0..20 {
            particles.update(&chunks, 0.05);
        }
        assert!(
            particles.live.iter().all(|p| p.position.y > 9.0),
            "something fell through the world"
        );
    }

    #[test]
    fn the_pool_is_bounded_however_hard_it_is_pushed() {
        // The one failure mode a particle system has.
        let chunks = ChunkManager::new(4);
        let mut particles = Particles::new();
        for _ in 0..500 {
            particles.block_broken((0, 30, 0), BLOCK_STONE);
        }
        assert!(particles.len() <= MAX, "{} particles", particles.len());
        // ...and it still steps in a sensible time, which is what the
        // cap is protecting.
        particles.update(&chunks, 0.016);
    }

    #[test]
    fn a_broken_block_throws_chips_of_itself() {
        let mut particles = Particles::new();
        particles.block_broken((3, 30, 4), BLOCK_STONE);
        assert_eq!(particles.len(), CHIPS);
        assert!(particles
            .live
            .iter()
            .all(|p| p.look == Look::Chip(BLOCK_STONE)));
        // Thrown outward and upward: a burst that fell straight down
        // reads as the block sinking rather than as it coming apart.
        assert!(particles.live.iter().any(|p| p.velocity.y > 1.0));
    }

    #[test]
    fn a_blow_throws_a_short_burst_of_blood() {
        let mut particles = Particles::new();
        particles.blood(Vec3::new(4.0, 31.0, -2.0));
        assert_eq!(particles.len(), BLOOD_DROPS);
        assert!(particles.live.iter().all(|p| p.look == Look::Blood));
        // Short: a burst still on screen a second later is a burst that
        // is still there when the next one lands, and two of these on
        // top of each other is a puddle in mid-air.
        assert!(
            particles.live.iter().all(|p| p.total < 1.0),
            "a drop of blood outlives its own blow"
        );
        // Thrown outward and upward rather than dropped: a burst that
        // fell straight down reads as the animal leaking rather than as
        // it being struck.
        assert!(particles.live.iter().any(|p| p.velocity.y > 0.5));
    }

    /// A blow leaves marks **on the ground**, and the whole of that
    /// mechanism is the drops it already threw.
    ///
    /// The player asked for it in the plainest possible terms: "упал --
    /// оставил следы крови на земле, которые потом пройдут". What was
    /// there was a spray that arced through the air and evaporated a
    /// foot above the floor, because a drop lived less than the fall
    /// took -- so this measures the two halves together: the drops land
    /// at all, and landing is what makes a mark.
    #[test]
    fn a_blow_leaves_marks_on_the_ground_that_fade_and_go() {
        let chunks = world_with_floor();
        let mut particles = Particles::new();
        // Chest height over the floor, which stands at y=10.
        particles.blood(Vec3::new(4.0, 11.3, 4.0));
        for _ in 0..120 {
            particles.update(&chunks, 1.0 / 60.0);
        }
        let marks: Vec<Particle> = particles
            .live
            .iter()
            .copied()
            .filter(|p| p.look == Look::Stain)
            .collect();
        assert!(
            !marks.is_empty(),
            "a burst of blood fell to the floor and left nothing on it"
        );
        assert!(
            particles.live.iter().all(|p| p.look != Look::Blood),
            "a drop of blood is still in the air two seconds after the blow"
        );
        for mark in &marks {
            // On the floor's own top face, lifted clear of it. Not
            // inside it, and not hanging where the drop happened to
            // stop.
            assert!(
                (mark.position.y - f64::from(10.0 + STAIN_LIFT)).abs() < 1e-4,
                "a mark landed at y={} where the floor is at 10",
                mark.position.y,
            );
            assert!(mark.velocity == Vec3::ZERO, "a mark on the ground is moving");
            assert!(mark.gravity == 0.0 && !mark.collides);
            assert!(
                mark.total >= STAIN_LIFE.0 && mark.total <= STAIN_LIFE.1,
                "a mark lives {} seconds",
                mark.total,
            );
        }
        // ...and it goes on its own. Half a minute is not "for ever",
        // which is the other half of what was asked for.
        for _ in 0..(STAIN_LIFE.1 as usize * 60 + 120) {
            particles.update(&chunks, 1.0 / 60.0);
        }
        assert!(
            particles.is_empty(),
            "{} particles outlived the longest stain",
            particles.len(),
        );
    }

    /// A mark **lies on the ground** rather than standing up out of it,
    /// and it is the same mark whichever way the player is facing.
    ///
    /// Every other particle in this pool is built in the camera's own
    /// axes, because a speck seen edge-on is a speck that is not there.
    /// A stain built that way is a red disc standing upright in the
    /// grass that spins to follow the player round it -- which is the
    /// one thing a mark on the floor must never do, and it is invisible
    /// from the seat it was drawn at. So the quad is built in world
    /// axes, and this checks it from two cameras a quarter turn apart.
    #[test]
    fn a_mark_on_the_ground_lies_flat_whichever_way_the_player_faces() {
        let chunks = world_with_floor();
        let layers = FaceLayers::empty_for_test();
        let mut particles = Particles::new();
        particles.blood(Vec3::new(4.0, 11.3, 4.0));
        for _ in 0..120 {
            particles.update(&chunks, 1.0 / 60.0);
        }
        assert!(particles.live.iter().any(|p| p.look == Look::Stain));

        let corners = |right: Vec3, up: Vec3| {
            let (mut vertices, mut indices) = (Vec::new(), Vec::new());
            particles.build_into(Vec3::ZERO, right, up, &layers, &LightMap::new(), &mut vertices, &mut indices);
            vertices
                .iter()
                .map(|v| Vec3::from_array(v.position))
                .collect::<Vec<_>>()
        };
        let facing_x = corners(Vec3::Z, Vec3::Y);
        let facing_z = corners(-Vec3::X, Vec3::Y);
        assert_eq!(facing_x.len(), facing_z.len());
        for (a, b) in facing_x.iter().zip(facing_z.iter()) {
            assert!(
                (*a - *b).length() < 1e-6,
                "a mark turned with the camera: {a} from one seat, {b} from the other",
            );
        }
        // Flat: every quad is level, and its normal is the vertical.
        for quad in facing_x.chunks_exact(4) {
            let normal = (quad[1] - quad[0]).cross(quad[2] - quad[1]).normalize();
            assert!(
                normal.y.abs() > 0.999,
                "a mark is standing up out of the floor: its normal is {normal}",
            );
            for corner in quad {
                assert!(
                    (corner.y - quad[0].y).abs() < 1e-6,
                    "a corner of a mark is off the floor's plane",
                );
                // Above the ground it is drawn on, and by less than a
                // texel: below it and the terrain hides it, further and
                // it is a sticker floating over the grass.
                assert!(
                    corner.y > 10.0 && corner.y < 10.0 + 1.0 / 16.0,
                    "a mark sits at y={}, and the floor is at 10",
                    corner.y,
                );
            }
        }
    }

    /// A drop that hits a **wall** leaves nothing hanging over it.
    ///
    /// `solid_at` -- what stops a drop -- answers "is this point inside
    /// something", which is not enough to lay a decal: a drop that hit
    /// the side of a boulder at knee height would put its mark on the
    /// boulder's roof, a metre up in the air, and one that hit a ceiling
    /// would put it there too. See `landing_face`, which is what asks
    /// the other question.
    #[test]
    fn a_drop_that_hits_a_wall_leaves_no_mark_hanging_in_the_air() {
        let chunks = world_with_floor();
        // Down through the floor's top face: a landing.
        assert!(landing_face(&chunks, Vec3::new(4.0, 10.05, 4.0), Vec3::new(4.0, 9.95, 4.0))
            .is_some());
        // Sideways into the same block, well below its top: a wall.
        assert!(landing_face(&chunks, Vec3::new(4.0, 9.5, 4.0), Vec3::new(4.2, 9.49, 4.0))
            .is_none());
        // Upward into it: a ceiling.
        assert!(landing_face(&chunks, Vec3::new(4.0, 9.4, 4.0), Vec3::new(4.0, 9.6, 4.0))
            .is_none());
        // ...and terrain that has not arrived is not a floor. A mark on
        // an unloaded chunk is a mark in mid-air, the same fault the
        // rain's splashes were fixed for.
        assert!(landing_face(&chunks, Vec3::new(900.0, 10.05, 900.0), Vec3::new(900.0, 9.95, 900.0))
            .is_none());
    }

    /// A mark never hangs over the edge of what it landed on.
    ///
    /// **Photographed on the plaza of the test world**: a drop that
    /// landed a few centimetres from the edge of the block the player
    /// stood on left a disc with a third of itself out past the edge,
    /// floating over the grass a metre below. The pass cannot clip one
    /// surface to another, so the mark is nudged in far enough to fit
    /// -- see `Particles::stain`. Checked at the worst case, which is a
    /// turned square: its corners reach half a width by root two.
    #[test]
    fn a_mark_never_hangs_over_the_edge_of_what_it_landed_on() {
        let chunks = world_with_floor();
        let layers = FaceLayers::empty_for_test();
        let mut particles = Particles::new();
        // Blows all over one block, so plenty of drops land near its
        // edges: it is the near-edge landing that used to overhang.
        for step in 0..40 {
            let along = step as f32 / 40.0;
            particles.blood(Vec3::new(4.0 + along, 11.3, 4.0 + along));
            for _ in 0..90 {
                particles.update(&chunks, 1.0 / 60.0);
            }
        }
        let (mut vertices, mut indices) = (Vec::new(), Vec::new());
        particles.build_into(Vec3::ZERO, Vec3::X, Vec3::Y, &layers, &LightMap::new(), &mut vertices, &mut indices);
        let mut marks = 0;
        for (particle, quad) in particles
            .live
            .iter()
            .zip(vertices.chunks_exact(4))
            .filter(|(particle, _)| particle.look == Look::Stain)
        {
            marks += 1;
            let cell = (particle.position.x.floor(), particle.position.z.floor());
            for corner in quad {
                let [x, _, z] = corner.position;
                assert!(
                    x >= (cell.0 - 1e-4) as f32 && x <= (cell.0 + 1.0 + 1e-4) as f32,
                    "a mark on the block at {cell:?} reaches out to x={x}",
                );
                assert!(
                    z >= (cell.1 - 1e-4) as f32 && z <= (cell.1 + 1.0 + 1e-4) as f32,
                    "a mark on the block at {cell:?} reaches out to z={z}",
                );
            }
        }
        assert!(marks > 0, "forty blows left no mark to measure");
    }

    /// Nothing is marked that is not a floor.
    ///
    /// **Photographed on the plaza**: a drying rack is a whole cell to
    /// every question the particles ask -- a drop stops on it, a player
    /// walks round it -- and what it actually is, is four poles with
    /// daylight between them. A mark laid on the top of its cell was a
    /// disc bridging the gap with grass a metre below it. See
    /// `landing_face`.
    #[test]
    fn a_frame_of_poles_is_not_a_floor_to_be_marked() {
        use primitive_shared::types::BLOCK_DRYING_RACK;

        let racks = crate::logic::physics::tests::world_of(|y| {
            if y < 10 {
                BLOCK_DRYING_RACK
            } else {
                BLOCK_AIR
            }
        });
        assert!(
            landing_face(&racks, Vec3::new(4.0, 10.05, 4.0), Vec3::new(4.0, 9.95, 4.0)).is_none(),
            "a mark was laid across the top of a rack",
        );
        // ...and the ordinary floor beside it still takes one, so this
        // is a rule about frames rather than a rule that stopped
        // everything.
        let stone = world_with_floor();
        assert!(
            landing_face(&stone, Vec3::new(4.0, 10.05, 4.0), Vec3::new(4.0, 9.95, 4.0)).is_some(),
        );
    }

    /// The floor of a long fight cannot fill up with marks.
    ///
    /// A stain outlives everything else here by a factor of fifty, so
    /// it is the one look that accumulates: a pack of wolves on a
    /// player is a blow a second for a minute, and without a ceiling of
    /// its own the ground would hold the whole pool and the rain would
    /// stop. At the ceiling the faintest mark is recycled rather than
    /// the new one refused -- a blow that leaves no mark is the fault
    /// this exists to fix.
    #[test]
    fn the_ground_holds_only_so_many_marks() {
        let chunks = world_with_floor();
        let mut particles = Particles::new();
        for _ in 0..80 {
            particles.blood(Vec3::new(4.0, 11.3, 4.0));
            for _ in 0..90 {
                particles.update(&chunks, 1.0 / 60.0);
            }
        }
        let marks = particles
            .live
            .iter()
            .filter(|p| p.look == Look::Stain)
            .count();
        assert!(marks > 0, "eighty blows left no mark at all");
        assert!(
            marks <= STAIN_MAX,
            "the ground is holding {marks} marks against a budget of {STAIN_MAX}",
        );
        assert!(particles.len() <= MAX);
    }

    /// A mark is dark red, holds its colour, and then dries out.
    #[test]
    fn a_mark_holds_its_colour_and_then_dries_out() {
        for shade in 0..=255u8 {
            let [r, g, b, a] = Look::Stain.tint(1.0, 1.0, shade);
            assert!(r > g * 3.0 && r > b * 3.0, "shade {shade} is {r},{g},{b}");
            // Darker than the spray that made it -- blood soaked into
            // the ground rather than lying on top of it -- and never
            // opaque, so the grain of the floor reads through it.
            let [drop, _, _, _] = Look::Blood.tint(1.0, 1.0, shade);
            assert!(r < drop, "a mark is no darker than the drop that made it");
            assert!(a < 1.0 && a > 0.5, "a mark is {a} opaque");
        }
        assert_eq!(Look::Stain.tint(0.0, 0.0, 0)[3], 0.0, "a mark never goes");
    }

    #[test]
    fn a_long_fight_cannot_crowd_out_the_weather() {
        // The cap is shared, and blood is the one thing here that is
        // emitted in bursts on an event a player controls.
        let chunks = ChunkManager::new(4);
        let mut particles = Particles::new();
        for _ in 0..500 {
            particles.blood(Vec3::new(0.0, 30.0, 0.0));
        }
        assert!(particles.len() <= MAX, "{} particles", particles.len());
        particles.update(&chunks, 0.016);
    }

    #[test]
    fn blood_is_red_whatever_shade_of_it_a_drop_drew() {
        // It is a *tint on a snowflake*: the picture is very nearly
        // white, so the whole of the colour is in these numbers, and a
        // palette that let the green or the blue through would be a
        // spray of pink or mauve rather than blood.
        for shade in 0..=255u8 {
            let [r, g, b, a] = Look::Blood.tint(1.0, 1.0, shade);
            assert!(r > 0.15, "shade {shade} came out at {r} red, which is black");
            assert!(r > g * 3.0 && r > b * 3.0, "shade {shade} is {r},{g},{b}");
            assert_eq!(a, 1.0, "shade {shade} is see-through before it fades");
        }
        // ...and it is gone at the end of a life rather than blinking
        // off at full strength.
        assert_eq!(Look::Blood.tint(0.0, 0.0, 0)[3], 0.0);
    }

    #[test]
    fn weather_stops_when_the_sky_clears() {
        let mut particles = Particles::new();
        particles.weather(0.0, false, Vec3::ZERO, Vec3::ZERO, 0.1);
        assert!(particles.is_empty(), "it rained in clear weather");
        particles.weather(1.0, false, Vec3::ZERO, Vec3::ZERO, 0.1);
        assert!(!particles.is_empty(), "a storm produced nothing");
    }

    /// A stone floor with a lit campfire on it at (4, 10, 4), and the
    /// player standing beside it.
    fn world_with_a_fire() -> (ChunkManager, Vec3) {
        let chunks = crate::logic::physics::tests::floor_with(
            4,
            4,
            primitive_shared::types::BLOCK_CAMPFIRE_LIT,
        );
        (chunks, Vec3::new(4.5, 11.0, 6.5))
    }

    /// Runs `fires` for `seconds` at `fps` and hands back what is alive.
    fn burn(particles: &mut Particles, chunks: &ChunkManager, at: Vec3, seconds: f32, fps: f32) {
        let dt = 1.0 / fps;
        for _ in 0..(seconds * fps) as usize {
            particles.fires(chunks, at, dt);
            particles.update(chunks, dt);
        }
    }

    fn count(particles: &Particles, look: Look) -> usize {
        particles.live.iter().filter(|p| p.look == look).count()
    }

    /// **A rapid is white and a lazy reach is not.** Water at the river's
    /// level running faster than a swimmer throws foam within a second, and
    /// the foam goes the way the water goes; the same water running at a
    /// walking pace throws none, and neither does ground with no water on it.
    #[test]
    fn a_rapid_throws_foam_downstream_and_a_lazy_reach_throws_none() {
        use primitive_shared::worldgen::SEA_LEVEL;
        let river = crate::logic::physics::tests::world_of(|y| match y as i32 {
            y if y < SEA_LEVEL - 3 => BLOCK_STONE,
            y if y < SEA_LEVEL => primitive_shared::types::BLOCK_WATER,
            _ => BLOCK_AIR,
        });
        let at = Vec3::new(8.0, SEA_LEVEL as f32, 8.0);
        let run = |speed: f32, chunks: &ChunkManager| {
            let mut particles = Particles::new();
            for _ in 0..60 {
                particles.rapids(chunks, at, |_, _, _| (speed, 0.0), 1.0 / 60.0);
                particles.update(chunks, 1.0 / 60.0);
            }
            particles
        };
        let rapid = run(3.0, &river);
        assert!(count(&rapid, Look::Foam) > 10, "a second over a rapid threw {} flecks of foam", count(&rapid, Look::Foam));
        let downstream = rapid.live.iter().filter(|p| p.look == Look::Foam && p.velocity.x > 0.0).count();
        assert!(downstream * 10 >= count(&rapid, Look::Foam) * 9, "the foam is not going the way the river goes");
        assert_eq!(count(&run(0.8, &river), Look::Foam), 0, "a lazy reach threw foam");
        let dry = crate::logic::physics::tests::floor_world();
        assert_eq!(count(&run(3.0, &dry), Look::Foam), 0, "dry ground threw foam");
    }

    #[test]
    fn a_lit_fire_is_remembered_rather_than_stumbled_upon_again() {
        // **The bug this system had for its whole life.** Emission used
        // to happen only on the frames whose random sample landed on
        // the fire's own cell -- one cell in thousands -- so
        // `EMBERS_PER_SECOND = 6.0` was about one spark every twenty
        // seconds and a campfire sat there inert. The cell is kept now,
        // and the constants mean what they say.
        let (chunks, at) = world_with_a_fire();
        let mut particles = Particles::new();
        burn(&mut particles, &chunks, at, 4.0, 60.0);
        assert_eq!(
            particles.hearths.iter().map(|h| h.cell).collect::<Vec<_>>(),
            vec![(4, 10, 4)],
            "four seconds beside a campfire and it was never found"
        );
        assert!(
            count(&particles, Look::Smoke) >= 4,
            "a lit fire produced almost no smoke: {}",
            count(&particles, Look::Smoke)
        );
        assert!(
            count(&particles, Look::Ember) >= 1,
            "a lit fire threw no sparks at all"
        );
    }

    #[test]
    fn a_fire_sparks_at_the_rate_the_constant_says() {
        // **The bug a countdown replaced.** Emission used to be a coin
        // flipped once a frame against `rate * dt`, which asks a
        // three-shift generator for a rare event at a fixed stride --
        // and it obliged: four seconds beside a campfire produced *no*
        // sparks where twelve were due, and half the smoke that was
        // asked for. Nothing about the code said so; the constants read
        // correctly and the fire simply sat there.
        //
        // Counted as emissions rather than as survivors, because how
        // many are alive also depends on how long they live.
        let (chunks, at) = world_with_a_fire();
        let mut particles = Particles::new();
        let (mut sparks, mut puffs) = (0usize, 0usize);
        let seconds = 20.0;
        let dt = 1.0 / 60.0;
        for _ in 0..(seconds / dt) as usize {
            let before = (count(&particles, Look::Ember), count(&particles, Look::Smoke));
            particles.fires(&chunks, at, dt);
            sparks += count(&particles, Look::Ember).saturating_sub(before.0);
            puffs += count(&particles, Look::Smoke).saturating_sub(before.1);
            particles.update(&chunks, dt);
        }
        let expected_sparks = (EMBERS_PER_SECOND * seconds) as usize;
        // The fire is two blocks from the player, so the smoke rate is
        // the full one.
        let expected_puffs = (SMOKE_PER_SECOND * seconds) as usize;
        assert!(
            sparks * 4 > expected_sparks * 3 && sparks < expected_sparks * 5 / 4,
            "{sparks} sparks in {seconds}s where {expected_sparks} were asked for"
        );
        assert!(
            puffs * 4 > expected_puffs * 3 && puffs < expected_puffs * 5 / 4,
            "{puffs} puffs in {seconds}s where {expected_puffs} were asked for"
        );
    }

    #[test]
    fn a_fire_that_goes_out_stops_smoking() {
        // The other half of remembering: a cell held after the fire in
        // it is gone would be a plume rising out of a cold ring of
        // stones, which is worse than no plume at all.
        let (chunks, at) = world_with_a_fire();
        let mut particles = Particles::new();
        burn(&mut particles, &chunks, at, 2.0, 60.0);
        assert!(!particles.hearths.is_empty(), "the fire was never found");

        let cold = crate::logic::physics::tests::floor_with(
            4,
            4,
            primitive_shared::types::BLOCK_CAMPFIRE,
        );
        particles.fires(&cold, at, 1.0 / 60.0);
        assert!(
            particles.hearths.is_empty(),
            "a fire that went out is still smoking"
        );
    }

    #[test]
    fn a_plume_is_the_same_thickness_at_twenty_frames_a_second_as_at_sixty() {
        // Emission is a rate scaled by the frame, and a phone is where
        // that matters: the same fire on a slow device must not smoke a
        // third as much. Compared as a count of what is alive after the
        // same *time*, which is the thing an eye judges.
        let (chunks, at) = world_with_a_fire();
        let mut fast = Particles::new();
        burn(&mut fast, &chunks, at, 6.0, 60.0);
        let mut slow = Particles::new();
        burn(&mut slow, &chunks, at, 6.0, 20.0);
        let (fast, slow) = (count(&fast, Look::Smoke), count(&slow, Look::Smoke));
        assert!(
            (fast as i32 - slow as i32).abs() <= 4,
            "{fast} puffs at 60 fps against {slow} at 20"
        );
    }

    #[test]
    fn smoke_starts_above_the_flame_and_never_covers_the_fire() {
        // Two complaints in one: a puff born inside the cell is a grey
        // blob over the fire a player is stood at feeding -- and the
        // fire is a screen you open by looking at it -- and a puff that
        // was ever opaque would be a hole in the world rather than
        // smoke. `mesh::FLAME_TOP` is 0.98 of the cell.
        let (chunks, at) = world_with_a_fire();
        let mut particles = Particles::new();
        burn(&mut particles, &chunks, at, 4.0, 60.0);
        let born_low = particles
            .live
            .iter()
            .filter(|p| p.look == Look::Smoke)
            .any(|p| p.position.y < 11.0);
        assert!(!born_low, "a puff of smoke was born inside the fire");
        for left in 0..=10 {
            let alpha = Look::Smoke.tint(1.0, left as f32 / 10.0, 0)[3];
            assert!(alpha <= 0.35, "smoke at {alpha} opacity is a wall");
        }
        assert_eq!(Look::Smoke.tint(0.0, 0.0, 0)[3], 0.0, "smoke never clears");
    }

    #[test]
    fn no_puff_of_smoke_is_ever_inside_the_block_over_a_fire() {
        // "If a block stands above a campfire it turns black": the puffs
        // rose through it, lit by the nought inside it. A block straight
        // over the fire refuses the smoke; a ceiling three up lets it
        // rise and die under it. Every frame, not only at the end.
        use primitive_shared::types::BLOCK_PLANKS;
        for gap in [1, 3] {
            let (mut chunks, at) = world_with_a_fire();
            let pos = primitive_shared::types::ChunkPos::new(0, 0);
            let mut chunk = chunks.get(pos).unwrap().clone();
            chunk.set(4, 10 + gap, 4, BLOCK_PLANKS);
            chunks.insert(chunk);
            let mut particles = Particles::new();
            let mut seen = 0;
            for _ in 0..(8.0 * 60.0) as usize {
                particles.fires(&chunks, at, 1.0 / 60.0);
                particles.update(&chunks, 1.0 / 60.0);
                for puff in particles.live.iter().filter(|p| p.look == Look::Smoke && p.life > 0.0) {
                    seen += 1;
                    let top = puff.position.y + f64::from(puff.size * (1.0 + SMOKE_GROWTH));
                    assert!(
                        top < f64::from((10 + gap) as f32),
                        "a puff reached {top} under a block at {} over the fire",
                        10 + gap
                    );
                }
            }
            if gap == 1 {
                assert_eq!(seen, 0, "a fire with a block on it still smoked into the block");
            } else {
                assert!(seen > 0, "a fire with a ceiling three blocks up gave no smoke at all");
            }
        }
    }

    #[test]
    fn a_camp_of_fires_cannot_fill_the_pool_with_smoke() {
        // Smoke is the one look here that lives for seconds, so it is
        // the one that can accumulate -- and the cap is what keeps a
        // phone out of trouble. Eight fires in a row, run long enough
        // that every puff of the first second has been replaced twice.
        let mut chunks = crate::logic::physics::tests::floor_world();
        for x in 0..8 {
            let mut chunk = chunks
                .get(primitive_shared::types::ChunkPos::new(0, 0))
                .unwrap()
                .clone();
            chunk.set(x * 2, 10, 8, primitive_shared::types::BLOCK_CAMPFIRE_LIT);
            chunks.insert(chunk);
        }
        let mut particles = Particles::new();
        burn(&mut particles, &chunks, Vec3::new(8.0, 11.0, 8.0), 20.0, 60.0);
        assert!(
            count(&particles, Look::Smoke) <= SMOKE_MAX,
            "{} puffs against a cap of {SMOKE_MAX}",
            count(&particles, Look::Smoke)
        );
        assert!(particles.len() <= MAX);
    }

    #[test]
    fn every_look_draws_something() {
        // A particle that draws no geometry is a particle that is not
        // there, which is the one bug this system can have that nothing
        // else would notice.
        let layers = FaceLayers::empty_for_test();
        for look in [
            Look::Rain,
            Look::Snow,
            Look::Splash,
            Look::Chip(BLOCK_STONE),
            Look::Chip(BLOCK_AIR),
            Look::Ember,
            Look::Blood,
            Look::Smoke,
            Look::Stain,
            Look::Cloud,
        ] {
            let mut particles = Particles::new();
            particles.emit(Particle {
                position: (Vec3::new(1.0, 2.0, 3.0)).as_dvec3(),
                velocity: Vec3::new(0.0, -8.0, 0.0),
                life: 1.0,
                total: 1.0,
                size: 0.06,
                look,
                texel: (3, 5),
                gravity: 0.0,
                drag: 1.0,
                collides: false,
            });
            let (mut vertices, mut indices) = (Vec::new(), Vec::new());
            particles.build_into(Vec3::ZERO, Vec3::X, Vec3::Y, &layers, &LightMap::new(), &mut vertices, &mut indices);
            assert_eq!(vertices.len(), 4, "{look:?} drew no quad");
            assert_eq!(indices.len(), 12, "{look:?} is missing a winding");
        }
    }

    /// A drop of blood is drawn in front of the wolf it came off, not in
    /// the sky above it.
    ///
    /// **Reported by a player as blood and sparks high in the sky with
    /// nothing there to bleed.** The particles were uploaded in world
    /// coordinates while `view_proj` had been rebuilt around the frame's
    /// render origin, which from the first frame of a world is the
    /// player's own floored position -- so every particle was drawn that
    /// position further on: the player's altitude straight up, and their
    /// x and z across. Every check that looked at the particles
    /// themselves passed, because their positions were right; only the
    /// space they were handed to the card in was wrong.
    ///
    /// So this goes through the matrix the renderer actually builds
    /// (`Camera::view_proj_about(render_origin)`, with `camera_pos`
    /// relative to the same origin) and compares where the drop lands on
    /// the screen with where the blow was in the world. The player is put
    /// well away from the middle of the map, where the old fault drew
    /// the drop hundreds of blocks off, and the settings are the player's
    /// own (fov 95).
    #[test]
    fn a_drop_of_blood_is_drawn_where_the_blow_landed_wherever_the_render_origin_stands() {
        use crate::engine::camera::Camera;

        let layers = FaceLayers::empty_for_test();
        let mut camera = Camera::new((Vec3::new(301.5, 72.6, -455.5)).as_dvec3(), 16.0 / 9.0);
        camera.fov_y_radians = 95f32.to_radians();
        camera.yaw = -std::f32::consts::FRAC_PI_2;
        camera.pitch = -0.2;
        // What `render_origin_for` makes of a player this far from zero:
        // their own position, floored.
        let origin = camera.position.floor();
        // A wolf's chest two blocks ahead and a little below the eye.
        let struck = camera.position.as_vec3() + Vec3::new(0.0, -0.6, -2.0);

        let mut particles = Particles::new();
        particles.blood(struck);
        let (mut vertices, mut indices) = (Vec::new(), Vec::new());
        let (right, up) = billboard_axes(&camera);
        particles.build_into(
            origin.as_vec3(),
            right,
            up,
            &layers,
            &LightMap::new(),
            &mut vertices,
            &mut indices,
        );
        assert_eq!(vertices.len(), BLOOD_DROPS * 4);

        // The globals exactly as `Renderer::render` fills them.
        let view_proj = camera.view_proj_about(origin.as_vec3());
        let camera_pos = camera.position - origin;
        // ...and the world as a camera with no origin at all sees it,
        // which at a few hundred blocks out an `f32` still holds to well
        // under a pixel.
        let truth = camera.view_proj_about(Vec3::ZERO) * struck.extend(1.0);
        let truth = truth.truncate() / truth.w;

        for quad in vertices.chunks_exact(4) {
            // A drop just born is at full size and its quad is centred
            // on it, so the mean of the corners is the drop.
            let centre = quad
                .iter()
                .map(|v| Vec3::from_array(v.position))
                .sum::<Vec3>()
                / 4.0;
            // `particles.wgsl`, `vs_main`: one multiply.
            let clip = view_proj * centre.extend(1.0);
            assert!(clip.w > 0.0, "a drop of blood is behind the player");
            let ndc = clip.truncate() / clip.w;
            assert!(
                ndc.x.abs() < 1.0 && ndc.y.abs() < 1.0,
                "a drop of blood struck in front of the player is off the screen at {ndc}",
            );
            assert!(
                (ndc - truth).truncate().length() < 1e-3,
                "a drop of blood is drawn at {ndc} on the screen, and the blow landed at {truth}",
            );
            // The fog is measured in the same space, as `view_distance`.
            let seen_at = (centre - camera_pos.as_vec3()).length();
            let really = (struck - camera.position.as_vec3()).length();
            assert!(
                (seen_at - really).abs() < 1e-3,
                "the fog puts a drop {really} blocks away at {seen_at}",
            );
        }
    }

    /// Nothing outlives its own life, and nothing climbs further than it
    /// was thrown -- through hitches as well as smooth frames.
    ///
    /// Written while hunting the blood in the sky, to rule out the
    /// integration as the cause: a long frame launching a drop, a
    /// buoyant spark with no ceiling, a stain that forgot to die. None
    /// of them was it (see the test above for what was), and this keeps
    /// them from becoming it. Every emitter in the pool runs together
    /// over a stone floor at y=10 with a fire on it, frames alternate
    /// between a sixtieth of a second and stalls of half a second and
    /// three seconds, and the ceilings are what each look's own launch
    /// numbers allow -- with room to spare, and far below the seventy
    /// blocks the reported fault drew blood at.
    #[test]
    fn no_particle_outlives_its_life_or_climbs_higher_than_it_was_thrown() {
        let (chunks, at) = world_with_a_fire();
        let mut particles = Particles::new();
        let frames = [1.0 / 60.0, 1.0 / 60.0, 0.5, 1.0 / 144.0, 3.0, 1.0 / 30.0];
        // The highest anything of each look can honestly be. Blood is
        // thrown from 11.3 at 2.6 up against 14 down; chips leave the
        // cell above the floor at 4.5 against 16; a spark starts at
        // 10.45 and rises for at most 1.4 s; smoke starts at 11.06 and
        // rises for at most 3.6 s; weather starts `CEILING` over the
        // player and only falls.
        let ceiling = |look: Look| match look {
            Look::Blood => 11.3 + 1.0,
            // A drop may come down on the fire's own cell rather than
            // on the floor, so the top of a whole block standing on it.
            Look::Stain => 11.0 + STAIN_LIFT + 0.5,
            Look::Chip(_) => 12.0 + 1.5,
            Look::Ember => 10.45 + 1.8 * 1.4 + 0.6 * 1.4 * 1.4 + 1.0,
            Look::Smoke => 11.06 + 0.16 + 1.2 * 3.6 + 0.225 * 3.6 * 3.6 + 1.0,
            Look::Rain | Look::Snow | Look::Splash => at.y + CEILING + 1.0,
            // There is no water in this world; a cloud would be where a drop
            // went in, which is no higher than the drop.
            Look::Cloud => 11.3 + 1.0,
            // There is no river in this world either.
            Look::Foam => at.y + 1.0,
        };
        for frame in 0..1_200 {
            let dt = frames[frame % frames.len()];
            if frame % 20 == 0 {
                particles.blood(Vec3::new(4.5, 11.3, 6.5));
                particles.block_broken((6, 10, 6), BLOCK_STONE);
            }
            particles.weather(0.3, frame % 400 > 200, at, Vec3::new(2.5, 0.0, 0.0), dt);
            particles.fires(&chunks, at, dt);
            particles.update(&chunks, dt);
            for particle in &particles.live {
                assert!(
                    particle.position.is_finite() && particle.velocity.is_finite(),
                    "a {:?} has gone to {} after a {dt}s frame",
                    particle.look,
                    particle.position,
                );
                assert!(
                    particle.position.y <= f64::from(ceiling(particle.look)),
                    "a {:?} climbed to y={} after a {dt}s frame",
                    particle.look,
                    particle.position.y,
                );
                assert!(
                    particle.life <= particle.total,
                    "a {:?} has more life left than it started with",
                    particle.look,
                );
            }
        }
        // ...and once nothing is emitting, everything goes -- stains
        // last, and they are the longest-lived thing in the pool.
        for _ in 0..((STAIN_LIFE.1 + 1.0) * 60.0) as usize {
            particles.update(&chunks, 1.0 / 60.0);
        }
        assert!(
            particles.is_empty(),
            "{} particles outlived the longest life anything is given",
            particles.len(),
        );
    }

    /// **Every kind of particle is drawn on the pixel of the point it is at,
    /// and faces the camera it is drawn for** -- through the matrix the frame
    /// builds, from render origins that are not the camera's block, with the
    /// axes the frame passes (`billboard_axes`).
    ///
    /// The truth each one is measured against is the terrain's: a block face at
    /// that point, as `GraphicsState::set_chunk_mesh` hands one to the card --
    /// the vertex measured from its chunk's corner, the instance offset from
    /// that corner to the origin. That is the pixel the ground a drop lands on
    /// is drawn at, so a particle that disagrees with it is a drop beside its
    /// own stain. Every emitter the pool has, a drip as well as a burst, and
    /// every look, from four cameras including one looking all but straight
    /// down. Every quad but a mark on the floor and a streak of rain is checked
    /// square to the view: with the world's up for an axis, a chip broken at the
    /// player's feet was a hairline seen edge-on.
    #[test]
    fn every_particle_is_drawn_on_its_own_pixel_and_faces_the_camera_wherever_the_origin_stands() {
        use crate::engine::camera::Camera;
        use primitive_shared::types::CHUNK_SIZE_X;

        let layers = FaceLayers::empty_for_test();
        let light = LightMap::new();
        let eye = Vec3::new(301.5, 72.6, -455.5);
        let (width, height) = (1280.0f32, 720.0f32);
        let pixel = |clip: glam::Vec4| {
            glam::Vec2::new((clip.x / clip.w * 0.5 + 0.5) * width, (0.5 - clip.y / clip.w * 0.5) * height)
        };
        let origins = [
            eye.floor(),
            eye.floor() + Vec3::new(-63.0, 20.0, 41.0),
            Vec3::new(256.0, 64.0, -512.0),
            Vec3::ZERO,
        ];
        let cameras = [
            (-std::f32::consts::FRAC_PI_2, -0.2),
            (0.7, -1.3),
            (2.5, 0.9),
            (4.0, -1.55),
        ];
        let mut checked = 0;
        for origin in origins {
            for (yaw, pitch) in cameras {
                let mut camera = Camera::new(eye.as_dvec3(), width / height);
                camera.fov_y_radians = 95f32.to_radians();
                camera.yaw = yaw;
                camera.pitch = pitch;
                let ahead = eye + camera.forward() * 3.0;

                let mut particles = Particles::new();
                particles.spray(ahead, BLOOD_DROPS);
                particles.spray(ahead, 1);
                particles.block_broken(
                    (ahead.x.floor() as i32, ahead.y.floor() as i32, ahead.z.floor() as i32),
                    BLOCK_STONE,
                );
                particles.ember(ahead);
                particles.smoke_under(ahead - Vec3::Y * SMOKE_BIRTH_HEIGHT, f32::INFINITY);
                particles.weather(1.0, false, ahead - Vec3::Y * CEILING, Vec3::ZERO, 0.1);
                particles.weather(1.0, true, ahead - Vec3::Y * CEILING, Vec3::ZERO, 0.1);
                for look in [Look::Splash, Look::Stain, Look::Cloud] {
                    particles.emit(Particle {
                        position: ahead.as_dvec3(),
                        velocity: Vec3::new(0.0, -3.0, 0.0),
                        life: 1.0,
                        total: 1.0,
                        size: 0.06,
                        look,
                        texel: (1, 40),
                        gravity: 0.0,
                        drag: 1.0,
                        collides: false,
                    });
                }

                let (right, up) = billboard_axes(&camera);
                let (mut vertices, mut indices) = (Vec::new(), Vec::new());
                particles.build_into(origin, right, up, &layers, &light, &mut vertices, &mut indices);
                assert_eq!(vertices.len(), particles.len() * 4);
                let view_proj = camera.view_proj_about(origin);

                for (particle, quad) in particles.live.iter().zip(vertices.chunks_exact(4)) {
                    let chunk = CHUNK_SIZE_X as f32;
                    let corner = (particle.position.as_vec3() / chunk).floor() * chunk;
                    let truth = view_proj * ((particle.position.as_vec3() - corner) + (corner - origin)).extend(1.0);
                    if truth.w <= 0.05 {
                        continue;
                    }
                    let corners: Vec<Vec3> = quad.iter().map(|v| Vec3::from_array(v.position)).collect();
                    let centre = corners.iter().copied().sum::<Vec3>() / 4.0;
                    let off = (pixel(view_proj * centre.extend(1.0)) - pixel(truth)).length();
                    assert!(
                        off < 0.5,
                        "a {:?} at {} is drawn {off} pixels from its place (origin {origin}, yaw {yaw}, pitch {pitch})",
                        particle.look,
                        particle.position,
                    );
                    if !matches!(particle.look, Look::Stain | Look::Rain) {
                        let normal = (corners[1] - corners[0]).cross(corners[2] - corners[1]).normalize_or_zero();
                        let facing = normal.dot(camera.forward()).abs();
                        assert!(
                            facing > 0.999,
                            "a {:?} is turned {:.0} degrees away from a camera at pitch {pitch}",
                            particle.look,
                            facing.clamp(-1.0, 1.0).acos().to_degrees(),
                        );
                    }
                    checked += 1;
                }
            }
        }
        assert!(checked > 100, "only {checked} particles were in front of a camera to check");
    }

    /// **Blood in water clouds and is gone; it does not fall like a drop in
    /// air.**
    ///
    /// "кровь должно растворятся под водой": a drop in a lake was the same
    /// drop falling at the same speed as in the air, and it left its mark on
    /// the bed. The same blow struck under the surface and in the air above it,
    /// stepped the same way: the one in the air falls over a block in half a
    /// second; the one in the water is a cloud from its first frame, barely
    /// moves, leaves no mark and is gone inside `CLOUD_LIFE`.
    #[test]
    fn a_drop_of_blood_born_in_water_clouds_fades_quickly_and_never_falls_like_one_in_air() {
        use primitive_shared::types::BLOCK_WATER;
        let lake = crate::logic::physics::tests::world_of(|y| match y {
            0..=9 => BLOCK_STONE,
            10..=13 => BLOCK_WATER,
            _ => BLOCK_AIR,
        });
        let under = Vec3::new(4.5, 12.5, 4.5);
        let over = Vec3::new(4.5, 30.5, 4.5);
        let (mut in_water, mut in_air) = (Particles::new(), Particles::new());
        in_water.blood(under);
        in_air.blood(over);
        let dt = 1.0 / 60.0;
        in_water.update(&lake, dt);
        in_air.update(&lake, dt);
        assert!(
            in_water.live.iter().all(|p| p.look == Look::Cloud),
            "a drop born under water is still a drop: {:?}",
            in_water.live.iter().map(|p| p.look).collect::<Vec<_>>(),
        );
        for _ in 0..30 {
            in_water.update(&lake, dt);
            in_air.update(&lake, dt);
        }
        let (low, high) = in_water
            .live
            .iter()
            .fold((f32::MAX, f32::MIN), |(l, h), p| (l.min(p.position.y as f32), h.max(p.position.y as f32)));
        assert!(
            low > under.y - 0.2 && high < under.y + 0.2,
            "blood in water moved from {} to between {low} and {high} in half a second",
            under.y,
        );
        let fallen = in_air.live.iter().map(|p| p.position.y as f32).fold(f32::MAX, f32::min);
        assert!(fallen < over.y - 1.0, "blood in the air did not fall (lowest {fallen})");

        for _ in 0..((CLOUD_LIFE * 60.0) as usize) {
            in_water.update(&lake, dt);
            assert!(in_water.live.iter().all(|p| p.look != Look::Stain), "blood left a mark on a lake bed");
        }
        assert!(in_water.is_empty(), "{} clouds outlived their second", in_water.len());

        // ...and it is a thin red cloud, not a speck.
        for shade in 0..BLOOD.len() as u8 {
            let [r, g, b, a] = Look::Cloud.tint(1.0, 1.0, shade);
            assert!(r > g * 3.0 && r > b * 3.0, "a cloud of shade {shade} is {r},{g},{b}");
            assert!(a <= CLOUD_ALPHA && a > 0.0, "a cloud is {a} opaque");
        }
        assert_eq!(Look::Cloud.tint(0.0, 0.0, 0)[3], 0.0, "a cloud never clears");
    }

    /// A mark on the ground that water comes over goes the way a drop in water
    /// does: a cloud, and then nothing -- not a stain lying on a flooded floor.
    #[test]
    fn a_mark_on_the_ground_that_water_covers_clouds_and_goes() {
        use primitive_shared::types::BLOCK_WATER;
        let dry = world_with_floor();
        let flooded = crate::logic::physics::tests::world_of(|y| match y {
            0..=9 => BLOCK_STONE,
            10..=12 => BLOCK_WATER,
            _ => BLOCK_AIR,
        });
        let mut particles = Particles::new();
        particles.blood(Vec3::new(4.0, 11.3, 4.0));
        for _ in 0..120 {
            particles.update(&dry, 1.0 / 60.0);
        }
        assert!(particles.live.iter().any(|p| p.look == Look::Stain), "the blow left no mark to flood");

        particles.update(&flooded, 1.0 / 60.0);
        assert!(
            particles.live.iter().all(|p| p.look == Look::Cloud),
            "a mark under water is still a mark: {:?}",
            particles.live.iter().map(|p| p.look).collect::<Vec<_>>(),
        );
        for _ in 0..((CLOUD_LIFE * 60.0) as usize + 2) {
            particles.update(&flooded, 1.0 / 60.0);
        }
        assert!(particles.is_empty(), "{} particles outlived the flood", particles.len());
    }
}
