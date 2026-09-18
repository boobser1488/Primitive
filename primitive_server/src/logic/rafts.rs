//! Rafts on the water: the server's half of `primitive_shared::raft`.
//!
//! The rules -- what floats, what a stroke is worth, what a sail does in a
//! head wind -- are shared, because the rowing client predicts with them.
//! What lives here is what only the authority has: which rafts exist, who
//! is at whose oars, how many blows each has taken, and the file they are
//! kept in between sessions.
//!
//! ## Shape of the simulation
//!
//! Small, like the items. A raft is a body, the oars its rower last sent,
//! and a count of blows. Rafts do not collide with each other or with
//! animals: two rafts are rare, a raft ramming a deer is rarer, and either
//! would be a per-pair pass every tick for a picture nobody has asked for.
//! Players are not obstacles either -- a swimmer under a raft is lifted
//! onto its deck by their own client (`physics::Player::decks`).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use primitive_shared::protocol::{entity_id, EntityId, EntityKind, EntitySource, EntityState, PlayerId};
use primitive_shared::raft::{self, Body, Oars, Wind};

use crate::logic::world::World;

/// The most rafts that may exist at once.
///
/// A cap for the reason the items have one: each is sent to every player
/// near it twenty times a second, and a server should not be talked into an
/// unbounded list by somebody with a great deal of timber.
pub const MAX_RAFTS: usize = 256;

/// A raft with nobody within this many blocks does not move.
///
/// **A drift is a nuisance within a session, not a lottery between them.**
/// The wind moves an unattended raft a few centimetres a second, which is
/// what makes where one is moored a decision. Simulated for a whole night
/// with nobody there, it would cross a lake and be found on the far shore --
/// or, on the sea, not found -- and nobody could have planned against that.
/// It is also what stops a server paying for rafts nobody can see.
pub const STILL_BEYOND: f32 = 128.0;

/// How long a rower's last stroke keeps pulling without another.
///
/// The client sends its oars every quarter second while rowing. A second of
/// silence is a client that went away mid-stroke, and a raft that went on
/// rowing itself across the lake for as long as the session took to time
/// out would be a raft with a ghost at the oars.
const OARS_TIMEOUT: Duration = Duration::from_secs(1);

/// How fast the flash of a blow fades, per second.
const HURT_DECAY_PER_SECOND: f32 = 3.0;

/// How close two rafts may be launched, centre to centre, in blocks.
///
/// Rafts pass through each other (see the module note), so launching one
/// on top of another is refused instead: two decks in one place is two
/// colliders a player falls between.
const LAUNCH_APART: f32 = 3.0;

/// **Two, because the sail grew an angle.** `Body` is written field by
/// field with no names on the wire, so a file from before
/// `Body::sail_angle` is four bytes short of the one this build writes and
/// reading it as the new shape would take the next raft's `x` for this
/// raft's sail angle.
const SAVE_FORMAT_VERSION: u32 = 2;

#[derive(Serialize, Deserialize)]
struct SaveFile {
    version: u32,
    rafts: Vec<Body>,
}

#[derive(Deserialize)]
struct SaveVersion {
    version: u32,
}

/// A raft as version one wrote it: `Body` before the sail had an angle.
///
/// **Kept rather than refused.** A raft is eight planks, four sticks, a
/// sail of four leathers and two oars, and the bump above would otherwise
/// turn every world saved before it into "rafts.bin is version 1, this
/// build reads 2" -- an error the operator is told about, with the rafts
/// still in the file and no way to get them out. So the old shape is read
/// and the yard comes back square, which is where a yard nobody has
/// touched belongs anyway.
#[derive(Deserialize)]
struct SaveFileV1 {
    #[allow(dead_code)]
    version: u32,
    rafts: Vec<BodyV1>,
}

#[derive(Deserialize)]
struct BodyV1 {
    x: f32,
    y: f32,
    z: f32,
    yaw: f32,
    vx: f32,
    vz: f32,
    spin: f32,
    sail: bool,
}

impl From<BodyV1> for Body {
    fn from(old: BodyV1) -> Body {
        Body {
            x: f64::from(old.x),
            y: old.y,
            z: f64::from(old.z),
            yaw: old.yaw,
            vx: old.vx,
            vz: old.vz,
            spin: old.spin,
            sail: old.sail,
            sail_angle: 0.0,
        }
    }
}

pub struct Raft {
    pub id: EntityId,
    pub body: Body,
    oars: Oars,
    oars_at: Option<Instant>,
    /// Who has the oars. Nobody pulls unless this is somebody.
    pub rower: Option<PlayerId>,
    hits: u32,
    hurt: f32,
}

impl Raft {
    pub fn state(&self) -> EntityState {
        EntityState {
            id: self.id,
            kind: EntityKind::Raft {
                yaw: self.body.yaw,
                vx: self.body.vx,
                vz: self.body.vz,
                spin: self.body.spin,
                sail: self.body.sail,
                sail_angle: self.body.sail_angle,
                stroke: self.oars.stroke,
                hurt: self.hurt,
            },
            x: self.body.x,
            y: f64::from(self.body.y),
            z: self.body.z,
        }
    }

    /// What the oars are doing at `now`: the rower's last stroke, or nothing
    /// if there is no rower or they have gone quiet. See `OARS_TIMEOUT`.
    pub fn oars_at(&self, now: Instant) -> Oars {
        match (self.rower, self.oars_at) {
            (Some(_), Some(at)) if now.saturating_duration_since(at) <= OARS_TIMEOUT => self.oars,
            _ => Oars::REST,
        }
    }
}

/// What a blow did to a raft.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Struck {
    /// There is no such raft.
    Missed,
    /// It shook, and holds.
    Hit,
    /// It came apart, here. The raft is gone and what it was is the
    /// caller's to scatter (`raft::SALVAGE`).
    Broke(Body),
}

#[derive(Default)]
pub struct Rafts {
    rafts: Vec<Raft>,
    /// How many rafts this simulation has ever launched. Not the id --
    /// see `protocol::EntitySource`.
    next_ordinal: u64,
    dirty: bool,
}

impl Rafts {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.rafts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rafts.is_empty()
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn get(&self, id: EntityId) -> Option<&Raft> {
        self.rafts.iter().find(|raft| raft.id == id)
    }

    /// A raft onto the water. `None` past the cap, or on top of another raft.
    pub fn launch(&mut self, body: Body) -> Option<EntityId> {
        if self.rafts.len() >= MAX_RAFTS || !body.is_sane() {
            return None;
        }
        let crowded = self
            .rafts
            .iter()
            .any(|other| (other.body.x - body.x).hypot(other.body.z - body.z) < f64::from(LAUNCH_APART));
        if crowded {
            return None;
        }
        self.next_ordinal += 1;
        let id = entity_id(EntitySource::Raft, self.next_ordinal);
        self.rafts.push(Raft {
            id,
            body,
            oars: Oars::REST,
            oars_at: None,
            rower: None,
            hits: 0,
            hurt: 0.0,
        });
        self.dirty = true;
        Some(id)
    }

    /// Gives the oars to a player, if nobody else has them.
    pub fn take_oars(&mut self, id: EntityId, player: PlayerId) -> Result<(), &'static str> {
        let Some(raft) = self.rafts.iter_mut().find(|raft| raft.id == id) else {
            return Err("there is no raft there");
        };
        match raft.rower {
            Some(other) if other != player => Err("somebody is already at the oars"),
            _ => {
                raft.rower = Some(player);
                raft.oars = Oars::REST;
                raft.oars_at = None;
                Ok(())
            }
        }
    }

    /// Takes the oars off whoever this player was rowing, and answers which
    /// raft that was.
    pub fn let_go(&mut self, player: PlayerId) -> Option<EntityId> {
        let raft = self.rafts.iter_mut().find(|raft| raft.rower == Some(player))?;
        raft.rower = None;
        raft.oars = Oars::REST;
        raft.oars_at = None;
        Some(raft.id)
    }

    /// A stroke from a player. Ignored unless that player has these oars:
    /// a passenger's client that sends oars is rowing nothing.
    pub fn row(&mut self, id: EntityId, player: PlayerId, oars: Oars, now: Instant) -> bool {
        let Some(raft) = self.rafts.iter_mut().find(|raft| raft.id == id && raft.rower == Some(player)) else {
            return false;
        };
        raft.oars = oars.clamped();
        raft.oars_at = Some(now);
        true
    }

    /// Raises the sail if it is furled and furls it if it is up. Answers
    /// whether it is up now.
    pub fn toggle_sail(&mut self, id: EntityId) -> Option<bool> {
        let raft = self.rafts.iter_mut().find(|raft| raft.id == id)?;
        raft.body.sail = !raft.body.sail;
        self.dirty = true;
        Some(raft.body.sail)
    }

    /// Braces the yard round to `angle`, within what the mast allows.
    ///
    /// **Whoever asks, not only the rower**, because a raft carries two
    /// people and the second one is the one with a free hand: the check
    /// that they are aboard and within reach of the mast is made by the
    /// caller, which is the only place that knows where a player is
    /// standing. Answers the angle the yard ended at, which is what the
    /// asker is told -- their own number clamped, so a client predicting
    /// with an angle the mast refuses learns so rather than trembling.
    pub fn trim(&mut self, id: EntityId, angle: f32) -> Option<f32> {
        let raft = self.rafts.iter_mut().find(|raft| raft.id == id)?;
        let angle = raft::trim_clamped(angle);
        if raft.body.sail_angle != angle {
            raft.body.sail_angle = angle;
            self.dirty = true;
        }
        Some(angle)
    }

    /// A blow on a raft, worth `blows` of `raft::HITS_TO_BREAK`.
    pub fn strike(&mut self, id: EntityId, blows: u32) -> Struck {
        let Some(index) = self.rafts.iter().position(|raft| raft.id == id) else {
            return Struck::Missed;
        };
        let raft = &mut self.rafts[index];
        raft.hits += blows.max(1);
        raft.hurt = 1.0;
        if raft.hits < raft::HITS_TO_BREAK {
            return Struck::Hit;
        }
        let body = self.rafts.swap_remove(index).body;
        self.dirty = true;
        Struck::Broke(body)
    }

    /// Moves every raft that somebody is near by `dt`.
    pub fn step(
        &mut self,
        world: &World,
        wind: Wind,
        // The hour of the world, for the sea's own circulation: see
        // `fluid::current_at`, which both sides work out rather than send.
        world_days: f32,
        players: &[(f32, f32, f32)],
        now: Instant,
        dt: f32,
    ) {
        let block = |x: i32, y: i32, z: i32| world.cached_block(x, y, z);
        let mut moved = false;
        for raft in &mut self.rafts {
            raft.hurt = (raft.hurt - HURT_DECAY_PER_SECOND * dt).max(0.0);
            let oars = raft.oars_at(now);
            if !oars.pulling() {
                raft.oars = Oars::REST;
            }
            let near = players
                .iter()
                .any(|p| (p.0 - raft.body.x as f32).hypot(p.2 - raft.body.z as f32) < STILL_BEYOND);
            if !near {
                raft.body.vx = 0.0;
                raft.body.vz = 0.0;
                raft.body.spin = 0.0;
                continue;
            }
            let before = raft.body;
            // The river under it, from the generator the client predicts
            // with too (`WorldGen::river_current`); nothing on a lake or the
            // sea.
            let river = world.generator().river_current(raft.body.x as f32, raft.body.y, raft.body.z as f32);
            raft::step(&mut raft.body, oars, wind, world_days, river, &block, dt);
            moved |= raft.body != before;
        }
        self.dirty |= moved;
    }

    pub fn states(&self) -> impl Iterator<Item = EntityState> + '_ {
        self.rafts.iter().map(Raft::state)
    }

    fn save_path(dir: &Path) -> PathBuf {
        dir.join("rafts.bin")
    }

    /// Writes them out, atomically, the way the fires are written.
    ///
    /// The bodies and nothing else: who was rowing belongs to a session that
    /// is over, and the blows a raft had taken are a flash, not damage worth
    /// remembering.
    pub fn save(&mut self, dir: &Path) -> std::io::Result<usize> {
        std::fs::create_dir_all(dir)?;
        let rafts: Vec<Body> = self.rafts.iter().map(|raft| raft.body).collect();
        let count = rafts.len();
        let bytes = bincode::serialize(&SaveFile { version: SAVE_FORMAT_VERSION, rafts })
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let final_path = Self::save_path(dir);
        let tmp_path = final_path.with_extension("bin.tmp");
        std::fs::write(&tmp_path, &bytes)?;
        std::fs::rename(&tmp_path, &final_path)?;
        self.dirty = false;
        Ok(count)
    }

    /// Reads them back. A missing file is a world with no rafts, which is
    /// what every world from before rafts is.
    ///
    /// **A raft is an object somebody spent a great deal on**, so unlike the
    /// fires a file that cannot be read is an error the operator is told
    /// about rather than an empty lake. A raft off a disk that is not a
    /// number anywhere is dropped: it would sit invisible and immovable
    /// forever (see `Body::is_sane`).
    pub fn load(&mut self, dir: &Path) -> std::io::Result<usize> {
        let bytes = match std::fs::read(Self::save_path(dir)) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
            Err(e) => return Err(e),
        };
        let invalid = |e: String| std::io::Error::new(std::io::ErrorKind::InvalidData, e);
        let SaveVersion { version } = bincode::deserialize(&bytes).map_err(|e| invalid(e.to_string()))?;
        let bodies: Vec<Body> = match version {
            SAVE_FORMAT_VERSION => {
                bincode::deserialize::<SaveFile>(&bytes).map_err(|e| invalid(e.to_string()))?.rafts
            }
            // See `SaveFileV1`: a world from before the sail had an angle
            // still has rafts in it, and they come back with the yard
            // square.
            1 => bincode::deserialize::<SaveFileV1>(&bytes)
                .map_err(|e| invalid(e.to_string()))?
                .rafts
                .into_iter()
                .map(Body::from)
                .collect(),
            other => {
                return Err(invalid(format!("rafts.bin is version {other}, this build reads {SAVE_FORMAT_VERSION}")))
            }
        };
        self.rafts.clear();
        for mut body in bodies.into_iter().filter(Body::is_sane) {
            // Still, where it was left: a raft that came back moving would
            // leave the shore it was pulled up on before anybody arrived.
            body.vx = 0.0;
            body.vz = 0.0;
            body.spin = 0.0;
            self.next_ordinal += 1;
            self.rafts.push(Raft {
                id: entity_id(EntitySource::Raft, self.next_ordinal),
                body,
                oars: Oars::REST,
                oars_at: None,
                rower: None,
                hits: 0,
                hurt: 0.0,
            });
        }
        self.dirty = false;
        Ok(self.rafts.len())
    }
}

// ---- what players do to rafts ----
//
// The launch, the three raft messages, a blow, and carrying the people on a
// deck. Here beside the simulation rather than in `lib.rs`, because every
// one of them is about rafts first and a player second, and the file that
// says everything that can happen to a raft is the file somebody changing
// rafts will open.

/// How far past the deck's edge, and above and below its top, a `Deck`
/// transform may claim to be.
///
/// Generous on purpose. Past the edge by a stride covers a rider stepping
/// off into the water, whose next message is an `UpdateTransform`; below by
/// most of a body covers a swimmer climbing aboard; above by a jump covers a
/// jump. What is refused is a claim to be standing on a raft's deck from
/// across the lake.
const DECK_ACCEPT_PAST_EDGE: f32 = 0.8;
const DECK_ACCEPT_BELOW: f32 = 0.9;
const DECK_ACCEPT_ABOVE: f32 = 2.5;

/// What moving the rafts did to the people on them, for the tick loop that
/// bills their hunger.
#[derive(Default)]
pub(crate) struct Rafted {
    /// How far each rider was carried this tick.
    pub carried: Vec<(PlayerId, (f32, f32, f32))>,
    /// Who is pulling on a pair of oars this tick.
    pub pulling: Vec<PlayerId>,
}

fn lock<T>(mutex: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

/// Moves the rafts, then puts everyone aboard one back on its deck.
///
/// **Called before the snapshots are built, every tick**, and that ordering
/// is what makes a rider travel *with* a raft on somebody else's screen. The
/// rider's world position is worked out here from the raft's position this
/// tick and their place on the deck, so the snapshot that carries the rider
/// and the one that carries the raft describe the same instant. A client
/// interpolating the two draws them moving together; a rider whose position
/// came from their own client's view of the raft, a tick or two old, would
/// be drawn a stride towards the stern.
///
/// Locks: the rafts, then each player's state in turn, never the other way
/// round -- every other raft path lets go of a player's lock before it takes
/// the rafts'.
pub(crate) fn tick(ctx: &std::sync::Arc<crate::Context>, handles: &[std::sync::Arc<crate::players::PlayerHandle>], dt: f32) -> Rafted {
    let mut rafted = Rafted::default();
    let now = Instant::now();
    let wind = raft::wind(ctx.clock.world_days(), lock(&ctx.sky).weather());
    let feet: Vec<(f32, f32, f32)> = handles.iter().map(|handle| primitive_shared::geometry::narrow(lock(&handle.state).position)).collect();
    let mut rafts = lock(&ctx.rafts);
    if !rafts.is_empty() {
        rafts.step(&ctx.world, wind, ctx.clock.world_days(), &feet, now, dt);
    }
    let mut stood = Vec::new();
    for handle in handles {
        let mut state = lock(&handle.state);
        let Some((id, local)) = state.aboard else {
            continue;
        };
        let rowing = state.rowing == Some(id);
        let raft = rafts
            .get(id)
            .map(|raft| (raft.body, raft.oars_at(now).pulling()))
            .filter(|_| !state.vitals.is_dead());
        // Gone -- broken up under them -- or they died on it: they are in
        // the water, or on the respawn screen, and not on a deck.
        let Some((body, pulling)) = raft else {
            state.aboard = None;
            if state.rowing.take().is_some() {
                stood.push(std::sync::Arc::clone(handle));
            }
            continue;
        };
        let local = if rowing { raft::SEAT } else { local };
        let world = body.world_of(local);
        let before = state.position;
        let carried = (world[0] - before.0, world[1] - before.1, world[2] - before.2);
        // **Something other than the raft moved them** -- a respawn, a
        // `/tp` -- and a deck that dragged them back across the map would be
        // undoing it every tick. A raft moves a fraction of a block in one.
        if carried.0.hypot(carried.2) > f64::from(raft::BOARDING_REACH + 1.0) {
            state.aboard = None;
            if state.rowing.take().is_some() {
                stood.push(std::sync::Arc::clone(handle));
            }
            continue;
        }
        state.position = (world[0], world[1], world[2]);
        // The anti-cheat's last known position follows the deck, so the
        // `UpdateTransform` a rider sends as they step off is measured from
        // where they stood rather than from where they stepped aboard.
        let at = state.position;
        state.anticheat.reset_to(at);
        if rowing {
            // A rower faces the bow whatever their camera does, as a sitter
            // in a chair faces the chair.
            state.yaw = body.yaw;
            state.on_ground = true;
            if pulling {
                rafted.pulling.push(handle.id);
            }
        }
        rafted.carried.push((handle.id, primitive_shared::geometry::narrow(carried)));
    }
    for handle in &stood {
        rafts.let_go(handle.id);
    }
    drop(rafts);
    for handle in stood {
        handle.send(primitive_shared::protocol::ServerMessage::Oars { raft: None });
        handle.send(primitive_shared::protocol::ServerMessage::Posture {
            posture: primitive_shared::protocol::Posture::Standing,
            at: None,
            yaw: 0.0,
        });
    }
    rafted
}

/// `ClientMessage::Deck`: a player's feet, on a raft's deck.
#[allow(clippy::too_many_arguments)]
pub(crate) fn deck(
    ctx: &std::sync::Arc<crate::Context>,
    handle: &std::sync::Arc<crate::players::PlayerHandle>,
    id: EntityId,
    local: [f32; 3],
    yaw: f32,
    pitch: f32,
    on_ground: bool,
) {
    let body = lock(&ctx.rafts).get(id).map(|raft| raft.body);
    let outcome = {
        let mut state = lock(&handle.state);
        if state.sleeping_in.is_some() || state.vitals.is_dead() {
            return;
        }
        let on_this_deck = local.iter().all(|v| v.is_finite())
            && Body::over_deck(local, raft::DECK_SLACK + DECK_ACCEPT_PAST_EDGE)
            && (-DECK_ACCEPT_BELOW..=DECK_ACCEPT_ABOVE).contains(&local[1]);
        let accepted = body.filter(|body| {
            let world = body.world_of(local);
            let already = state.aboard.is_some_and(|(aboard, _)| aboard == id);
            on_this_deck
                && (already
                    || (world[0] - state.position.0).hypot(world[2] - state.position.2)
                        <= f64::from(raft::BOARDING_REACH))
        });
        let Some(body) = accepted else {
            // Not a violation worth scoring: the likeliest reason is a raft
            // the client still draws that the server has just broken up.
            // The player is put back where the server has them.
            state.aboard = None;
            let at = state.position;
            drop(state);
            handle.send(primitive_shared::protocol::ServerMessage::PositionCorrection {
                x: at.0,
                y: at.1,
                z: at.2,
                reason: "not on that raft".to_string(),
            });
            return;
        };
        match state.rowing {
            // The rower's body is the seat's (see `tick`); only where they
            // look is theirs.
            Some(rowing) if rowing == id => {
                state.pitch = pitch;
                return;
            }
            Some(_) => return,
            None => {}
        }
        let world = body.world_of(local);
        state.aboard = Some((id, local));
        state.position = (world[0], world[1], world[2]);
        state.yaw = yaw;
        state.pitch = pitch;
        state.on_ground = on_ground;
        let at = state.position;
        state.anticheat.reset_to(at);
        // A jump from a cliff onto a deck is still a fall.
        state.vitals.on_transform((world[1]) as f32, on_ground, false)
    };
    crate::report_vitals(ctx, handle, outcome);
}

/// `ClientMessage::UseRaft`: take the oars, or, at them, raise or furl the
/// sail.
pub(crate) fn use_raft(
    ctx: &std::sync::Arc<crate::Context>,
    handle: &std::sync::Arc<crate::players::PlayerHandle>,
    id: EntityId,
) {
    use primitive_shared::protocol::ServerMessage;
    let (aboard, rowing, unable) = {
        let state = lock(&handle.state);
        (
            state.aboard.map(|(aboard, _)| aboard),
            state.rowing,
            state.vitals.is_dead() || state.sleeping_in.is_some(),
        )
    };
    if unable {
        return;
    }
    if rowing == Some(id) {
        if let Some(up) = lock(&ctx.rafts).toggle_sail(id) {
            // **Only raising the sail speaks, and only to say what to do with
            // it.** Bracing the yard is a held button and a sweep of the hand
            // (`riding::sail_at_hand`), and a gesture nobody is told about is
            // a gesture nobody finds. Furling said "you furl the sail", which
            // the player had just watched happen: gone, with the other chat
            // lines that narrated an action back to the one who did it.
            if up {
                handle.send(ServerMessage::Chat {
                    from: None,
                    username: "server".to_string(),
                    text: "hold the swing button to brace the yard to the wind".to_string(),
                });
            }
        }
        return;
    }
    // **From the deck, and not from the bank.** A player who could take the
    // oars of a raft they are not standing on would be rowing it away from
    // whoever is.
    if aboard != Some(id) {
        handle.send(ServerMessage::Error("step aboard to take the oars".to_string()));
        return;
    }
    let taken = {
        let mut rafts = lock(&ctx.rafts);
        rafts.take_oars(id, handle.id).map(|()| rafts.get(id).map(|raft| raft.body))
    };
    let body = match taken {
        Err(why) => {
            handle.send(ServerMessage::Error(why.to_string()));
            return;
        }
        Ok(None) => return,
        Ok(Some(body)) => body,
    };
    let seat = body.world_of(raft::SEAT);
    {
        let mut state = lock(&handle.state);
        state.sitting_on = None;
        state.rowing = Some(id);
        state.aboard = Some((id, raft::SEAT));
        state.position = (seat[0], seat[1], seat[2]);
        state.yaw = body.yaw;
        let at = state.position;
        state.anticheat.reset_to(at);
    }
    handle.send(ServerMessage::Oars { raft: Some(id) });
    handle.send(ServerMessage::Posture {
        posture: primitive_shared::protocol::Posture::Sitting,
        at: Some((seat[0], seat[1], seat[2])),
        yaw: body.yaw,
    });
}

/// `ClientMessage::Trim`: a hand on the sheets, bracing the yard round.
///
/// **The server decides who may**, and the rule is the player's own: at the
/// oars, or standing at the sail (`raft::at_the_sail`). Checked here rather
/// than trusted from the client for the reason every raft message is: a
/// client that could trim any raft by its id could sail one out from under
/// the person standing on it, from the far bank, with the sail furled.
///
/// A refusal is silent. The client shows its own angle for a moment and
/// then the snapshots put the yard back where the server has it -- which is
/// the correction every prediction gets, and saying it in words would be a
/// notice every frame of a drag that was never going to work.
pub(crate) fn trim(
    ctx: &std::sync::Arc<crate::Context>,
    handle: &std::sync::Arc<crate::players::PlayerHandle>,
    id: EntityId,
    angle: f32,
) {
    let hand_on_it = {
        let state = lock(&handle.state);
        if state.vitals.is_dead() || state.sleeping_in.is_some() {
            return;
        }
        state.rowing == Some(id)
            || state
                .aboard
                .is_some_and(|(aboard, local)| aboard == id && raft::at_the_sail(local))
    };
    if !hand_on_it {
        return;
    }
    lock(&ctx.rafts).trim(id, angle);
}

/// `ClientMessage::Row`. Ignored unless this player has these oars.
pub(crate) fn row(
    ctx: &std::sync::Arc<crate::Context>,
    handle: &std::sync::Arc<crate::players::PlayerHandle>,
    id: EntityId,
    stroke: f32,
    turn: f32,
) {
    lock(&ctx.rafts).row(id, handle.id, Oars { stroke, turn }, Instant::now());
}

/// A raft out of the hand and onto the water at `at`, a cell of water the
/// player right-clicked. See `raft::launch` for where it goes.
pub(crate) fn launch(
    ctx: &std::sync::Arc<crate::Context>,
    handle: &std::sync::Arc<crate::players::PlayerHandle>,
    at: (i32, i32, i32),
) {
    use primitive_shared::protocol::ServerMessage;
    use primitive_shared::types::{block_kind, BLOCK_RAFT};
    let (yaw, holding) = {
        let state = lock(&handle.state);
        (
            state.yaw,
            state
                .inventory
                .block_in(state.selected_slot)
                .is_some_and(|held| block_kind(held) == BLOCK_RAFT),
        )
    };
    if !holding {
        return;
    }
    let aim = [f64::from(at.0) + 0.5, f64::from(at.1) + 0.5, f64::from(at.2) + 0.5];
    let Some(body) = raft::launch(aim, yaw, &|x, y, z| ctx.world.cached_block(x, y, z)) else {
        handle.send(ServerMessage::Error(
            "a raft needs open water: three blocks long, two wide, and deep enough to float".to_string(),
        ));
        return;
    };
    // Out of the pack before it is on the water, so that no failure between
    // the two can leave a player with a raft in the pack *and* one afloat.
    let taken = {
        let mut state = lock(&handle.state);
        let taken = state.inventory.take_one(BLOCK_RAFT);
        state.inventory_dirty |= taken;
        taken
    };
    if !taken {
        return;
    }
    if lock(&ctx.rafts).launch(body).is_none() {
        let mut state = lock(&handle.state);
        let _ = state.inventory.add(BLOCK_RAFT, 1);
        drop(state);
        handle.send(ServerMessage::Error("there is already a raft there".to_string()));
    }
    crate::send_inventory(handle);
}

/// A swing that landed on a raft, from an eye at `eye` with `reach`.
pub(crate) fn strike(
    ctx: &std::sync::Arc<crate::Context>,
    handle: &std::sync::Arc<crate::players::PlayerHandle>,
    id: EntityId,
    eye: (f32, f32, f32),
    reach: f32,
) {
    let Some(body) = lock(&ctx.rafts).get(id).map(|raft| raft.body) else {
        return;
    };
    // **To the nearest timber, not to the middle.** A raft is three blocks
    // long, and a swing at its bow from the bank lands on the bow.
    let local = body.local_of([f64::from(eye.0), f64::from(eye.1), f64::from(eye.2)]);
    let nearest = [
        local[0].clamp(-raft::HALF_LENGTH, raft::HALF_LENGTH),
        local[1].clamp(-(raft::FREEBOARD + raft::DRAFT), 0.0),
        local[2].clamp(-raft::HALF_WIDTH, raft::HALF_WIDTH),
    ];
    let gap = ((local[0] - nearest[0]).powi(2) + (local[1] - nearest[1]).powi(2) + (local[2] - nearest[2]).powi(2)).sqrt();
    if gap > reach + 0.5 {
        return;
    }
    let Struck::Broke(body) = lock(&ctx.rafts).strike(id, 1) else {
        return;
    };
    let now = Instant::now();
    {
        let mut items = lock(&ctx.items);
        for (index, &(block, count)) in raft::SALVAGE.iter().enumerate() {
            // Thrown apart rather than stacked in one spot, so what came off
            // the raft reads as the raft coming apart. Everything on the list
            // floats but the sail, which sinks slowly -- fish it out quickly.
            let (sin, cos) = (index as f32 * 1.7).sin_cos();
            items.spawn(
                block,
                count,
                ((body.x + f64::from(cos * 0.6)), f64::from(body.deck_top() + 0.3), (body.z + f64::from(sin * 0.6))),
                (cos * 1.2, 2.0, sin * 1.2),
                None,
                now,
            );
        }
    }
    handle.send(primitive_shared::protocol::ServerMessage::Chat {
        from: None,
        username: "server".to_string(),
        text: "the raft comes apart".to_string(),
    });
}

/// A player left the server: nobody is at their oars any more.
pub(crate) fn forget(ctx: &std::sync::Arc<crate::Context>, player: PlayerId) {
    lock(&ctx.rafts).let_go(player);
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "primitive_rafts_{}_{tag}_{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("temp dir");
            Self(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn a_raft_is_where_it_was_left_after_a_save_and_load() {
        let dir = TempDir::new("save");
        let mut rafts = Rafts::new();
        let mut left = Body::at_rest(12.25, 62.88, -40.5, 1.2);
        left.sail = true;
        left.sail_angle = -0.8;
        left.vx = 1.5;
        rafts.launch(left).expect("launch");
        assert!(rafts.is_dirty());
        assert_eq!(rafts.save(&dir.0).expect("save"), 1);

        let mut again = Rafts::new();
        assert_eq!(again.load(&dir.0).expect("load"), 1);
        let body = again.states().next().map(|s| (s.x as f32, s.y as f32, s.z as f32)).expect("a raft came back");
        assert_eq!(body, (12.25, 62.88, -40.5), "the raft moved while the world was closed");
        let back = again.rafts[0].body;
        assert_eq!(back.yaw, 1.2, "the raft came back pointing another way");
        assert!(back.sail, "the sail came back furled");
        assert_eq!(back.sail_angle, -0.8, "the yard came back braced somewhere else");
        assert_eq!(back.speed(), 0.0, "the raft came back already moving");
    }

    #[test]
    fn a_world_saved_before_rafts_has_none_and_is_not_an_error() {
        let dir = TempDir::new("none");
        assert_eq!(Rafts::new().load(&dir.0).expect("no file is no rafts"), 0);
    }

    #[test]
    fn a_world_saved_before_the_sail_had_an_angle_still_has_its_rafts() {
        // The one thing a save-format bump must not do: turn somebody's
        // rafts into an error message with the rafts still inside it. The
        // file is written by hand in the shape version one wrote, because
        // the struct that wrote it no longer exists.
        let dir = TempDir::new("v1");
        #[derive(Serialize)]
        struct OldSave {
            version: u32,
            rafts: Vec<BodyV1Out>,
        }
        #[derive(Serialize)]
        struct BodyV1Out {
            x: f32,
            y: f32,
            z: f32,
            yaw: f32,
            vx: f32,
            vz: f32,
            spin: f32,
            sail: bool,
        }
        let old = OldSave {
            version: 1,
            rafts: vec![
                BodyV1Out { x: 1.5, y: 62.0, z: -2.5, yaw: 0.75, vx: 0.0, vz: 0.0, spin: 0.0, sail: true },
                BodyV1Out { x: 40.0, y: 62.0, z: 8.0, yaw: 0.0, vx: 0.0, vz: 0.0, spin: 0.0, sail: false },
            ],
        };
        std::fs::create_dir_all(&dir.0).expect("dir");
        std::fs::write(Rafts::save_path(&dir.0), bincode::serialize(&old).expect("serialize")).expect("write");

        let mut rafts = Rafts::new();
        assert_eq!(rafts.load(&dir.0).expect("a version one file was refused"), 2);
        let first = rafts.rafts[0].body;
        assert_eq!((first.x, first.z, first.yaw), (1.5, -2.5, 0.75), "the raft came back somewhere else");
        assert!(first.sail, "the sail came back furled");
        assert_eq!(first.sail_angle, 0.0, "a yard nobody has touched came back braced round");
        // ...and what this build writes is read back as this build's.
        assert_eq!(rafts.save(&dir.0).expect("save"), 2);
        assert_eq!(Rafts::new().load(&dir.0).expect("load"), 2);
        // A version from the future is still a refusal rather than a guess.
        std::fs::write(Rafts::save_path(&dir.0), bincode::serialize(&SaveFile { version: 99, rafts: Vec::new() }).expect("serialize")).expect("write");
        assert!(Rafts::new().load(&dir.0).is_err(), "a file from a later build was read anyway");
    }

    #[test]
    fn the_yard_is_braced_where_it_is_asked_and_never_through_its_own_mast() {
        let mut rafts = Rafts::new();
        let id = rafts.launch(Body::at_rest(0.0, 10.0, 0.0, 0.0)).expect("launch");
        assert_eq!(rafts.trim(id, 0.5), Some(0.5));
        assert_eq!(rafts.get(id).expect("raft").body.sail_angle, 0.5);
        // What the server answers is the clamped angle, so a client
        // predicting with an angle the mast refuses is told the real one
        // rather than being left to tremble against it.
        assert_eq!(rafts.trim(id, 99.0), Some(raft::SAIL_MAX_ANGLE));
        assert_eq!(rafts.trim(id, f32::NAN), Some(0.0));
        assert!(rafts.get(id).expect("raft").body.is_sane(), "a NaN off the wire reached the raft");
        assert_eq!(rafts.trim(id + 1, 0.2), None, "a raft that does not exist was trimmed");
    }

    #[test]
    fn one_person_rows_a_raft_and_a_passengers_oars_move_nothing() {
        let mut rafts = Rafts::new();
        let id = rafts.launch(Body::at_rest(0.0, 10.0, 0.0, 0.0)).expect("launch");
        let now = Instant::now();
        assert!(rafts.take_oars(id, 1).is_ok());
        assert!(rafts.take_oars(id, 2).is_err(), "two people at one pair of oars");
        assert!(!rafts.row(id, 2, Oars { stroke: 1.0, turn: 0.0 }, now), "a passenger rowed");
        assert!(rafts.row(id, 1, Oars { stroke: 1.0, turn: 0.0 }, now));
        assert!(rafts.get(id).expect("raft").oars_at(now).pulling());
        assert_eq!(rafts.let_go(1), Some(id));
        assert!(!rafts.get(id).expect("raft").oars_at(now).pulling(), "the oars pull with nobody at them");
    }

    #[test]
    fn oars_nobody_is_sending_stop_pulling() {
        let mut rafts = Rafts::new();
        let id = rafts.launch(Body::at_rest(0.0, 10.0, 0.0, 0.0)).expect("launch");
        let now = Instant::now();
        rafts.take_oars(id, 7).expect("oars");
        rafts.row(id, 7, Oars { stroke: 1.0, turn: 0.0 }, now);
        let later = now + OARS_TIMEOUT + Duration::from_millis(100);
        assert!(!rafts.get(id).expect("raft").oars_at(later).pulling(), "a ghost at the oars");
    }

    #[test]
    fn a_raft_comes_apart_after_enough_blows_and_not_before() {
        let mut rafts = Rafts::new();
        let id = rafts.launch(Body::at_rest(5.0, 10.0, 5.0, 0.0)).expect("launch");
        for _ in 1..raft::HITS_TO_BREAK {
            assert_eq!(rafts.strike(id, 1), Struck::Hit);
        }
        assert!(matches!(rafts.strike(id, 1), Struck::Broke(body) if body.x == 5.0));
        assert!(rafts.is_empty());
        assert_eq!(rafts.strike(id, 1), Struck::Missed);
    }

    #[test]
    fn a_raft_is_not_launched_on_top_of_another() {
        let mut rafts = Rafts::new();
        assert!(rafts.launch(Body::at_rest(0.0, 10.0, 0.0, 0.0)).is_some());
        assert!(rafts.launch(Body::at_rest(1.0, 10.0, 1.0, 0.0)).is_none());
        assert!(rafts.launch(Body::at_rest(6.0, 10.0, 0.0, 0.0)).is_some());
    }
}
