//! The wind made visible: streaks of moving air and dust or grass carried
//! low over open ground in a strong wind, and leaves torn off the crowns of
//! broadleaf trees -- a few on a still autumn day, a stream of them in a gale.
//!
//! ## Why a pool of its own, and not more `Look`s in `Particles`
//!
//! Three homes were weighed:
//!
//! * **More looks in `engine::particles`.** A particle there is thrown and
//!   falls: it has a velocity, a gravity and a drag, and nothing pushes it
//!   after it is born. A leaf that is not pushed is a leaf that slows to a
//!   stop in mid-air and drops like a chip of stone, and giving every
//!   particle a wind to be pushed by is a field on a struct built in forty
//!   places and a multiply in a loop over a thousand raindrops that do not
//!   want it. And they would share `particles::MAX` with the rain, so a
//!   storm -- the one time the wind is worth seeing -- would be the time the
//!   rain had already taken every slot.
//! * **Critters** (`engine::critters`). A critter has a mind: a home, a
//!   fright, an hour. A leaf has none of those, and a pool of forty leaves
//!   thinking would be forty `match`es a frame for nothing.
//! * **This (chosen)**: a small pool beside both that knows the wind, with
//!   caps of its own and one world read a mote a frame, drawn into the same
//!   particle buffer. What it gives up is that nothing here is the server's:
//!   two players see different leaves, which nobody can tell.
//!
//! ## What each is for
//!
//! **A mechanic should create a decision, not a chore**, and scenery is
//! neither, so each carries one true thing:
//!
//! * **streaks and blown dust** say which way the wind is going and how hard,
//!   *on land* -- the thing a sail and a slant of rain said only at sea or in
//!   the rain. They are drawn only in a strong wind (`STRONG`) and only under
//!   open sky, so the moment they stop when a player steps into a doorway is
//!   the moment the player knows they are out of it;
//! * **falling leaves** are the season and the kind of wood: a broadleaf
//!   crown sheds, a conifer does not (`sheds`), autumn sheds on a still day
//!   and summer only in a storm, and the colour is the tree's -- a birch wood
//!   in autumn is yellow in the air before the player has looked up.
//!
//! Rejected: leaves as blocks that pile up. A drift of leaves the server had
//! to store, stream and melt is a chunk edit for every gust -- the price of a
//! thing a player can gather, and nothing here is.

use glam::Vec3;

use primitive_shared::lighting::LightMap;
use primitive_shared::raft::Wind;
use primitive_shared::season::Season;
use primitive_shared::types::{self, BlockId};

use crate::engine::critters::quad;
use crate::engine::mesh::pack_light;
use crate::engine::particles::ParticleVertex;
use crate::engine::texture::{FaceLayers, EXTRA_SNOW};
use crate::logic::chunk_manager::ChunkManager;

/// The most streaks of moving air alive at once. Each is one long thin quad.
pub const MAX_STREAKS: usize = 24;
/// The most specks of dust or bits of grass.
pub const MAX_DUST: usize = 60;
/// The most leaves, in the air and lying on the ground together.
///
/// Forty-eight: an autumn wood in a gale is a stream of them past the eye,
/// and a leaf lies for several seconds after it lands, so the cap is mostly
/// the ground's.
pub const MAX_LEAVES: usize = 48;

/// The wind strength (`raft::Wind::strength`, 0..1) past which the air is
/// drawn at all.
///
/// **Past a fair day's breeze and short of a rain's.** A clear sky's wind is
/// 0.45 before its lull (`raft::weather_wind`), so a fair day shows moving
/// air only in a squall; rain blows at 0.68 and a storm at 1, so weather
/// shows it most of the time. Drawn in every wind it would be wallpaper, and
/// wallpaper says nothing about the wind.
pub const STRONG: f32 = 0.55;

/// How fast the air itself moves at full strength, in blocks a second.
/// `RAIN_WIND_SPEED` in `lib.rs` is the rain's twelve; a streak is the air,
/// which runs a little ahead of the drops it carries.
const AIR_SPEED: f32 = 14.0;

/// How many streaks and specks a second a full gale sends past, before the
/// caps. Scaled down to nothing at `STRONG`.
const STREAKS_PER_SECOND: f32 = 14.0;
const DUST_PER_SECOND: f32 = 40.0;

/// How many columns a second are looked at for a crown to shed a leaf.
///
/// **One column a look**, the critters' way (`critters::LOOK_EVERY`): at most
/// `CROWN_UP + CROWN_DOWN` reads each, so the whole of the searching is a few
/// hundred reads a second however much wood is loaded.
const CROWN_LOOKS_PER_SECOND: f32 = 16.0;
const CROWN_UP: i32 = 16;
const CROWN_DOWN: i32 = 4;

/// The ring round the player a mote is born in, in blocks.
const NEAREST: f32 = 2.0;
const FURTHEST: f32 = 16.0;

/// Sky light, of fifteen, at the player's head that is "out in it".
///
/// **Fourteen, not fifteen**, so the eaves of a tree do not count as indoors:
/// a canopy takes a level or two off. A doorway three blocks in is eleven or
/// twelve, and a cave is nought.
const OPEN_SKY: u8 = 14;

/// How fast a leaf sinks through still air, in blocks a second, and how much
/// of the wind's speed it takes. A leaf is a sail: it goes with the air far
/// more than it falls through it.
const LEAF_SINK: f32 = 1.1;
const LEAF_CARRY: f32 = 0.35;
/// How long a leaf lies where it came down before it is gone, in seconds.
const LEAF_LIES: (f32, f32) = (5.0, 9.0);
/// Half the width of a leaf, in blocks.
const LEAF_SIZE: f32 = 0.09;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Streak,
    Dust,
    Leaf,
}

#[derive(Debug, Clone, Copy)]
struct Mote {
    kind: Kind,
    position: glam::DVec3,
    velocity: Vec3,
    /// Seconds left, and how many it started with.
    life: f32,
    total: f32,
    /// Seconds lived, for the flutter and the spin.
    phase: f32,
    colour: [f32; 3],
    /// A leaf on the ground.
    landed: bool,
}

/// What the pool needs from the frame.
pub struct Air<'a> {
    pub chunks: &'a ChunkManager,
    pub light: &'a LightMap,
    /// The player's feet.
    pub player: Vec3,
    /// The world's wind (`raft::wind`), the one the rain falls in and the
    /// clouds run with.
    pub wind: Wind,
    /// The world's age in days, for the season.
    pub world_time: f32,
    /// Raining or snowing: wet ground raises no dust.
    pub wet: bool,
}

/// The pool.
pub struct Breeze {
    live: Vec<Mote>,
    seed: u32,
    /// Fractions of a streak, a speck and a look the last frame's rate left
    /// over, so a rate is the rate at any frame rate (`Particles::rapids`
    /// does the same for foam).
    streaks_owed: f32,
    dust_owed: f32,
    looks_owed: f32,
}

impl Default for Breeze {
    fn default() -> Self {
        Self::new()
    }
}

impl Breeze {
    pub fn new() -> Self {
        Self {
            live: Vec::with_capacity(MAX_STREAKS + MAX_DUST + MAX_LEAVES),
            seed: 0x0B1E_E2E5,
            streaks_owed: 0.0,
            dust_owed: 0.0,
            looks_owed: 0.0,
        }
    }

    /// Where the leaves are and whether each has come down, for a repro's
    /// printout.
    #[cfg(test)]
    pub fn leaves(&self) -> Vec<(Vec3, bool)> {
        self.live.iter().filter(|m| m.kind == Kind::Leaf).map(|m| (m.position.as_vec3(), m.landed)).collect()
    }

    /// How many of one kind are alive.
    pub fn count(&self, kind: Kind) -> usize {
        self.live.iter().filter(|m| m.kind == kind).count()
    }

    fn random(&mut self) -> f32 {
        // xorshift32, as `Particles` and `Critters` use.
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        (self.seed >> 8) as f32 / (1 << 24) as f32
    }

    fn between(&mut self, low: f32, high: f32) -> f32 {
        low + self.random() * (high - low)
    }

    /// One frame: stock the air, then move everything in it.
    pub fn update(&mut self, air: &Air<'_>, dt: f32) {
        let dt = dt.clamp(0.0, 0.1);
        let (sin, cos) = air.wind.toward.sin_cos();
        let heading = Vec3::new(cos, 0.0, sin);
        let gust = ((air.wind.strength - STRONG) / (1.0 - STRONG)).clamp(0.0, 1.0);
        let head = air.player + Vec3::Y * 1.5;
        // **Asked once, at the player's head**, and not of every mote: what
        // the player is standing in decides whether the air is drawn, and a
        // streak that was born outside a window is still the wind outside.
        let out_in_it = sky_at(air.light, head).is_some_and(|sky| sky >= OPEN_SKY);

        if gust > 0.0 && out_in_it {
            self.streaks_owed += STREAKS_PER_SECOND * gust * dt;
            self.dust_owed += if air.wet { 0.0 } else { DUST_PER_SECOND * gust * dt };
        } else {
            self.streaks_owed = 0.0;
            self.dust_owed = 0.0;
        }
        while self.streaks_owed >= 1.0 {
            self.streaks_owed -= 1.0;
            if self.count(Kind::Streak) < MAX_STREAKS {
                self.blow_a_streak(air, heading);
            }
        }
        while self.dust_owed >= 1.0 {
            self.dust_owed -= 1.0;
            if self.count(Kind::Dust) < MAX_DUST {
                self.blow_dust(air, heading);
            }
        }

        let season = Season::at(air.world_time);
        let chance = shed_chance(season, air.wind.strength);
        if chance > 0.0 {
            self.looks_owed += CROWN_LOOKS_PER_SECOND * dt;
        } else {
            self.looks_owed = 0.0;
        }
        while self.looks_owed >= 1.0 {
            self.looks_owed -= 1.0;
            if self.count(Kind::Leaf) < MAX_LEAVES {
                self.look_for_a_crown(air, season, chance);
            }
        }

        let air_speed = AIR_SPEED * air.wind.strength;
        for index in 0..self.live.len() {
            let mut mote = self.live[index];
            mote.life -= dt;
            mote.phase += dt;
            match mote.kind {
                Kind::Streak => {
                    mote.position += (mote.velocity * dt).as_dvec3();
                    if solid(air.chunks, mote.position.as_vec3()) {
                        mote.life = 0.0;
                    }
                }
                Kind::Dust => {
                    // Skipping along the ground rather than flying level: a
                    // speck is lifted and dropped by the eddies over grass.
                    let bob = (mote.phase * 5.0 + mote.colour[0] * 40.0).sin() * 0.9;
                    let wanted = heading * air_speed * 0.7 + Vec3::Y * bob;
                    mote.velocity = mote.velocity.lerp(wanted, (dt * 3.0).min(1.0));
                    let next = mote.position + (mote.velocity * dt).as_dvec3();
                    if solid(air.chunks, next.as_vec3()) {
                        mote.life = 0.0;
                    } else {
                        mote.position = next;
                    }
                }
                Kind::Leaf => self.step_leaf(&mut mote, air, heading, air_speed, dt),
            }
            self.live[index] = mote;
        }
        let player = air.player;
        self.live.retain(|m| m.life > 0.0 && (m.position.as_vec3() - player).length_squared() < 40.0 * 40.0);
    }

    fn step_leaf(&mut self, leaf: &mut Mote, air: &Air<'_>, heading: Vec3, air_speed: f32, dt: f32) {
        if leaf.landed {
            return;
        }
        // **Carried, sinking, and rocking across its path**: the flutter is
        // what tells a leaf from a chip of bark at thirty paces.
        let across = Vec3::new(-heading.z, 0.0, heading.x);
        let rock = (leaf.phase * 2.3 + leaf.colour[1] * 30.0).sin();
        let wanted = heading * air_speed * LEAF_CARRY + across * rock * 0.8
            + Vec3::Y * (-LEAF_SINK + 0.5 * (leaf.phase * 4.1).cos());
        leaf.velocity = leaf.velocity.lerp(wanted, (dt * 2.0).min(1.0));
        let next = leaf.position + (leaf.velocity * dt).as_dvec3();
        let cell = |at: Vec3| air.chunks.block_at(at.x.floor() as i32, at.y.floor() as i32, at.z.floor() as i32);
        let down = cell(next.as_vec3());
        if down.is_some_and(|b| types::is_collidable(b) || types::is_liquid(b)) {
            if leaf.velocity.y < 0.0 && next.y.floor() < leaf.position.y.floor() {
                // Down on the top of whatever it met, and it lies there.
                leaf.position.y = next.y.floor() + if down.is_some_and(types::is_liquid) { 0.9 } else { 1.02 };
                leaf.landed = true;
                leaf.velocity = Vec3::ZERO;
                let lies = self.between(LEAF_LIES.0, LEAF_LIES.1);
                leaf.life = lies;
                leaf.total = lies;
            } else {
                // Against a trunk or a wall: it slides down it.
                leaf.velocity.x = 0.0;
                leaf.velocity.z = 0.0;
            }
            return;
        }
        leaf.position = next;
    }

    /// A streak of air, born upwind of the player so it crosses the view.
    fn blow_a_streak(&mut self, air: &Air<'_>, heading: Vec3) {
        let Some(at) = self.open_spot(air, heading, 0.6, 4.5) else {
            return;
        };
        let speed = AIR_SPEED * air.wind.strength * self.between(0.85, 1.15);
        let rise = self.between(-0.3, 0.3);
        let life = self.between(0.5, 1.1);
        self.live.push(Mote {
            kind: Kind::Streak,
            position: at.as_dvec3(),
            velocity: heading * speed + Vec3::Y * rise,
            life,
            total: life,
            phase: 0.0,
            colour: [0.93, 0.95, 0.98],
            landed: false,
        });
    }

    /// A speck of whatever the ground there is made of, low over it.
    fn blow_dust(&mut self, air: &Air<'_>, heading: Vec3) {
        let Some(at) = self.open_spot(air, heading, 0.15, 1.4) else {
            return;
        };
        // What it is, from the ground under it: sand is sand, snow is snow,
        // and over grass it is bits of dry grass. Nothing off rock, water or
        // a void -- a speck of nothing blown off a cliff edge.
        let (x, z) = (at.x.floor() as i32, at.z.floor() as i32);
        let ground = (0..4)
            .map(|down| at.y.floor() as i32 - down)
            .find_map(|y| air.chunks.block_at(x, y, z).filter(|&b| !types::is_air(b)));
        let Some(ground) = ground else {
            return;
        };
        let tone = self.between(0.85, 1.1);
        let colour = match types::block_kind(ground) {
            types::BLOCK_SAND => [0.80, 0.70, 0.48],
            types::BLOCK_SNOW => [0.95, 0.97, 1.0],
            types::BLOCK_GRASS | types::BLOCK_TALL_GRASS | types::BLOCK_DIRT => {
                if self.random() < 0.5 {
                    [0.72, 0.66, 0.38]
                } else {
                    [0.46, 0.56, 0.26]
                }
            }
            _ => return,
        };
        let life = self.between(1.0, 2.2);
        let phase = self.between(0.0, 3.0);
        self.live.push(Mote {
            kind: Kind::Dust,
            position: at.as_dvec3(),
            velocity: heading * AIR_SPEED * air.wind.strength * 0.5,
            life,
            total: life,
            phase,
            colour: [colour[0] * tone, colour[1] * tone, colour[2] * tone],
            landed: false,
        });
    }

    /// A cell of open air near the player at a height over their feet,
    /// upwind of them by half the ring so what is born crosses the view
    /// rather than leaving it. `None` when the cell is not under open sky.
    fn open_spot(&mut self, air: &Air<'_>, heading: Vec3, low: f32, high: f32) -> Option<Vec3> {
        let angle = self.between(0.0, std::f32::consts::TAU);
        let distance = self.between(NEAREST, FURTHEST);
        let at = air.player + Vec3::new(angle.cos(), 0.0, angle.sin()) * distance - heading * FURTHEST * 0.5
            + Vec3::Y * self.between(low, high);
        let block = air.chunks.block_at(at.x.floor() as i32, at.y.floor() as i32, at.z.floor() as i32)?;
        if !types::is_air(block) {
            return None;
        }
        // **Fifteen: under nothing at all.** A streak in a cave mouth or under
        // a roof is wind where the wind is not.
        (sky_at(air.light, at)? >= 15).then_some(at)
    }

    /// One column: if the first thing in it from above is a broadleaf crown
    /// with air under it, perhaps a leaf.
    fn look_for_a_crown(&mut self, air: &Air<'_>, season: Season, chance: f32) {
        let angle = self.between(0.0, std::f32::consts::TAU);
        let distance = self.between(NEAREST, FURTHEST);
        let x = (air.player.x + angle.cos() * distance).floor() as i32;
        let z = (air.player.z + angle.sin() * distance).floor() as i32;
        let feet = air.player.y.floor() as i32;
        for y in (feet - CROWN_DOWN..=feet + CROWN_UP).rev() {
            let Some(block) = air.chunks.block_at(x, y, z) else {
                return;
            };
            if types::is_air(block) {
                continue;
            }
            // The first thing from above decides it: a crown under a roof
            // or an overhang is not one the wind is in.
            if !sheds(block) {
                return;
            }
            // Down through the crown to its underside, which is where a leaf
            // comes away -- and only if there is air under it, not a trunk.
            let mut bottom = y;
            while air.chunks.block_at(x, bottom - 1, z).is_some_and(sheds) {
                bottom -= 1;
            }
            if !air.chunks.block_at(x, bottom - 1, z).is_some_and(types::is_air) || self.random() >= chance {
                return;
            }
            let colour = self.leaf_colour(block, season);
            let at = Vec3::new(x as f32 + self.random(), bottom as f32 - 0.1, z as f32 + self.random());
            let life = self.between(9.0, 14.0);
            let phase = self.between(0.0, 6.0);
            self.live.push(Mote {
                kind: Kind::Leaf,
                position: at.as_dvec3(),
                velocity: Vec3::ZERO,
                life,
                total: life,
                phase,
                colour,
                landed: false,
            });
            return;
        }
    }

    /// The tree's own colour in this season, a shade apart leaf to leaf.
    fn leaf_colour(&mut self, block: BlockId, season: Season) -> [f32; 3] {
        let autumn = season == Season::Autumn;
        let pick = self.random();
        let tone = self.between(0.85, 1.12);
        let base = match (types::block_kind(block), autumn) {
            (types::BLOCK_BIRCH_LEAVES, true) => [0.86, 0.70, 0.20],
            (types::BLOCK_MAPLE_LEAVES, true) if pick < 0.6 => [0.80, 0.26, 0.10],
            (types::BLOCK_MAPLE_LEAVES, true) => [0.90, 0.52, 0.12],
            (_, true) if pick < 0.4 => [0.62, 0.42, 0.14],
            (_, true) if pick < 0.75 => [0.74, 0.58, 0.18],
            (_, true) => [0.48, 0.32, 0.13],
            (types::BLOCK_BIRCH_LEAVES, false) => [0.46, 0.60, 0.22],
            (types::BLOCK_WILLOW_LEAVES, false) => [0.44, 0.56, 0.26],
            (_, false) => [0.33, 0.50, 0.18],
        };
        [base[0] * tone, base[1] * tone, base[2] * tone]
    }

    /// Everything alive, into the particle buffer, measured from the render
    /// origin as `Particles::build_into` is. `right` and `up` are
    /// `particles::billboard_axes`.
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
        let blob = layers.extra(EXTRA_SNOW);
        let whole = (0.0, 0.0, 1.0, 1.0);
        let flat = (7.5 / 16.0, 7.5 / 16.0, 8.5 / 16.0, 8.5 / 16.0);
        for mote in &self.live {
            let at = (mote.position - origin.as_dvec3()).as_vec3();
            let lit = {
                let (sky, block) = crate::logic::entities::sampled_light(mote.position, light);
                pack_light(sky, block, 3, 0)
            };
            let age = 1.0 - (mote.life / mote.total.max(1e-3)).clamp(0.0, 1.0);
            match mote.kind {
                Kind::Streak => {
                    // **In and out over its whole life**: a line of air that
                    // began or ended at full strength would be a line
                    // somebody drew.
                    let alpha = 0.3 * (age * std::f32::consts::PI).sin();
                    let direction = mote.velocity.normalize_or(Vec3::X);
                    let toward = up.cross(right);
                    let across = direction.cross(toward);
                    if across.length_squared() < 1e-4 {
                        continue;
                    }
                    let (a, b) = (across.normalize() * 0.028, direction * 1.2);
                    let [r, g, bl] = mote.colour;
                    quad(vertices, indices, [at - a + b, at + a + b, at + a - b, at - a - b], whole, blob, lit, [r, g, bl, alpha]);
                }
                Kind::Dust => {
                    let alpha = (mote.life / 0.3).min(1.0) * (age / 0.1).min(1.0);
                    let (a, b) = (right * 0.022, up * 0.022);
                    let [r, g, bl] = mote.colour;
                    quad(vertices, indices, [at - a + b, at + a + b, at + a - b, at - a - b], flat, blob, lit, [r, g, bl, alpha]);
                }
                Kind::Leaf => {
                    // Turning about the vertical and rocking as it goes, and
                    // flat once it lies. A diamond, because a square leaf is
                    // a confetti.
                    let spin = mote.phase * 2.7 + mote.colour[2] * 50.0;
                    let tilt = if mote.landed { 0.0 } else { (mote.phase * 3.3).sin() * 1.1 };
                    let long = Vec3::new(spin.cos(), 0.0, spin.sin());
                    let wide = Vec3::new(-spin.sin(), 0.0, spin.cos()) * tilt.cos() + Vec3::Y * tilt.sin();
                    let alpha = if mote.landed { (mote.life / 1.5).min(1.0) } else { (age * 20.0).min(1.0) };
                    let (a, b) = (long * LEAF_SIZE, wide * LEAF_SIZE * 0.7);
                    let [r, g, bl] = mote.colour;
                    quad(vertices, indices, [at + a, at + b, at - a, at - b], flat, blob, lit, [r, g, bl, alpha]);
                }
            }
        }
    }
}

/// Does this crown shed leaves?
///
/// **Broadleaf canopy only.** A conifer's needles, a palm's fronds and a
/// saxaul's scales do not come off a tree in drifts, and a pine wood in a
/// gale throwing yellow leaves would be the game saying something false
/// about the pine. A bush is not a crown the wind strips either. A new
/// broadleaf tree is one line here.
pub fn sheds(block: BlockId) -> bool {
    matches!(
        types::block_kind(block),
        types::BLOCK_LEAVES
            | types::BLOCK_BIRCH_LEAVES
            | types::BLOCK_MAPLE_LEAVES
            | types::BLOCK_WILLOW_LEAVES
            | types::BLOCK_APPLE_LEAVES
            | types::BLOCK_APPLE_LEAVES_FRUIT
            | types::BLOCK_ACACIA_LEAVES
    )
}

/// The chance a crown found by a look lets a leaf go, by season and wind.
///
/// Autumn sheds in any air and a stream in a gale; spring and summer only
/// when the wind is strong enough to tear green leaves off, and then few;
/// winter's crowns have nothing left to give.
fn shed_chance(season: Season, strength: f32) -> f32 {
    let blow = ((strength - 0.3) / 0.7).clamp(0.0, 1.0);
    match season {
        Season::Autumn => 0.12 + 0.55 * blow,
        Season::Spring | Season::Summer => 0.25 * blow * blow,
        Season::Winter => 0.0,
    }
}

/// The sky light at a point, or `None` where the light map has not got to
/// the chunk yet -- which is not "open sky", whatever `sampled_light` says for
/// an entity's sake.
fn sky_at(light: &LightMap, at: Vec3) -> Option<u8> {
    let (x, y, z) = (at.x.floor() as i32, at.y.floor() as i32, at.z.floor() as i32);
    light
        .is_lit(primitive_shared::types::ChunkPos::from_world(at.x, at.z))
        .then(|| light.sky(x, y, z))
}

fn solid(chunks: &ChunkManager, at: Vec3) -> bool {
    chunks
        .block_at(at.x.floor() as i32, at.y.floor() as i32, at.z.floor() as i32)
        .is_some_and(types::is_collidable)
}

#[cfg(test)]
mod tests {
    use super::*;
    use primitive_shared::types::{Chunk, ChunkPos, BLOCK_AIR, BLOCK_GRASS, BLOCK_STONE, CHUNK_VOLUME};

    /// Grass at y = 10 over stone, nine chunks, with some cells written over
    /// it, and its light worked out.
    fn world(cells: &[((i32, i32, i32), BlockId)]) -> (ChunkManager, LightMap) {
        let mut chunks = ChunkManager::new(4);
        for cx in -1..=1 {
            for cz in -1..=1 {
                let mut blocks = vec![BLOCK_AIR; CHUNK_VOLUME];
                for lz in 0..16usize {
                    for lx in 0..16usize {
                        for y in 0..10 {
                            blocks[Chunk::index(lx, y, lz)] = BLOCK_STONE;
                        }
                        blocks[Chunk::index(lx, 10, lz)] = BLOCK_GRASS;
                    }
                }
                chunks.insert(Chunk { pos: ChunkPos::new(cx, cz), blocks });
            }
        }
        for &((x, y, z), block) in cells {
            let pos = ChunkPos::new(x.div_euclid(16), z.div_euclid(16));
            let mut chunk = chunks.get(pos).unwrap().clone();
            chunk.set(x.rem_euclid(16) as usize, y as usize, z.rem_euclid(16) as usize, block);
            chunks.insert(chunk);
        }
        let mut light = LightMap::new();
        for cx in -1..=1 {
            for cz in -1..=1 {
                light.load_chunk(&chunks, ChunkPos::new(cx, cz));
            }
        }
        (chunks, light)
    }

    fn summer() -> f32 {
        primitive_shared::season::MIDSUMMER_WORLD_TIME + 0.5
    }

    fn autumn() -> f32 {
        summer() + primitive_shared::season::SEASON_DAYS
    }

    /// Runs the pool for some seconds and hands back the most of each kind
    /// seen at once.
    fn most(chunks: &ChunkManager, light: &LightMap, player: Vec3, strength: f32, time: f32, seconds: f32) -> [usize; 3] {
        let mut breeze = Breeze::new();
        let mut most = [0; 3];
        for _ in 0..(seconds * 30.0) as usize {
            let air = Air { chunks, light, player, wind: Wind { toward: 0.6, strength }, world_time: time, wet: false };
            breeze.update(&air, 1.0 / 30.0);
            for (slot, kind) in [Kind::Streak, Kind::Dust, Kind::Leaf].into_iter().enumerate() {
                most[slot] = most[slot].max(breeze.count(kind));
            }
            assert!(breeze.count(Kind::Streak) <= MAX_STREAKS);
            assert!(breeze.count(Kind::Dust) <= MAX_DUST);
            assert!(breeze.count(Kind::Leaf) <= MAX_LEAVES);
        }
        most
    }

    #[test]
    fn the_air_is_drawn_only_in_a_strong_wind_and_only_under_open_sky() {
        let (open, light) = world(&[]);
        let field = Vec3::new(8.0, 11.0, 8.0);
        let [streaks, dust, _] = most(&open, &light, field, 0.3, summer(), 10.0);
        assert_eq!(streaks + dust, 0, "moving air drawn in a breeze");
        let [streaks, dust, _] = most(&open, &light, field, 0.95, summer(), 10.0);
        assert!(streaks > 0 && dust > 0, "a gale over an open field drew {streaks} streaks and {dust} specks");

        // A cave: a hollow in the stone under the meadow, sealed.
        let mut hollow = Vec::new();
        for x in 2..14 {
            for z in 2..14 {
                for y in 4..7 {
                    hollow.push(((x, y, z), BLOCK_AIR));
                }
            }
        }
        let (cave, light) = world(&hollow);
        let [streaks, dust, _] = most(&cave, &light, Vec3::new(8.0, 4.0, 8.0), 0.95, summer(), 10.0);
        assert_eq!(streaks + dust, 0, "wind drawn in a sealed cave");

        // A hut: stone walls and a roof over the player, a doorway open.
        let mut hut = Vec::new();
        for x in 4..=12 {
            for z in 4..=12 {
                hut.push(((x, 15, z), BLOCK_STONE));
                let wall = x == 4 || x == 12 || z == 4 || z == 12;
                let door = z == 4 && x == 8;
                if wall && !door {
                    for y in 11..15 {
                        hut.push(((x, y, z), BLOCK_STONE));
                    }
                }
            }
        }
        let (indoors, light) = world(&hut);
        let [streaks, dust, _] = most(&indoors, &light, Vec3::new(8.5, 11.0, 9.5), 0.95, summer(), 10.0);
        assert_eq!(streaks + dust, 0, "wind drawn inside a hut");
    }

    /// A crown of `leaves` from y = 14 to 16 over a trunk, a few columns
    /// round the player.
    fn grove(leaves: BlockId) -> Vec<((i32, i32, i32), BlockId)> {
        let mut cells = Vec::new();
        for (tx, tz) in [(4, 4), (12, 5), (5, 13), (13, 12)] {
            for y in 11..14 {
                cells.push(((tx, y, tz), types::BLOCK_LOG));
            }
            for dx in -2..=2 {
                for dz in -2..=2 {
                    for y in 14..17 {
                        cells.push(((tx + dx, y, tz + dz), leaves));
                    }
                }
            }
        }
        cells
    }

    #[test]
    fn leaves_fall_only_from_broadleaf_crowns_and_lie_where_they_land() {
        let player = Vec3::new(8.5, 11.0, 8.5);
        let (oaks, light) = world(&grove(types::BLOCK_LEAVES));
        let mut breeze = Breeze::new();
        let mut landed = 0;
        for _ in 0..(40.0 * 30.0) as usize {
            let air = Air { chunks: &oaks, light: &light, player, wind: Wind { toward: 0.6, strength: 0.7 }, world_time: autumn(), wet: false };
            breeze.update(&air, 1.0 / 30.0);
            for leaf in breeze.live.iter().filter(|m| m.kind == Kind::Leaf && m.landed) {
                landed += 1;
                let under = oaks.block_at(leaf.position.x.floor() as i32, (leaf.position.y - 0.5).floor() as i32, leaf.position.z.floor() as i32);
                assert!(under.is_some_and(|b| types::is_collidable(b) || types::is_liquid(b)), "a leaf lies on air at {:?}", leaf.position);
            }
        }
        assert!(landed > 0, "no leaf came down in an oak wood in an autumn wind");

        for conifer in [types::BLOCK_PINE_NEEDLES, types::BLOCK_FIR_NEEDLES, types::BLOCK_PALM_FRONDS] {
            let (wood, light) = world(&grove(conifer));
            let [_, _, leaves] = most(&wood, &light, player, 0.95, autumn(), 30.0);
            assert_eq!(leaves, 0, "leaves fell from a crown of block {conifer}");
        }
        let [_, _, leaves] = most(&oaks, &light, player, 0.2, summer(), 30.0);
        assert_eq!(leaves, 0, "green leaves torn off an oak by a summer breeze");
    }

    #[test]
    fn the_air_and_the_leaves_never_outnumber_their_caps() {
        let (oaks, light) = world(&grove(types::BLOCK_MAPLE_LEAVES));
        // `most` asserts the caps every frame.
        let [streaks, dust, leaves] = most(&oaks, &light, Vec3::new(8.5, 11.0, 8.5), 1.0, autumn(), 60.0);
        assert!(streaks > 0 && dust > 0 && leaves > 0, "{streaks} {dust} {leaves}: the gale in a maple wood drew nothing to cap");
    }
}
