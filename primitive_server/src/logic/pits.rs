//! Fires in the ground, on the server: what is in each pit kiln and log
//! pile, how long each has left to burn, and what puts them out.
//!
//! The rules both sides agree on -- the stages, what a pit is, what counts
//! as cover, how long the hour is and why -- are in `primitive_shared::pit`,
//! with TerraFirmaCraft's versions of all three structures. This is the
//! part only the authority has: the clock, the contents and the gestures.
//!
//! ## Why the gestures are decided here and not in `use_block`
//!
//! A pit kiln is built in seventeen right clicks, and each of them has a
//! refusal that has to say *why* -- seven logs, a side open, fired pots
//! still in it, rain. Written inline in the server's use gesture that is
//! a page of branches nobody can test without a socket. So a gesture here
//! is a function of the world, the cell and what is in the hand, and it
//! answers an [`Outcome`]: what left the hand, what goes back to the pack,
//! what the cells became and what the player is told. The server does the
//! pack and the broadcast; everything that could be wrong is here, under
//! unit tests.
//!
//! ## Why the state is its own map and its own file
//!
//! The fires' reasons exactly (`logic::fire`): the counts live in the block
//! id, which the world already saves and sends, and what does not fit in an
//! id -- which pots, which logs, how many seconds -- lives in a map keyed by
//! position, written to `pits.bin` beside the world. A world saved before
//! pits existed has no such file, which reads as "nothing is in any pit",
//! which is right.
//!
//! **Dirty for as long as anything burns.** The fires let their fuel burn
//! down without marking the map, because a fire that comes back after a
//! crash with a few more minutes of wood is a fire nobody notices. A kiln
//! that came back with its whole hour is a kiln whose player was robbed of
//! the hour they already waited, so while anything is burning the autosave
//! writes the file -- a few dozen bytes a kiln.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use primitive_shared::pit::{
    self, burnt_pile, fires_into, log_pile, log_pile_lit, pile_logs, Stage, CHARCOAL_SECONDS,
    FIBRE_NEEDED, LOGS_NEEDED, OPEN_PILE_SECONDS, PILE_LOGS_MAX, PIT_KILN_SECONDS, POTTERY_MAX,
    RAIN_PUTS_OUT_SECONDS,
};
use primitive_shared::protocol::BlockChange;
use primitive_shared::types::{block_kind, BlockId, BLOCK_AIR, BLOCK_ASH, BLOCK_FIBER, BLOCK_LOG, BLOCK_LOG_PILE, BLOCK_LOG_PILE_LIT};
use primitive_shared::weather::Weather;

use crate::logic::falling::BlockWorld;
use crate::logic::fire::STRIKER;

/// An unlit pile of `logs`, drawn in the wood most of its logs are.
///
/// **The block has to say it, because the client never sees the list.** What
/// went into a pile is the server's (`Pile::logs`), and the picture of a pile
/// is the client's; one wood in the id is how a stack of birch comes to look
/// like birch. The most of, and the latest on a tie, so a pile of oak with
/// one birch log on it still reads as oak.
fn pile_in_wood(logs: u8, kinds: &[BlockId]) -> BlockId {
    let woods = &primitive_shared::wood::WOODS;
    let mut count = [0u8; 8];
    let mut best = 0usize;
    for &kind in kinds {
        if let Some(wood) = woods.iter().position(|w| w.log == block_kind(kind)) {
            count[wood] += 1;
            if count[wood] >= count[best] {
                best = wood;
            }
        }
    }
    primitive_shared::types::in_wood(log_pile(logs), best)
}

/// Its own version, independent of the world's.
const SAVE_FORMAT_VERSION: u32 = 1;

/// Where a pit is, in global block coordinates.
pub type PitPos = (i32, i32, i32);

/// How many piles one strike can light through their shared faces.
///
/// A bound on the walk, not a rule a player meets: a charcoal pit of
/// sixty-four piles is a clearing's worth of wood. Past it the rest still
/// catch -- a burning pile lights the piles it touches every tick -- a
/// tick at a time.
const SPREAD_MAX: usize = 64;

/// What is in one kiln.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Kiln {
    /// The pieces on the floor, raw or fired, in the order they went in.
    pottery: Vec<BlockId>,
    /// The logs over the fibre, by kind, so a kiln broken open before it is
    /// lit gives back the birch that went into it and not oak.
    logs: Vec<BlockId>,
    /// Seconds left, and how long the rain has been on it. `None` while it
    /// is being built, or once it has burnt.
    burn: Option<Burn>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct Burn {
    left: f64,
    wet: f32,
}

/// What is in one pile of logs.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Pile {
    logs: Vec<BlockId>,
    burn: Option<PileBurn>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct PileBurn {
    left: f64,
    /// How long a face has been open to the air, running.
    open: f32,
}

/// What a gesture did.
#[derive(Debug, Default, PartialEq)]
pub struct Outcome {
    /// One of this left the hand -- the striker too, when a strike took:
    /// see `fire::STRIKER` for why a nodule is spent on every fire.
    pub spent: Option<BlockId>,
    /// One of this goes back into the pack.
    pub returned: Option<BlockId>,
    /// Cells that changed, and what they became.
    pub wrote: Vec<(PitPos, BlockId)>,
    /// What the player is told.
    pub said: Option<String>,
}

impl Outcome {
    fn say(text: impl Into<String>) -> Outcome {
        Outcome {
            said: Some(text.into()),
            ..Outcome::default()
        }
    }
}

/// What a tick did: the cells that changed, and the news for whoever is
/// near each of them.
#[derive(Debug, Default)]
pub struct Stepped {
    pub changes: Vec<BlockChange>,
    pub news: Vec<(PitPos, &'static str)>,
}

#[derive(Serialize, Deserialize)]
struct SaveFile {
    version: u32,
    kilns: Vec<(PitPos, Kiln)>,
    piles: Vec<(PitPos, Pile)>,
}

/// Every pit kiln and log pile with something in it.
#[derive(Default)]
pub struct Pits {
    kilns: HashMap<PitPos, Kiln>,
    piles: HashMap<PitPos, Pile>,
    /// Cells that changed and have not been looked at yet: the
    /// `CellMechanic` contract the fires follow, so a burning kiln the
    /// world was born with (the test world has one) is found and timed.
    pending: Vec<PitPos>,
    weather: Weather,
    dirty: bool,
}

fn change((x, y, z): PitPos, block: BlockId) -> BlockChange {
    BlockChange {
        global_x: x,
        global_y: y,
        global_z: z,
        block_id: block,
    }
}

impl Pits {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn is_empty(&self) -> bool {
        self.kilns.is_empty() && self.piles.is_empty()
    }

    pub fn set_weather(&mut self, weather: Weather) {
        self.weather = weather;
    }

    /// The pottery in a kiln, in the order it went in.
    pub fn pottery(&self, at: PitPos) -> &[BlockId] {
        self.kilns.get(&at).map_or(&[], |kiln| kiln.pottery.as_slice())
    }

    /// Seconds left on a burning kiln.
    pub fn kiln_seconds_left(&self, at: PitPos) -> Option<f64> {
        self.kilns.get(&at).and_then(|kiln| kiln.burn).map(|burn| burn.left)
    }

    /// Seconds left on a burning pile.
    pub fn pile_seconds_left(&self, at: PitPos) -> Option<f64> {
        self.piles.get(&at).and_then(|pile| pile.burn).map(|burn| burn.left)
    }

    /// Puts pottery into a kiln from outside the gestures: the test world's
    /// stock. Refuses what is not pottery and anything past four.
    pub fn stock_kiln(&mut self, at: PitPos, pottery: &[BlockId]) {
        let kiln = self.kilns.entry(at).or_default();
        for &piece in pottery {
            if kiln.pottery.len() < POTTERY_MAX as usize
                && (pit::is_raw_pottery(piece) || fires_into(piece).is_none() && is_fired(piece))
            {
                kiln.pottery.push(piece);
            }
        }
        self.dirty = true;
    }

    /// A cell changed. Queue it; the world is not ours to read here.
    pub fn on_block_changed(&mut self, x: i32, y: i32, z: i32) {
        self.pending.push((x, y, z));
    }

    // ---- the pit kiln ----

    /// A right click on a pit kiln, or with pottery on the floor of an
    /// empty pit. `at` is the pit's own cell in both cases: the server asks
    /// `pit::takes_pottery_above` of the floor before it calls this.
    pub fn use_kiln(&mut self, world: &dyn BlockWorld, at: PitPos, held: Option<BlockId>) -> Outcome {
        let look = |x, y, z| world.block(x, y, z);
        let Some(block) = world.block(at.0, at.1, at.2) else {
            return Outcome::default();
        };
        let held_kind = held.map(block_kind);
        let Some(stage) = Stage::of(block) else {
            // An empty pit, with pottery in the hand.
            return match held {
                Some(piece) if pit::is_raw_pottery(piece) && block_kind(block) == BLOCK_AIR => {
                    match pit::breach(look, at) {
                        Ok(None) => {}
                        Ok(Some(breach)) => return Outcome::say(breach.says()),
                        Err(pit::Unseen) => return Outcome::default(),
                    }
                    let kiln = self.kilns.entry(at).or_default();
                    *kiln = Kiln::default();
                    kiln.pottery.push(block_kind(piece));
                    self.dirty = true;
                    let wrote = Stage::Pottery { pieces: 1, fired: false }.block();
                    world.set(at.0, at.1, at.2, wrote);
                    Outcome {
                        spent: Some(block_kind(piece)),
                        wrote: vec![(at, wrote)],
                        ..Outcome::default()
                    }
                }
                _ => Outcome::default(),
            };
        };

        let write = |pits: &mut Pits, stage: Stage| -> (PitPos, BlockId) {
            let block = stage.block();
            world.set(at.0, at.1, at.2, block);
            pits.dirty = true;
            (at, block)
        };
        let striking = held_kind == Some(STRIKER);

        match stage {
            Stage::Pottery { pieces, fired } => {
                if let Some(piece) = held.filter(|&piece| pit::is_raw_pottery(piece)) {
                    if fired {
                        return Outcome::say("take the fired pottery out of the pit first");
                    }
                    let kiln = self.kilns.entry(at).or_default();
                    if kiln.pottery.len() >= POTTERY_MAX as usize {
                        return Outcome::say("a pit kiln holds four pieces of pottery");
                    }
                    kiln.pottery.push(block_kind(piece));
                    let count = kiln.pottery.len() as u8;
                    let wrote = write(self, Stage::Pottery { pieces: count, fired: false });
                    return Outcome {
                        spent: Some(block_kind(piece)),
                        wrote: vec![wrote],
                        ..Outcome::default()
                    };
                }
                if held.is_some_and(pit::is_fibre) {
                    if fired {
                        return Outcome::say("take the fired pottery out of the pit first");
                    }
                    let wrote = write(self, Stage::Fibre(1));
                    return Outcome {
                        spent: Some(BLOCK_FIBER),
                        wrote: vec![wrote],
                        ..Outcome::default()
                    };
                }
                if held.is_none() {
                    // Taking a piece back out: the last one in.
                    let kiln = self.kilns.entry(at).or_default();
                    let returned = kiln.pottery.pop();
                    let left = kiln.pottery.len() as u8;
                    let wrote = if left == 0 {
                        self.kilns.remove(&at);
                        world.set(at.0, at.1, at.2, BLOCK_AIR);
                        self.dirty = true;
                        (at, BLOCK_AIR)
                    } else {
                        write(self, Stage::Pottery { pieces: left, fired })
                    };
                    return Outcome {
                        returned,
                        wrote: vec![wrote],
                        ..Outcome::default()
                    };
                }
                let _ = pieces;
                if held.is_some_and(pit::is_log) || striking {
                    return Outcome::say(format!(
                        "{FIBRE_NEEDED} fibre go over the pottery first, then {LOGS_NEEDED} logs, then it is lit"
                    ));
                }
                Outcome::default()
            }
            Stage::Fibre(fibre) => {
                if held.is_some_and(pit::is_fibre) {
                    if fibre >= FIBRE_NEEDED {
                        return Outcome::say(format!("the fibre is packed in: {LOGS_NEEDED} logs go on top now"));
                    }
                    let wrote = write(self, Stage::Fibre(fibre + 1));
                    return Outcome {
                        spent: Some(BLOCK_FIBER),
                        wrote: vec![wrote],
                        ..Outcome::default()
                    };
                }
                if let Some(log) = held.filter(|&log| pit::is_log(log)) {
                    if fibre < FIBRE_NEEDED {
                        return Outcome::say(format!(
                            "the pit needs {FIBRE_NEEDED} fibre before the logs go on ({fibre} now)"
                        ));
                    }
                    self.kilns.entry(at).or_default().logs = vec![block_kind(log)];
                    let wrote = write(self, Stage::Logs(1));
                    return Outcome {
                        spent: Some(block_kind(log)),
                        wrote: vec![wrote],
                        ..Outcome::default()
                    };
                }
                if striking || held.is_none() {
                    return Outcome::say(format!(
                        "a pit kiln is lit with {FIBRE_NEEDED} fibre and {LOGS_NEEDED} logs in it ({fibre} fibre, 0 logs now)"
                    ));
                }
                Outcome::default()
            }
            Stage::Logs(logs) => {
                if let Some(log) = held.filter(|&log| pit::is_log(log)) {
                    if logs >= LOGS_NEEDED {
                        return Outcome::say("the pit kiln is full: strike it with flint to light it");
                    }
                    self.kilns.entry(at).or_default().logs.push(block_kind(log));
                    let wrote = write(self, Stage::Logs(logs + 1));
                    return Outcome {
                        spent: Some(block_kind(log)),
                        wrote: vec![wrote],
                        ..Outcome::default()
                    };
                }
                if striking {
                    if logs < LOGS_NEEDED {
                        return Outcome::say(format!(
                            "a pit kiln is lit with {LOGS_NEEDED} logs on it ({logs} now)"
                        ));
                    }
                    return self.light_kiln(world, at);
                }
                if held.is_none() {
                    return Outcome::say(format!("{logs} of {LOGS_NEEDED} logs on the pit kiln"));
                }
                Outcome::default()
            }
            Stage::Burning => {
                let minutes = self
                    .kiln_seconds_left(at)
                    .map_or(60, |left| (left / 60.0).ceil().max(1.0) as u32);
                Outcome::say(format!("the pit kiln is burning: {minutes} minutes until the pottery is fired"))
            }
        }
    }

    /// Strikes a full kiln.
    fn light_kiln(&mut self, world: &dyn BlockWorld, at: PitPos) -> Outcome {
        let look = |x, y, z| world.block(x, y, z);
        match pit::breach(look, at) {
            Ok(None) => {}
            Ok(Some(breach)) => return Outcome::say(breach.says()),
            Err(pit::Unseen) => return Outcome::default(),
        }
        // Refused rather than lit and drowned: striking a kiln in the rain
        // is spending sixteen armfuls on twenty seconds, and the player
        // asked for none of that.
        if self.weather.is_wet() && pit::open_to_the_sky(look, at) {
            return Outcome::say("it is raining on the pit: a pit kiln in the open goes out in the rain");
        }
        let kiln = self.kilns.entry(at).or_default();
        if kiln.pottery.is_empty() {
            return Outcome::say("there is nothing in the pit to fire");
        }
        kiln.burn = Some(Burn {
            left: PIT_KILN_SECONDS,
            wet: 0.0,
        });
        let lit = Stage::Burning.block();
        world.set(at.0, at.1, at.2, lit);
        self.dirty = true;
        Outcome {
            // The nodule is spent on a strike that took, and only then --
            // every refusal above returned before it. See `fire::STRIKER`.
            spent: Some(STRIKER),
            wrote: vec![(at, lit)],
            said: Some("the pit kiln is alight: in an hour the pottery is fired".to_string()),
            ..Outcome::default()
        }
    }

    // ---- the log pile ----

    /// Lays one log as a new pile.
    pub fn lay_pile(&mut self, world: &dyn BlockWorld, at: PitPos, held: Option<BlockId>) -> Outcome {
        let Some(log) = held.filter(|&log| pit::is_log(log)) else {
            return Outcome::default();
        };
        if !pit::pile_fits(|x, y, z| world.block(x, y, z), at) {
            return Outcome::say("a log pile needs an empty cell with a floor under it");
        }
        self.piles.insert(
            at,
            Pile {
                logs: vec![block_kind(log)],
                burn: None,
            },
        );
        let block = pile_in_wood(1, &[block_kind(log)]);
        world.set(at.0, at.1, at.2, block);
        self.dirty = true;
        Outcome {
            spent: Some(block_kind(log)),
            wrote: vec![(at, block)],
            ..Outcome::default()
        }
    }

    /// A right click on a pile.
    pub fn use_pile(&mut self, world: &dyn BlockWorld, at: PitPos, held: Option<BlockId>) -> Outcome {
        let Some(block) = world.block(at.0, at.1, at.2) else {
            return Outcome::default();
        };
        let Some(logs) = pile_logs(block) else {
            return Outcome::default();
        };
        if block_kind(block) == BLOCK_LOG_PILE_LIT {
            let covered = pit::pile_covered(|x, y, z| world.block(x, y, z), at) == Ok(true);
            if !covered {
                return Outcome::say(
                    "the pile is burning open to the air: cover every face with earth or stone, or it burns to ash",
                );
            }
            let minutes = self
                .pile_seconds_left(at)
                .map_or(60, |left| (left / 60.0).ceil().max(1.0) as u32);
            return Outcome::say(format!("the charcoal pit is burning: {minutes} minutes to go"));
        }
        if let Some(log) = held.filter(|&log| pit::is_log(log)) {
            if logs >= PILE_LOGS_MAX {
                return Outcome::say(format!("a log pile holds {PILE_LOGS_MAX} logs"));
            }
            let pile = self.piles.entry(at).or_default();
            if pile.logs.is_empty() {
                pile.logs = vec![BLOCK_LOG; logs as usize];
            }
            pile.logs.push(block_kind(log));
            let wrote = pile_in_wood(logs + 1, &pile.logs);
            world.set(at.0, at.1, at.2, wrote);
            self.dirty = true;
            return Outcome {
                spent: Some(block_kind(log)),
                wrote: vec![(at, wrote)],
                ..Outcome::default()
            };
        }
        if held.map(block_kind) == Some(STRIKER) {
            let wrote = self.light_piles(world, at);
            return Outcome {
                // Spent only if something caught: a strike at a pile that is
                // already alight lights nothing and costs nothing.
                spent: (!wrote.is_empty()).then_some(STRIKER),
                said: Some(format!(
                    "the log pile is alight: cover it within {} seconds, every face, or it burns to ash",
                    OPEN_PILE_SECONDS as u32
                )),
                wrote,
                ..Outcome::default()
            };
        }
        Outcome::default()
    }

    /// Lights a pile and every unlit pile joined to it through its faces.
    fn light_piles(&mut self, world: &dyn BlockWorld, from: PitPos) -> Vec<(PitPos, BlockId)> {
        let mut wrote = Vec::new();
        let mut queue = vec![from];
        while let Some(at) = queue.pop() {
            if wrote.len() >= SPREAD_MAX {
                break;
            }
            let Some(block) = world.block(at.0, at.1, at.2) else {
                continue;
            };
            if block_kind(block) != BLOCK_LOG_PILE {
                continue;
            }
            let logs = pile_logs(block).unwrap_or(1);
            let lit = log_pile_lit(logs);
            world.set(at.0, at.1, at.2, lit);
            let pile = self.piles.entry(at).or_default();
            if pile.logs.is_empty() {
                pile.logs = vec![BLOCK_LOG; logs as usize];
            }
            pile.burn = Some(PileBurn {
                left: CHARCOAL_SECONDS,
                open: 0.0,
            });
            wrote.push((at, lit));
            for (dx, dy, dz) in NEIGHBOURS {
                queue.push((at.0 + dx, at.1 + dy, at.2 + dz));
            }
        }
        self.dirty = true;
        wrote
    }

    // ---- breaking ----

    /// A pit kiln or a pile has been broken. Forgets it, and answers what
    /// comes out of it -- the pottery, the fibre and the logs that went in,
    /// as they are. What was burning is burnt: a lit kiln gives back its
    /// pottery unfired and nothing else, a lit pile gives nothing.
    pub fn broken(&mut self, at: PitPos, block: BlockId) -> Vec<(BlockId, u32)> {
        let mut out: Vec<(BlockId, u32)> = Vec::new();
        if let Some(stage) = Stage::of(block) {
            let kiln = self.kilns.remove(&at).unwrap_or_default();
            for piece in kiln.pottery {
                out.push((piece, 1));
            }
            if stage != Stage::Burning {
                if stage.fibre() > 0 {
                    out.push((BLOCK_FIBER, u32::from(stage.fibre())));
                }
                let logs = stage.logs() as usize;
                let mut kinds = kiln.logs;
                kinds.resize(logs, BLOCK_LOG);
                for log in kinds {
                    out.push((log, 1));
                }
            }
            self.dirty = true;
        } else if let Some(logs) = pile_logs(block) {
            let pile = self.piles.remove(&at).unwrap_or_default();
            if block_kind(block) == BLOCK_LOG_PILE {
                let mut kinds = pile.logs;
                kinds.resize(logs as usize, BLOCK_LOG);
                for log in kinds {
                    out.push((log, 1));
                }
            }
            self.dirty = true;
        }
        out
    }

    // ---- the clock ----

    /// One tick: burns every lit kiln and pile down, puts out what the
    /// rain or a broken wall reaches, and finishes what has burnt through.
    ///
    /// `dt` is seconds the server has been running since the last call --
    /// the definition of the hour, see `pit::PIT_KILN_SECONDS` -- and it
    /// may be any size: a test hands it an hour at once.
    pub fn step(&mut self, world: &dyn BlockWorld, dt: f64, budget: usize) -> Stepped {
        let mut stepped = Stepped::default();
        let look = |x, y, z| world.block(x, y, z);

        // Reconcile what the world says with what the map thinks, on the
        // fires' terms: a lit cell the map has never seen was lit before
        // the map existed -- the test world's -- and gets a whole burn; a
        // cell in the map that is no longer a pit has been replaced.
        let batch: Vec<PitPos> = self.pending.drain(..budget.min(self.pending.len())).collect();
        for at in batch {
            let Some(block) = world.block(at.0, at.1, at.2) else {
                continue;
            };
            if Stage::of(block) == Some(Stage::Burning) {
                let kiln = self.kilns.entry(at).or_default();
                if kiln.burn.is_none() {
                    kiln.burn = Some(Burn { left: PIT_KILN_SECONDS, wet: 0.0 });
                    self.dirty = true;
                }
            } else if !pit::is_pit_kiln(block) && self.kilns.remove(&at).is_some() {
                self.dirty = true;
            }
            if block_kind(block) == BLOCK_LOG_PILE_LIT {
                let pile = self.piles.entry(at).or_default();
                if pile.burn.is_none() {
                    pile.burn = Some(PileBurn { left: CHARCOAL_SECONDS, open: 0.0 });
                    self.dirty = true;
                }
            } else if !pit::is_log_pile(block) && self.piles.remove(&at).is_some() {
                self.dirty = true;
            }
        }

        let dt_f32 = dt as f32;
        let raining = self.weather.is_wet();
        let mut burning = false;

        // ---- kilns ----
        let mut done: Vec<(PitPos, bool, &'static str)> = Vec::new();
        for (&at, kiln) in self.kilns.iter_mut() {
            let Some(burn) = kiln.burn.as_mut() else {
                continue;
            };
            burning = true;
            let Some(block) = world.block(at.0, at.1, at.2) else {
                // Nobody has the chunk: the hour goes on, and the ending
                // waits for somebody to come and see it.
                burn.left = (burn.left - dt).max(0.0);
                continue;
            };
            if Stage::of(block) != Some(Stage::Burning) {
                kiln.burn = None;
                continue;
            }
            // A wall nobody can see (`pit::Unseen`) is not a broken wall.
            if let Ok(Some(breach)) = pit::breach(look, at) {
                done.push((at, false, match breach {
                    pit::Breach::Covered => "the pit kiln went out: something was laid over it and smothered it",
                    _ => "the pit kiln went out: its pit was broken open",
                }));
                continue;
            }
            if raining && pit::open_to_the_sky(look, at) {
                let was = burn.wet;
                burn.wet += dt_f32;
                if burn.wet >= RAIN_PUTS_OUT_SECONDS {
                    done.push((at, false, "the rain put the pit kiln out"));
                    continue;
                }
                if was == 0.0 {
                    stepped.news.push((at, "rain is falling on the pit kiln: it will go out"));
                }
            } else {
                burn.wet = 0.0;
            }
            burn.left -= dt;
            if burn.left <= 0.0 {
                done.push((at, true, "the pit kiln has burnt down: the pottery is fired"));
            }
        }
        for (at, fired, news) in done {
            let Some(kiln) = self.kilns.get_mut(&at) else {
                continue;
            };
            kiln.burn = None;
            kiln.logs.clear();
            if fired {
                for piece in kiln.pottery.iter_mut() {
                    if let Some(hard) = fires_into(*piece) {
                        *piece = hard;
                    }
                }
            }
            let block = if kiln.pottery.is_empty() {
                self.kilns.remove(&at);
                BLOCK_AIR
            } else {
                Stage::Pottery {
                    pieces: kiln.pottery.len() as u8,
                    fired,
                }
                .block()
            };
            world.set(at.0, at.1, at.2, block);
            stepped.changes.push(change(at, block));
            stepped.news.push((at, news));
            self.dirty = true;
        }

        // ---- piles ----
        let mut catches: Vec<PitPos> = Vec::new();
        let mut burnt: Vec<(PitPos, BlockId, &'static str)> = Vec::new();
        for (&at, pile) in self.piles.iter_mut() {
            let Some(burn) = pile.burn.as_mut() else {
                continue;
            };
            burning = true;
            let Some(block) = world.block(at.0, at.1, at.2) else {
                burn.left = (burn.left - dt).max(0.0);
                continue;
            };
            if block_kind(block) != BLOCK_LOG_PILE_LIT {
                pile.burn = None;
                continue;
            }
            let logs = pile_logs(block).unwrap_or(1);
            // A burning pile lights the piles it touches: TerraFirmaCraft's
            // spread, and what makes a pit of many piles one burn.
            for (dx, dy, dz) in NEIGHBOURS {
                let next = (at.0 + dx, at.1 + dy, at.2 + dz);
                if world.block(next.0, next.1, next.2).is_some_and(|b| block_kind(b) == BLOCK_LOG_PILE) {
                    catches.push(next);
                }
            }
            // A face nobody has loaded counts as covered, for the kiln's
            // reason: a pile must not burn to ash for a wall it cannot see.
            let covered = pit::pile_covered(look, at).unwrap_or(true);
            if covered {
                burn.open = 0.0;
            } else {
                let was = burn.open;
                burn.open += dt_f32;
                if burn.open >= OPEN_PILE_SECONDS {
                    burnt.push((at, BLOCK_ASH, "a log pile left open to the air has burnt to ash"));
                    continue;
                }
                if was == 0.0 && burn.left < CHARCOAL_SECONDS {
                    stepped.news.push((at, "a burning log pile is open to the air: cover it or it burns to ash"));
                }
            }
            burn.left -= dt;
            if burn.left <= 0.0 {
                let left = burnt_pile(logs, covered);
                let news = if left == BLOCK_ASH {
                    "the log pile has burnt down to ash"
                } else {
                    "the charcoal pit has burnt down: dig the charcoal out"
                };
                burnt.push((at, left, news));
            }
        }
        for (at, block, news) in burnt {
            self.piles.remove(&at);
            world.set(at.0, at.1, at.2, block);
            stepped.changes.push(change(at, block));
            stepped.news.push((at, news));
            self.dirty = true;
        }
        for at in catches {
            for (cell, block) in self.light_piles(world, at) {
                stepped.changes.push(change(cell, block));
            }
        }

        if burning {
            self.dirty = true;
        }
        stepped
    }

    // ---- saving ----

    fn save_path(dir: &Path) -> PathBuf {
        dir.join("pits.bin")
    }

    /// Writes them out, atomically, the way the fires are written.
    pub fn save(&mut self, dir: &Path) -> std::io::Result<usize> {
        std::fs::create_dir_all(dir)?;
        let mut kilns: Vec<(PitPos, Kiln)> = self.kilns.iter().map(|(&at, kiln)| (at, kiln.clone())).collect();
        let mut piles: Vec<(PitPos, Pile)> = self.piles.iter().map(|(&at, pile)| (at, pile.clone())).collect();
        kilns.sort_by_key(|(at, _)| *at);
        piles.sort_by_key(|(at, _)| *at);
        let count = kilns.len() + piles.len();
        let bytes = bincode::serialize(&SaveFile {
            version: SAVE_FORMAT_VERSION,
            kilns,
            piles,
        })
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let final_path = Self::save_path(dir);
        let tmp_path = final_path.with_extension("bin.tmp");
        std::fs::write(&tmp_path, &bytes)?;
        std::fs::rename(&tmp_path, &final_path)?;
        self.dirty = false;
        Ok(count)
    }

    /// Reads them back. A missing file is a world with nothing in any pit.
    ///
    /// **A file this build cannot read is an error, said out loud**, and
    /// not quietly an empty map: a kiln is up to four pots somebody made,
    /// and the chests' rule applies to things a player made. The server
    /// prints it and starts with nothing, rather than refusing the world.
    pub fn load(&mut self, dir: &Path) -> std::io::Result<usize> {
        let bytes = match std::fs::read(Self::save_path(dir)) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
            Err(e) => return Err(e),
        };
        let save: SaveFile = bincode::deserialize(&bytes)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        if save.version != SAVE_FORMAT_VERSION {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("pits.bin is version {}, this build reads {SAVE_FORMAT_VERSION}", save.version),
            ));
        }
        // Off a disk an operator can edit: an hour that is not a number
        // would burn for ever, and a fifth pot is a pot out of nothing.
        let sane = |left: f64| if left.is_finite() { left.clamp(0.0, PIT_KILN_SECONDS.max(CHARCOAL_SECONDS)) } else { 0.0 };
        self.kilns = save
            .kilns
            .into_iter()
            .map(|(at, mut kiln)| {
                kiln.pottery.retain(|&piece| pit::is_raw_pottery(piece) || is_fired(piece));
                kiln.pottery.truncate(POTTERY_MAX as usize);
                kiln.logs.retain(|&log| pit::is_log(log));
                kiln.logs.truncate(LOGS_NEEDED as usize);
                if let Some(burn) = kiln.burn.as_mut() {
                    burn.left = sane(burn.left);
                    burn.wet = if burn.wet.is_finite() { burn.wet.max(0.0) } else { 0.0 };
                }
                (at, kiln)
            })
            .collect();
        self.piles = save
            .piles
            .into_iter()
            .map(|(at, mut pile)| {
                pile.logs.retain(|&log| pit::is_log(log));
                pile.logs.truncate(PILE_LOGS_MAX as usize);
                if let Some(burn) = pile.burn.as_mut() {
                    burn.left = sane(burn.left);
                    burn.open = if burn.open.is_finite() { burn.open.max(0.0) } else { 0.0 };
                }
                (at, pile)
            })
            .collect();
        self.dirty = false;
        Ok(self.kilns.len() + self.piles.len())
    }
}

/// The six face neighbours.
const NEIGHBOURS: [(i32, i32, i32); 6] = [(1, 0, 0), (-1, 0, 0), (0, 1, 0), (0, -1, 0), (0, 0, 1), (0, 0, -1)];

/// Is this a piece of pottery that has been fired -- something a kiln that
/// burnt down leaves in the pit?
fn is_fired(block: BlockId) -> bool {
    use primitive_shared::types::{BLOCK_BRICK_RAW, BLOCK_JUG_RAW, BLOCK_MOULD_RAW, BLOCK_VESSEL_RAW};
    [BLOCK_VESSEL_RAW, BLOCK_MOULD_RAW, BLOCK_JUG_RAW, BLOCK_BRICK_RAW]
        .into_iter()
        .filter_map(fires_into)
        .any(|fired| fired == block_kind(block))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logic::falling::tests::TestWorld;
    use primitive_shared::types::{
        BLOCK_BIRCH_LOG, BLOCK_COAL, BLOCK_DIRT, BLOCK_FLINT, BLOCK_JUG, BLOCK_JUG_RAW, BLOCK_STONE, BLOCK_VESSEL,
        BLOCK_VESSEL_RAW,
    };

    const AT: PitPos = (3, 20, -4);

    /// A field of dirt with a one-deep hole at `AT`.
    fn pit_world() -> TestWorld {
        let world = TestWorld::default();
        for x in AT.0 - 3..=AT.0 + 3 {
            for z in AT.2 - 3..=AT.2 + 3 {
                world.put(x, AT.1 - 1, z, BLOCK_DIRT);
                if (x, z) != (AT.0, AT.2) {
                    world.put(x, AT.1, z, BLOCK_DIRT);
                }
            }
        }
        world
    }

    /// Builds a kiln the way a player does: every right click, in order.
    fn build(pits: &mut Pits, world: &TestWorld, pottery: &[BlockId], fibre: u8, logs: u8) {
        for &piece in pottery {
            let outcome = pits.use_kiln(world, AT, Some(piece));
            assert_eq!(outcome.spent, Some(piece), "the pottery was refused: {:?}", outcome.said);
        }
        for _ in 0..fibre {
            let outcome = pits.use_kiln(world, AT, Some(BLOCK_FIBER));
            assert_eq!(outcome.spent, Some(BLOCK_FIBER), "fibre was refused: {:?}", outcome.said);
        }
        for _ in 0..logs {
            let outcome = pits.use_kiln(world, AT, Some(BLOCK_LOG));
            assert_eq!(outcome.spent, Some(BLOCK_LOG), "a log was refused: {:?}", outcome.said);
        }
    }

    #[test]
    fn a_pit_kiln_with_pottery_eight_hay_and_eight_logs_fires_its_pottery_after_an_hour_and_not_a_minute_before() {
        let world = pit_world();
        let mut pits = Pits::new();
        build(&mut pits, &world, &[BLOCK_VESSEL_RAW, BLOCK_JUG_RAW], 8, 8);
        assert_eq!(Stage::of(world.get(AT.0, AT.1, AT.2)), Some(Stage::Logs(8)));

        let lit = pits.use_kiln(&world, AT, Some(BLOCK_FLINT));
        assert_eq!(lit.spent, Some(BLOCK_FLINT), "a strike that lit the kiln did not spend the nodule");
        assert_eq!(Stage::of(world.get(AT.0, AT.1, AT.2)), Some(Stage::Burning), "{:?}", lit.said);

        // Fifty-nine minutes, in the ticks a server takes and then all at
        // once: neither may fire anything.
        for _ in 0..600 {
            pits.step(&world, 0.05, 64);
        }
        pits.step(&world, PIT_KILN_SECONDS - 60.0 - 30.0, 64);
        assert_eq!(Stage::of(world.get(AT.0, AT.1, AT.2)), Some(Stage::Burning), "it finished a minute early");
        assert_eq!(pits.pottery(AT), [BLOCK_VESSEL_RAW, BLOCK_JUG_RAW]);

        let stepped = pits.step(&world, 60.0, 64);
        assert_eq!(
            Stage::of(world.get(AT.0, AT.1, AT.2)),
            Some(Stage::Pottery { pieces: 2, fired: true }),
            "an hour went by and nothing was fired"
        );
        assert_eq!(stepped.changes.len(), 1, "the fired kiln was not announced");
        assert_eq!(pits.pottery(AT), [BLOCK_VESSEL, BLOCK_JUG]);

        // The fibre and the logs are gone, and the pots come out by hand.
        let first = pits.use_kiln(&world, AT, None);
        assert_eq!(first.returned, Some(BLOCK_JUG));
        let second = pits.use_kiln(&world, AT, None);
        assert_eq!(second.returned, Some(BLOCK_VESSEL));
        assert_eq!(world.get(AT.0, AT.1, AT.2), BLOCK_AIR, "the empty pit is not a hole again");
    }

    #[test]
    fn a_pit_kiln_cannot_be_lit_with_seven_logs() {
        let world = pit_world();
        let mut pits = Pits::new();
        build(&mut pits, &world, &[BLOCK_VESSEL_RAW], 8, 7);
        let struck = pits.use_kiln(&world, AT, Some(BLOCK_FLINT));
        assert_eq!(Stage::of(world.get(AT.0, AT.1, AT.2)), Some(Stage::Logs(7)), "seven logs caught");
        assert!(struck.said.is_some_and(|said| said.contains("7")), "the refusal did not say how many");
        // ...and fibre short is the same refusal one step earlier: logs do
        // not go onto seven fibre at all.
        let world = pit_world();
        let mut pits = Pits::new();
        build(&mut pits, &world, &[BLOCK_VESSEL_RAW], 7, 0);
        assert_eq!(pits.use_kiln(&world, AT, Some(BLOCK_LOG)).spent, None, "a log went onto seven fibre");
    }

    #[test]
    fn a_kiln_whose_pit_is_not_enclosed_cannot_be_lit() {
        let world = pit_world();
        let mut pits = Pits::new();
        build(&mut pits, &world, &[BLOCK_VESSEL_RAW], 8, 8);
        // A wall dug away after the logs went on.
        world.put(AT.0 + 1, AT.1, AT.2, BLOCK_AIR);
        let struck = pits.use_kiln(&world, AT, Some(BLOCK_FLINT));
        assert_eq!(Stage::of(world.get(AT.0, AT.1, AT.2)), Some(Stage::Logs(8)), "an open pit caught");
        assert_eq!(struck.said.as_deref(), Some(pit::Breach::Wall.says()));

        // And pottery never goes into a hole with a side open to begin with.
        let open = pit_world();
        open.put(AT.0, AT.1, AT.2 + 1, BLOCK_AIR);
        let mut pits = Pits::new();
        assert_eq!(pits.use_kiln(&open, AT, Some(BLOCK_VESSEL_RAW)).spent, None);
    }

    #[test]
    fn rain_puts_out_an_open_pit_kiln_and_a_roof_two_up_keeps_it_burning() {
        let world = pit_world();
        let mut pits = Pits::new();
        build(&mut pits, &world, &[BLOCK_VESSEL_RAW], 8, 8);
        pits.use_kiln(&world, AT, Some(BLOCK_FLINT));
        pits.set_weather(Weather::Rain);
        let mut news = Vec::new();
        for _ in 0..30 {
            news.extend(pits.step(&world, 1.0, 64).news);
        }
        assert_eq!(
            Stage::of(world.get(AT.0, AT.1, AT.2)),
            Some(Stage::Pottery { pieces: 1, fired: false }),
            "the rain did not put it out, or fired the pottery"
        );
        assert!(news.iter().any(|(_, said)| said.contains("rain")), "nobody was told why");
        assert_eq!(pits.pottery(AT), [BLOCK_VESSEL_RAW], "the pottery was lost with the fire");

        let roofed = pit_world();
        roofed.put(AT.0, AT.1 + 2, AT.2, BLOCK_STONE);
        let mut pits = Pits::new();
        build(&mut pits, &roofed, &[BLOCK_VESSEL_RAW], 8, 8);
        pits.use_kiln(&roofed, AT, Some(BLOCK_FLINT));
        pits.set_weather(Weather::Storm);
        for _ in 0..60 {
            pits.step(&roofed, 1.0, 64);
        }
        assert_eq!(Stage::of(roofed.get(AT.0, AT.1, AT.2)), Some(Stage::Burning), "a roof kept no rain off");
    }

    #[test]
    fn taking_a_wall_away_from_a_burning_kiln_puts_it_out() {
        let world = pit_world();
        let mut pits = Pits::new();
        build(&mut pits, &world, &[BLOCK_VESSEL_RAW], 8, 8);
        pits.use_kiln(&world, AT, Some(BLOCK_FLINT));
        pits.step(&world, 100.0, 64);
        world.put(AT.0, AT.1 - 1, AT.2, BLOCK_AIR);
        let stepped = pits.step(&world, 0.05, 64);
        assert_eq!(Stage::of(world.get(AT.0, AT.1, AT.2)), Some(Stage::Pottery { pieces: 1, fired: false }));
        assert!(!stepped.news.is_empty(), "it went out without a word");
    }

    #[test]
    fn a_burning_pit_kiln_survives_a_save_with_its_remaining_time() {
        let dir = std::env::temp_dir().join(format!("primitive-pits-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let world = pit_world();
        let mut pits = Pits::new();
        build(&mut pits, &world, &[BLOCK_VESSEL_RAW, BLOCK_JUG_RAW], 8, 8);
        pits.use_kiln(&world, AT, Some(BLOCK_FLINT));
        pits.step(&world, 1234.5, 64);
        let left = pits.kiln_seconds_left(AT).expect("burning");
        assert!(pits.is_dirty(), "a burning kiln would not be written");
        assert_eq!(pits.save(&dir).expect("save"), 1);

        let mut restored = Pits::new();
        assert_eq!(restored.load(&dir).expect("load"), 1);
        assert_eq!(restored.kiln_seconds_left(AT), Some(left), "the kiln came back with another hour");
        assert_eq!(restored.pottery(AT), [BLOCK_VESSEL_RAW, BLOCK_JUG_RAW]);
        restored.step(&world, left + 0.1, 64);
        assert_eq!(restored.pottery(AT), [BLOCK_VESSEL, BLOCK_JUG], "the restored kiln never finished");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_kiln_broken_before_it_is_lit_gives_back_everything_that_went_in() {
        let world = pit_world();
        let mut pits = Pits::new();
        build(&mut pits, &world, &[BLOCK_VESSEL_RAW], 8, 0);
        pits.use_kiln(&world, AT, Some(BLOCK_BIRCH_LOG));
        pits.use_kiln(&world, AT, Some(BLOCK_LOG));
        let mut spilled = pits.broken(AT, world.get(AT.0, AT.1, AT.2));
        spilled.sort();
        let mut wanted = vec![(BLOCK_VESSEL_RAW, 1), (BLOCK_FIBER, 8), (BLOCK_BIRCH_LOG, 1), (BLOCK_LOG, 1)];
        wanted.sort();
        assert_eq!(spilled, wanted);
        assert!(pits.pottery(AT).is_empty());
    }

    /// A field of dirt with a hole `width` long at `AT`, piled with logs.
    fn pile_world(width: i32, logs: u8) -> TestWorld {
        let world = pit_world();
        for dx in 0..width {
            world.put(AT.0 + dx, AT.1, AT.2, log_pile(logs));
        }
        world
    }

    #[test]
    fn a_covered_log_pile_becomes_charcoal_and_an_uncovered_one_burns_to_ash() {
        let covered = pile_world(1, 8);
        let mut pits = Pits::new();
        pits.use_pile(&covered, AT, Some(BLOCK_FLINT));
        assert_eq!(covered.get(AT.0, AT.1, AT.2), log_pile_lit(8));
        // Struck on its open top, then covered well inside the grace.
        pits.step(&covered, 5.0, 64);
        covered.put(AT.0, AT.1 + 1, AT.2, BLOCK_DIRT);
        pits.step(&covered, CHARCOAL_SECONDS - 60.0, 64);
        assert_eq!(covered.get(AT.0, AT.1, AT.2), log_pile_lit(8), "it finished early");
        pits.step(&covered, 60.0, 64);
        assert_eq!(pit::charcoal_in(covered.get(AT.0, AT.1, AT.2)), Some(4), "eight logs made the wrong charcoal");
        assert_eq!(primitive_shared::types::block_drop(covered.get(AT.0, AT.1, AT.2)), Some(BLOCK_COAL));
        assert_eq!(primitive_shared::types::block_drop_count(covered.get(AT.0, AT.1, AT.2)), 4);

        let open = pile_world(1, 8);
        let mut pits = Pits::new();
        pits.use_pile(&open, AT, Some(BLOCK_FLINT));
        let mut news = Vec::new();
        for _ in 0..(OPEN_PILE_SECONDS as i32 + 2) {
            news.extend(pits.step(&open, 1.0, 64).news);
        }
        assert_eq!(open.get(AT.0, AT.1, AT.2), BLOCK_ASH, "an open pile made charcoal");
        assert!(news.iter().any(|(_, said)| said.contains("ash")), "nobody was told why");
    }

    #[test]
    fn lighting_one_pile_lights_the_piles_it_touches() {
        let world = pile_world(3, 4);
        let mut pits = Pits::new();
        let outcome = pits.use_pile(&world, AT, Some(BLOCK_FLINT));
        assert_eq!(outcome.wrote.len(), 3, "the strike did not spread through the pit");
        for dx in 0..3 {
            assert_eq!(world.get(AT.0 + dx, AT.1, AT.2), log_pile_lit(4));
        }
    }

    #[test]
    fn a_pile_takes_logs_up_to_eight_and_gives_back_the_kinds_that_went_in() {
        let world = pit_world();
        let mut pits = Pits::new();
        assert_eq!(pits.lay_pile(&world, AT, Some(BLOCK_BIRCH_LOG)).spent, Some(BLOCK_BIRCH_LOG));
        for _ in 1..PILE_LOGS_MAX {
            assert_eq!(pits.use_pile(&world, AT, Some(BLOCK_LOG)).spent, Some(BLOCK_LOG));
        }
        assert_eq!(pits.use_pile(&world, AT, Some(BLOCK_LOG)).spent, None, "a ninth log went in");
        let spilled = pits.broken(AT, world.get(AT.0, AT.1, AT.2));
        assert_eq!(spilled.len(), 8);
        assert_eq!(spilled[0], (BLOCK_BIRCH_LOG, 1));
    }

    #[test]
    fn a_burning_kiln_the_world_was_born_with_is_timed_rather_than_ignored() {
        let world = pit_world();
        world.put(AT.0, AT.1, AT.2, Stage::Burning.block());
        let mut pits = Pits::new();
        pits.stock_kiln(AT, &[BLOCK_VESSEL_RAW]);
        pits.on_block_changed(AT.0, AT.1, AT.2);
        pits.step(&world, 1.0, 64);
        assert!(pits.kiln_seconds_left(AT).is_some_and(|left| left < PIT_KILN_SECONDS));
    }
}
