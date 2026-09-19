//! The two mini-games: striking hot metal on the anvil, and throwing a pot
//! on the wheel.
//!
//! ## Why there is a module rather than a screen
//!
//! Because the client is never the authority, and a mini-game is the one
//! place where that is hard: what the server has to judge is *when the player
//! pressed*, which only the client can know. The shape that works is the one
//! here.
//!
//! * The **server** opens a run and remembers the moment it did, with a seed
//!   of its own choosing. The seed decides where the sweet spots are, so a
//!   client cannot know them before the run starts and cannot choose them at
//!   all.
//! * The **client** draws the marker from that seed, collects the player's
//!   presses as milliseconds from the start of the run, and sends the lot in
//!   one message when the last one is in.
//! * The **server** judges them ([`judge`]), against the same functions the
//!   client drew with, and against its own clock.
//!
//! Rejected, and written down because it is the obvious design: **one message
//! per blow**. Four round trips inside five seconds, on a connection with two
//! hundred milliseconds of lag, is a game played against the network -- and it
//! is exactly the burst the message rate limit exists to stop. One message at
//! the end costs nothing in feel, because the *marker* is local and always
//! was; only the verdict waits.
//!
//! ## What stops a cheat
//!
//! A client that simply lies about its presses is the whole of the threat, and
//! four rules catch every version of it that matters ([`judge`]):
//!
//! 1. **The count.** Never more presses than there are blows.
//! 2. **The window.** Each press is scored against the sweep it landed in,
//!    and the sweeps go forward: one press a sweep, in order. A sweep with no
//!    press in it is a blow that was missed and scores nothing -- which is
//!    why a run can be *short* and cannot be re-timed into a sweet spot it
//!    did not see.
//! 3. **The hand.** Two presses closer together than [`MIN_GAP_MS`] is not a
//!    person, and a first press inside [`MIN_FIRST_MS`] is a person who moved
//!    before the marker did.
//! 4. **The clock.** The server timed the run itself. A client that claims
//!    five seconds of perfect strikes two hundred milliseconds after it asked
//!    to begin is refused whatever its numbers say -- and *that* is the rule
//!    that catches the bot, because a bot's numbers are always perfect and its
//!    patience never is.
//!
//! What is deliberately *not* defended against is a player who writes a
//! program that presses at the right millisecond in real time. That program
//! has to wait out the run like everybody else, and what it wins is four
//! nails. A defence against it would have to be a defence against playing
//! well.

use crate::types::{block_kind, BlockId};

/// Which station's game.
///
/// On the wire (`protocol::ServerMessage::StationOpen`), so it derives what
/// every other message carrier here does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Game {
    /// The anvil: strike the hot piece while it is still hot.
    Anvil,
    /// The potter's wheel: draw the wall up and let go at the right width.
    Wheel,
    /// The honing stone: lay the edge on the stone at its bevel, stroke by
    /// stroke. Appended, so the two games before it keep the index they are
    /// sent by.
    Whet,
    /// The sawhorse: cut to the line, the saw's stroke meeting the mark.
    Saw,
}

impl Game {
    /// How many presses a run is.
    ///
    /// **Short on purpose.** Four blows is five and a half seconds, which is
    /// about as long as a player will look at a bar instead of at the world.
    /// A run long enough to be *interesting on its own* would be a different
    /// game wearing this one's clothes; what this is for is the moment
    /// between having a bar and having nails.
    pub fn presses(self) -> usize {
        match self {
            Game::Anvil => 4,
            Game::Wheel => 3,
            // Three strokes is what a grinder counts on a side, and a run
            // that sharpens should be shorter than one that makes something.
            Game::Whet => 3,
            // Four cuts: the four ends of a joint.
            Game::Saw => 4,
        }
    }

    /// How long one press's window lasts, in milliseconds. The marker
    /// crosses the bar and comes back exactly once inside it, so there are
    /// two chances at the sweet spot per press and the rhythm is readable.
    ///
    /// The wheel's is slower than the anvil's because they are different
    /// gestures: a hammer falls and a potter's hands *close*, and a wheel
    /// that hurried would read as a second anvil.
    pub fn step_ms(self) -> u32 {
        match self {
            Game::Anvil => 1400,
            Game::Wheel => 1600,
            // Slow, because a stroke on a stone is drawn the length of the
            // slab and a hurried one rounds the bevel over.
            Game::Whet => 1700,
            // A saw's stroke is quicker than a potter's hands and slower
            // than a hammer's fall.
            Game::Saw => 1500,
        }
    }

    /// The whole run, in milliseconds.
    pub fn run_ms(self) -> u32 {
        self.step_ms() * self.presses() as u32
    }
}

/// The floor under the gap between two presses.
///
/// A hundred and ten milliseconds. Simple reaction time to a seen event is
/// about two hundred, and the fastest sustained tapping a person manages is
/// around eight a second; this is under both, so no human run is ever refused
/// by it and a machine-gun run always is.
pub const MIN_GAP_MS: u32 = 110;

/// ...and under the first press: before this, the marker has barely left the
/// end of the bar and nobody has seen it yet.
pub const MIN_FIRST_MS: u32 = 90;

/// How far a client's clock may run ahead of the server's before the run is
/// refused.
///
/// Half a second. It has to cover the flight of the "begin" message, the
/// flight of the answer, and a frame or two at each end -- a player on a bad
/// connection must not lose a good run. It must also be small next to a whole
/// sweep (1400 ms), or a cheat could skip a press's worth of waiting.
pub const CLOCK_SLACK_MS: u32 = 500;

/// Where the marker is `ms` into the run: 0 at the left end of the bar, 1 at
/// the right, and back to 0 by the end of the press's window.
///
/// A triangle rather than a sine, because the eye reads a constant speed and
/// a sine spends most of its time at the ends -- which is where the sweet
/// spot never is, so a sine would make every run feel like waiting.
pub fn marker_at(game: Game, ms: u32) -> f32 {
    let step = game.step_ms() as f32;
    let phase = (ms % game.step_ms()) as f32 / step;
    if phase < 0.5 {
        phase * 2.0
    } else {
        2.0 - phase * 2.0
    }
}

/// The middle of press number `step`'s sweet spot, from the run's seed.
///
/// Kept clear of both ends (0.22..0.86): a sweet spot at the very end of the
/// bar is one the marker crawls through twice in a row, which is a free hit,
/// and one at zero cannot be missed at all.
pub fn target(seed: u32, step: usize) -> f32 {
    // One round of a well-known integer hash. A generator with state would
    // have to be stepped in the same order on both sides; a hash of
    // (seed, step) is the same number wherever it is asked for, which is what
    // a client redrawing the bar every frame needs.
    let mut h = seed ^ (step as u32).wrapping_mul(0x9E37_79B9);
    h ^= h >> 16;
    h = h.wrapping_mul(0x7FEB_352D);
    h ^= h >> 15;
    h = h.wrapping_mul(0x846C_A68B);
    h ^= h >> 16;
    0.22 + (h % 1001) as f32 / 1000.0 * 0.64
}

/// How wide the sweet spot is for the tool in hand, either side of its middle.
///
/// **This is the whole of what a better hammer buys.** It opens no block a
/// worse one does not and swings no faster: what it does is forgive. A stone
/// head is a lump on a stick and it lands where it lands; an iron head is
/// balanced, and a smith with one hits what they aimed at.
///
/// The numbers, in the units a player feels: the marker crosses the bar in
/// 700 ms, so a stone hammer's 0.18 is a window of about a quarter of a
/// second and an iron one's 0.30 is about four tenths. The `None` case is the
/// wheel, where the tool is a pair of hands.
///
/// **A saw is a hammer's ladder, and its edge narrows it.** A blunt saw binds
/// in the kerf and wanders off the line, so the window shrinks by the share
/// the edge takes off everything else (`tools::edge_factor`): a blunt copper
/// saw is a stone hammer's window and less. That is where honing reaches the
/// joiner -- a player can take a dull saw to the sawhorse and work harder for
/// the same chair, or go to the stone first.
pub fn tolerance(tool: Option<BlockId>) -> f32 {
    use crate::types::{
        BLOCK_BRONZE_HAMMER, BLOCK_BRONZE_SAW, BLOCK_COPPER_SAW, BLOCK_IRON_HAMMER, BLOCK_IRON_SAW,
        BLOCK_STONE_HAMMER,
    };
    let edge = tool.map_or(1.0, crate::tools::edge_factor);
    match tool.map(block_kind) {
        Some(BLOCK_STONE_HAMMER) => 0.18,
        Some(BLOCK_BRONZE_HAMMER) => 0.24,
        Some(BLOCK_IRON_HAMMER) => 0.30,
        Some(BLOCK_COPPER_SAW) => 0.22 * edge,
        Some(BLOCK_BRONZE_SAW) => 0.26 * edge,
        Some(BLOCK_IRON_SAW) => 0.30 * edge,
        // A potter has no tool and the clay forgives what a bar does not:
        // a wall pulled a little thin can be pulled back, and the only
        // unrecoverable mistake is a gross one.
        _ => 0.26,
    }
}

/// Is this a hammer -- the thing the anvil asks to be held?
pub fn is_hammer(block: BlockId) -> bool {
    use crate::types::{BLOCK_BRONZE_HAMMER, BLOCK_IRON_HAMMER, BLOCK_STONE_HAMMER};
    matches!(block_kind(block), BLOCK_STONE_HAMMER | BLOCK_BRONZE_HAMMER | BLOCK_IRON_HAMMER)
}

/// Is this a saw -- the thing the sawhorse asks to be held?
pub fn is_saw(block: BlockId) -> bool {
    use crate::types::{BLOCK_BRONZE_SAW, BLOCK_COPPER_SAW, BLOCK_IRON_SAW};
    matches!(block_kind(block), BLOCK_COPPER_SAW | BLOCK_BRONZE_SAW | BLOCK_IRON_SAW)
}

/// How a run turned out.
///
/// Three bands and not a number, because what the player is told has to be
/// something they can *act* on next time. "Sixty-one per cent" is a score;
/// "the piece is sound" is a smith telling you it went well.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Verdict {
    /// Struck true: more out of the same metal, or a second piece off the
    /// same clay.
    Fine,
    /// Serviceable. What the menu row would have given you anyway.
    Fair,
    /// Spoiled. Some of the material is gone and the hammer took the rest.
    Ruined,
}

/// Where the bands are.
///
/// **Fair is generous and Fine is not.** A player who presses roughly in
/// time should get what the crafting menu would have given them -- the
/// mini-game must never be a *worse* deal than not playing it, or the only
/// sane move would be to never open the screen. Fine is the reward for
/// actually watching the marker.
pub fn verdict(accuracy: f32) -> Verdict {
    if accuracy >= 0.55 {
        Verdict::Fine
    } else if accuracy >= 0.15 {
        Verdict::Fair
    } else {
        Verdict::Ruined
    }
}

/// How close one press was, from 1 (dead centre) to 0 (outside the spot).
pub fn press_accuracy(game: Game, seed: u32, tolerance: f32, step: usize, at_ms: u32) -> f32 {
    let distance = (marker_at(game, at_ms) - target(seed, step)).abs();
    (1.0 - distance / tolerance.max(f32::EPSILON)).clamp(0.0, 1.0)
}

/// Why a run was thrown out. See the module docs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// More presses than there are blows. A run may be short -- a missed
    /// blow is a sweep with nothing in it -- and can never be long.
    Count,
    /// A press before the marker had moved.
    Early,
    /// A press outside its own blow's window, or behind the one before it.
    OutOfTurn,
    /// Two presses no hand could make.
    Rushed,
    /// The run claims more time than has passed on the server's clock.
    Ahead,
}

impl Refusal {
    /// What the player is told. In English here and translated on the way to
    /// the screen (`ui::lang`), like every other refusal on the wire.
    pub fn note(self) -> &'static str {
        match self {
            Refusal::Count => "that is not a run",
            Refusal::Early => "you struck before the marker moved",
            Refusal::OutOfTurn => "those blows are out of turn",
            Refusal::Rushed => "no hand strikes that fast",
            Refusal::Ahead => "that run has not happened yet",
        }
    }
}

/// **Would this press be a blow the server counts**, given the one before it
/// (`last`, its time) -- or should the client let it pass unrecorded?
///
/// The client keeps every rule [`judge`] refuses a whole run for, press by
/// press, because a refused run costs the materials: it took the bar or the
/// clay at the start and gives nothing back. It used to keep only "one blow
/// a sweep", so a nervous first press in the first ninety milliseconds, or
/// two blows either side of the line between two sweeps a few milliseconds
/// apart, sent a run the server threw out whole -- the player's whole bar,
/// for a double tap. Dropping the press instead is what a hammer that has not
/// come back up yet does.
pub fn press_counts(game: Game, last: Option<u32>, at: u32) -> bool {
    let window = (at / game.step_ms()) as usize;
    if at < MIN_FIRST_MS || window >= game.presses() {
        return false;
    }
    match last {
        Some(last) => {
            let last_window = (last / game.step_ms()) as usize;
            window > last_window && at > last && at - last >= MIN_GAP_MS
        }
        None => true,
    }
}

/// Judge a whole run: the four rules, then the score.
///
/// `elapsed_ms` is what the *server's* clock says has passed since it let the
/// run begin. Rule 4 lives on it, and it is the reason this takes a number
/// the client never touches.
pub fn judge(
    game: Game,
    seed: u32,
    tolerance: f32,
    presses: &[u32],
    elapsed_ms: u32,
) -> Result<f32, Refusal> {
    // **A short run is a run with misses in it, and that is the honest
    // shape.** The first version made the client pad a missed blow out with
    // a timestamp, and there is no timestamp that means "missed": whatever
    // the client filled in landed *somewhere* on the bar, and at the wrong
    // end of a wide sweet spot a miss scored better than half a hit. A sweep
    // with nothing in it scores nothing, which is what a miss is.
    if presses.len() > game.presses() {
        return Err(Refusal::Count);
    }
    let step = game.step_ms();
    let mut last: Option<(usize, u32)> = None;
    let mut total = 0.0;
    for &at in presses {
        if at < MIN_FIRST_MS {
            return Err(Refusal::Early);
        }
        // Which blow this was is read off the clock rather than off the
        // press's place in the list, so a client cannot hand the same timing
        // in against a different blow's sweet spot.
        let window = (at / step) as usize;
        if window >= game.presses() {
            return Err(Refusal::OutOfTurn);
        }
        if let Some((last_window, last_at)) = last {
            if window <= last_window || at <= last_at {
                return Err(Refusal::OutOfTurn);
            }
            if at - last_at < MIN_GAP_MS {
                return Err(Refusal::Rushed);
            }
        }
        total += press_accuracy(game, seed, tolerance, window, at);
        last = Some((window, at));
    }
    // The clock. `saturating_add` because a wrap here would be a rule that
    // *stops* refusing at exactly the input worth refusing.
    let claimed = last.map_or(0, |(_, at)| at);
    if claimed > elapsed_ms.saturating_add(CLOCK_SLACK_MS) {
        return Err(Refusal::Ahead);
    }
    // Divided by the blows there *were*, not by the presses that arrived: a
    // run of one good blow out of four is a quarter of a run, and dividing by
    // one would make walking away after the first hit the best play there is.
    Ok(total / game.presses() as f32)
}

/// One thing a station can be asked to make.
///
/// **A job, not a recipe**, and they are different in the one way that
/// matters: a recipe's output is fixed and a job's is a function of how well
/// it went. They cannot share a table for that reason, and a `Recipe` with a
/// variable output would make every one of the four hundred rows above carry
/// a field that only two of them use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Job {
    /// An iron bar drawn out under the hammer and cut into nails. The kiln's
    /// own row gives a flat dozen; this gives four, twelve or eighteen.
    Nails,
    /// A bronze sheet raised over the horn into a cap. The fire's row gives
    /// a helm for two ingots; a good smith gets one of them back.
    Helm,
    /// A pot thrown on the wheel: the crucible's shape.
    Vessel,
    /// A jug.
    Jug,
    /// An ingot mould, pressed and trued on the wheel head.
    Mould,
    /// A bowl: the cheapest thing the wheel makes, and the one a household
    /// wants most of. See `types::BLOCK_STEW` for why. Appended, so every
    /// job before it keeps the index it is sent by.
    Bowl,
    /// **The edge of the tool in the hand, taken back on the honing stone.**
    /// Nothing is spent and nothing is made: the run decides how much metal
    /// the stone takes with the dullness (`tools::hone_by`), and the server
    /// works the held stack rather than handing out a new one.
    Hone,
    // ---- the sawhorse ----
    //
    // **Each is a row of the crafting table, played** (`Job::recipe`): the
    // same boards, frame and pegs the menu row takes, so the choice between
    // the row and the game is a choice about skill and not about price --
    // the wheel's rule (`Job::Vessel`). What the game adds is wood saved on
    // a true cut and a better-made piece where the piece carries a judgement
    // (`quality::takes_quality`).
    /// A stool.
    Stool,
    /// A chair.
    Chair,
    /// A table.
    Table,
    /// A door.
    Door,
    /// The joined chest.
    Chest,
    /// A bed.
    Bed,
    /// A barrel.
    Barrel,
}

impl Job {
    /// Every job, in the order the screens list them.
    pub const ALL: [Job; 14] = [
        Job::Nails,
        Job::Helm,
        Job::Vessel,
        Job::Jug,
        Job::Mould,
        Job::Bowl,
        Job::Hone,
        Job::Stool,
        Job::Chair,
        Job::Table,
        Job::Door,
        Job::Chest,
        Job::Bed,
        Job::Barrel,
    ];

    /// The row of the crafting table a sawhorse job plays, found by its
    /// name. `None` for every other job.
    ///
    /// **By name and not by index**, because the index is the row's identity
    /// on the wire and the name is the identity a person reads; a test below
    /// holds every one of them to a row that exists and makes what the job
    /// says it makes. Rejected: *a copy of each row's inputs in this file*,
    /// which is two tables that must agree -- the wheel's vessel is checked
    /// against its row by a test for exactly that reason, and seven of them
    /// would be seven such tests.
    pub fn recipe(self) -> Option<&'static crate::crafting::Recipe> {
        let name = match self {
            Job::Stool => "stool",
            Job::Chair => "chair",
            Job::Table => "table",
            Job::Door => "door",
            Job::Chest => "chest",
            Job::Bed => "bed",
            Job::Barrel => "barrel",
            _ => return None,
        };
        crate::crafting::RECIPES.iter().find(|row| row.name == name)
    }

    /// Which station does it.
    pub fn game(self) -> Game {
        match self {
            Job::Nails | Job::Helm => Game::Anvil,
            Job::Vessel | Job::Jug | Job::Mould | Job::Bowl => Game::Wheel,
            Job::Hone => Game::Whet,
            Job::Stool | Job::Chair | Job::Table | Job::Door | Job::Chest | Job::Bed | Job::Barrel => Game::Saw,
        }
    }

    /// What it costs, spent when the run begins -- so a player who walks away
    /// from a half-struck bar has spent the bar, exactly as they would have
    /// if they had hammered it flat and thrown it in a corner.
    pub fn inputs(self) -> &'static [(BlockId, u32)] {
        use crate::types::{BLOCK_BRONZE_INGOT, BLOCK_CLAY, BLOCK_IRON_INGOT, BLOCK_LEATHER, BLOCK_SAND};
        match self {
            Job::Nails => &[(BLOCK_IRON_INGOT, 1)],
            Job::Helm => &[(BLOCK_BRONZE_INGOT, 2), (BLOCK_LEATHER, 1)],
            // The same clay the wheel's menu rows ask for, so the choice
            // between the row and the game is a choice about *skill* and not
            // about price. See `crafting`, "thrown vessel".
            Job::Vessel => &[(BLOCK_CLAY, 3), (BLOCK_SAND, 1)],
            Job::Jug => &[(BLOCK_CLAY, 3)],
            Job::Mould => &[(BLOCK_CLAY, 3)],
            // The wheel row's two clay, for the reason the vessel's says.
            Job::Bowl => &[(BLOCK_CLAY, 2)],
            // The tool is worked, not spent.
            Job::Hone => &[],
            // The row's own inputs. The server spends them through the row
            // (`crafting::begin_piece`), so any wood's boards will do, as they
            // do in the menu.
            _ => self.recipe().map_or(&[], |row| row.inputs),
        }
    }

    /// The identifier a screen names it by, and the key `ui::names` looks up.
    pub fn name(self) -> &'static str {
        match self {
            Job::Nails => "nails",
            Job::Helm => "bronze_helm",
            Job::Vessel => "vessel_raw",
            Job::Jug => "jug_raw",
            Job::Mould => "mould_raw",
            Job::Bowl => "bowl_raw",
            Job::Hone => "whetstone",
            Job::Stool => "stool",
            Job::Chair => "chair",
            Job::Table => "table",
            Job::Door => "door",
            Job::Chest => "chest",
            Job::Bed => "bed",
            Job::Barrel => "barrel",
        }
    }
}

/// What a run leaves in the pack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// What came off the anvil or the wheel, if anything.
    pub made: Option<(BlockId, u32)>,
    /// What of the material is handed back: metal that was never spoiled,
    /// clay that can be wedged and thrown again.
    pub back: Vec<(BlockId, u32)>,
    /// Extra points of wear on the hammer, on top of the one every blow
    /// costs. Zero at the wheel, which is worked with hands.
    pub extra_wear: u32,
}

/// What a job and a verdict come to.
///
/// **The shape of every row is the same**, and it is the answer to "what does
/// accuracy buy": a fine run is *less waste*, never a thing that could not
/// otherwise be had. There is no item here that a patient player cannot get
/// out of the crafting menu, which is what keeps the mini-game a better way
/// rather than a gate -- see `crafting::Station::Bench` for the same argument
/// about the workshops.
pub fn outcome(job: Job, verdict: Verdict) -> Outcome {
    use crate::types::{
        BLOCK_BRONZE_HELM, BLOCK_BRONZE_INGOT, BLOCK_CLAY, BLOCK_JUG_RAW, BLOCK_LEATHER, BLOCK_MOULD_RAW,
        BLOCK_NAILS, BLOCK_SAND, BLOCK_VESSEL_RAW,
    };
    let none = |back: Vec<(BlockId, u32)>, extra_wear| Outcome { made: None, back, extra_wear };
    match (job, verdict) {
        // A dozen is what the kiln gives for the same bar. Eighteen for a
        // smith who watched the marker, four for one who did not -- and the
        // four are the ones that were cut before the bar went cold, which is
        // why they are not none at all.
        (Job::Nails, Verdict::Fine) => Outcome { made: Some((BLOCK_NAILS, 18)), back: Vec::new(), extra_wear: 0 },
        (Job::Nails, Verdict::Fair) => Outcome { made: Some((BLOCK_NAILS, 12)), back: Vec::new(), extra_wear: 0 },
        (Job::Nails, Verdict::Ruined) => {
            Outcome { made: Some((BLOCK_NAILS, 4)), back: Vec::new(), extra_wear: 3 }
        }
        (Job::Helm, Verdict::Fine) => Outcome {
            made: Some((BLOCK_BRONZE_HELM, 1)),
            back: vec![(BLOCK_BRONZE_INGOT, 1)],
            extra_wear: 0,
        },
        (Job::Helm, Verdict::Fair) => {
            Outcome { made: Some((BLOCK_BRONZE_HELM, 1)), back: Vec::new(), extra_wear: 0 }
        }
        // A split sheet is not a helm and the leather was never in the fire,
        // so it comes back. One ingot's worth of bronze is scrap on the
        // floor; the other can be melted again.
        (Job::Helm, Verdict::Ruined) => {
            none(vec![(BLOCK_BRONZE_INGOT, 1), (BLOCK_LEATHER, 1)], 3)
        }
        // **Two pots off one lump, and that is what a wheel is for.** A wall
        // drawn thin and even leaves enough clay for a second small piece;
        // a wall pulled through collapses, and what comes back is the clay,
        // because clay always comes back -- a potter loses the hour, not the
        // material. The sand is temper and survives either way.
        (Job::Vessel, Verdict::Fine) => {
            Outcome { made: Some((BLOCK_VESSEL_RAW, 2)), back: Vec::new(), extra_wear: 0 }
        }
        (Job::Vessel, Verdict::Fair) => {
            Outcome { made: Some((BLOCK_VESSEL_RAW, 1)), back: Vec::new(), extra_wear: 0 }
        }
        (Job::Vessel, Verdict::Ruined) => none(vec![(BLOCK_CLAY, 2), (BLOCK_SAND, 1)], 0),
        (Job::Jug, Verdict::Fine) => Outcome { made: Some((BLOCK_JUG_RAW, 2)), back: Vec::new(), extra_wear: 0 },
        (Job::Jug, Verdict::Fair) => Outcome { made: Some((BLOCK_JUG_RAW, 1)), back: Vec::new(), extra_wear: 0 },
        (Job::Jug, Verdict::Ruined) => none(vec![(BLOCK_CLAY, 2)], 0),
        (Job::Mould, Verdict::Fine) => {
            Outcome { made: Some((BLOCK_MOULD_RAW, 2)), back: Vec::new(), extra_wear: 0 }
        }
        (Job::Mould, Verdict::Fair) => {
            Outcome { made: Some((BLOCK_MOULD_RAW, 1)), back: Vec::new(), extra_wear: 0 }
        }
        (Job::Mould, Verdict::Ruined) => none(vec![(BLOCK_CLAY, 2)], 0),
        // Two bowls off two clay for a wall drawn even -- a bowl is small,
        // and the lump a careless hand makes one out of makes two in a good
        // one. A collapsed one is a lump of clay again, less what stuck to
        // the hands.
        (Job::Bowl, Verdict::Fine) => {
            Outcome { made: Some((crate::types::BLOCK_BOWL_RAW, 2)), back: Vec::new(), extra_wear: 0 }
        }
        (Job::Bowl, Verdict::Fair) => {
            Outcome { made: Some((crate::types::BLOCK_BOWL_RAW, 1)), back: Vec::new(), extra_wear: 0 }
        }
        (Job::Bowl, Verdict::Ruined) => none(vec![(BLOCK_CLAY, 1)], 0),
        // Nothing is made at the stone: the held tool is worked by the
        // server (`tools::hone_by`).
        (Job::Hone, _) => none(Vec::new(), 0),
        (joinery, verdict) => joinery_outcome(joinery, verdict),
    }
}

/// What a sawhorse run comes to.
///
/// **Boards, and only boards, are what a cut saves or spoils.** A true cut
/// hands one board of the piece back -- the offcut is a board and not
/// sawdust; a spoiled one leaves no piece, gives back half the boards (the
/// other half are the wrong length now, and firewood) and every other input
/// whole: a frame, pegs, leather and cord were never under the saw. A saw
/// that bound in the cut takes two more points of wear.
///
/// The boards here are named as oak because a row names oak; the server
/// hands back boards of the wood the piece was actually made of
/// (`crafting::begin_piece`).
fn joinery_outcome(job: Job, verdict: Verdict) -> Outcome {
    use crate::types::BLOCK_PLANKS;
    let Some(row) = job.recipe() else {
        return Outcome { made: None, back: Vec::new(), extra_wear: 0 };
    };
    let boards = row.inputs.iter().find(|&&(block, _)| block_kind(block) == BLOCK_PLANKS).map_or(0, |&(_, n)| n);
    match verdict {
        Verdict::Fine => Outcome {
            made: Some(row.output),
            back: if boards > 0 { vec![(BLOCK_PLANKS, 1)] } else { Vec::new() },
            extra_wear: 0,
        },
        Verdict::Fair => Outcome { made: Some(row.output), back: Vec::new(), extra_wear: 0 },
        Verdict::Ruined => Outcome {
            made: None,
            back: row
                .inputs
                .iter()
                .map(|&(block, n)| if block_kind(block) == BLOCK_PLANKS { (block, n / 2) } else { (block, n) })
                .filter(|&(_, n)| n > 0)
                .collect(),
            extra_wear: 2,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_run_made_only_of_presses_the_client_counts_is_never_refused() {
        // Every press a player could make, in every rhythm: a cheap
        // generator walked over a spread of gaps, including the ones that
        // straddle a sweep's edge and the ones under the floors.
        for game in [Game::Anvil, Game::Wheel, Game::Whet, Game::Saw] {
            let mut state = 0x1234_5678u32;
            for _ in 0..4000 {
                let mut presses: Vec<u32> = Vec::new();
                let mut at = 0u32;
                loop {
                    state ^= state << 13;
                    state ^= state >> 17;
                    state ^= state << 5;
                    at += state % 900;
                    if at > game.run_ms() + 200 {
                        break;
                    }
                    if press_counts(game, presses.last().copied(), at) {
                        presses.push(at);
                    }
                }
                let elapsed = presses.last().copied().unwrap_or(0);
                assert!(
                    judge(game, 7, tolerance(None), &presses, elapsed).is_ok(),
                    "{game:?}: the client would send {presses:?} and the server would throw it out"
                );
            }
        }
        // ...and the three presses that used to lose the bar are dropped.
        assert!(!press_counts(Game::Anvil, None, MIN_FIRST_MS - 1), "a press before the marker moved counted");
        let edge = Game::Anvil.step_ms();
        assert!(!press_counts(Game::Anvil, Some(edge - 20), edge + 20), "a double tap across a sweep's edge counted");
        assert!(press_counts(Game::Anvil, Some(edge - 200), edge + 200), "an honest next blow was dropped");
    }

    /// A run of presses that lands dead on every sweet spot, the way a
    /// player who watched the marker would.
    fn perfect(game: Game, seed: u32) -> Vec<u32> {
        (0..game.presses())
            .map(|step| {
                let want = target(seed, step);
                // The rising half of the sweep: phase = want / 2.
                let base = step as u32 * game.step_ms();
                base + (want / 2.0 * game.step_ms() as f32) as u32
            })
            .collect()
    }

    #[test]
    fn a_run_struck_on_the_marker_is_a_fine_one_and_a_run_of_wild_blows_is_not() {
        for seed in [1u32, 7, 4242, 0xDEAD_BEEF] {
            let good = perfect(Game::Anvil, seed);
            let score = judge(Game::Anvil, seed, tolerance(None), &good, Game::Anvil.run_ms())
                .expect("a run on the marker was refused");
            assert!(score > 0.95, "a perfect run scored {score} on seed {seed}");
            assert_eq!(verdict(score), Verdict::Fine);

            // ...and the same run pushed a third of a sweep out of step.
            let late: Vec<u32> = good
                .iter()
                .enumerate()
                .map(|(step, &at)| {
                    let base = step as u32 * Game::Anvil.step_ms();
                    let end = base + Game::Anvil.step_ms() - 1;
                    (at + Game::Anvil.step_ms() / 3).min(end)
                })
                .collect();
            let score = judge(Game::Anvil, seed, tolerance(None), &late, Game::Anvil.run_ms())
                .expect("a late run was refused rather than scored");
            assert!(score < 0.55, "blows a third of a sweep out still scored {score}");
        }
    }

    #[test]
    fn a_better_hammer_forgives_a_blow_a_worse_one_does_not() {
        use crate::types::{BLOCK_BRONZE_HAMMER, BLOCK_IRON_HAMMER, BLOCK_STONE_HAMMER};
        let widths: Vec<f32> = [BLOCK_STONE_HAMMER, BLOCK_BRONZE_HAMMER, BLOCK_IRON_HAMMER]
            .into_iter()
            .map(|hammer| tolerance(Some(hammer)))
            .collect();
        assert!(widths[0] < widths[1] && widths[1] < widths[2], "the hammers do not climb: {widths:?}");
        // And the ladder is a ladder of *accuracy*, not of possibility: the
        // same blow scores better with a better head and is never impossible
        // with the worst.
        let seed = 99;
        let off = perfect(Game::Anvil, seed).iter().map(|&at| at + 90).collect::<Vec<_>>();
        let scores: Vec<f32> = widths
            .iter()
            .map(|&w| judge(Game::Anvil, seed, w, &off, Game::Anvil.run_ms()).expect("refused"))
            .collect();
        assert!(scores[0] < scores[2], "a better hammer did not help: {scores:?}");
    }

    #[test]
    fn the_server_refuses_a_run_no_hand_could_have_made() {
        let seed = 5;
        let good = perfect(Game::Anvil, seed);
        let elapsed = Game::Anvil.run_ms();
        // One blow short is allowed and is worth less; one too many is not
        // a run at all.
        let short = judge(Game::Anvil, seed, 0.2, &good[..3], elapsed).expect("a short run was refused");
        let whole = judge(Game::Anvil, seed, 0.2, &good, elapsed).expect("a whole run was refused");
        assert!(short < whole, "three good blows scored as well as four: {short} vs {whole}");
        let mut extra = good.clone();
        extra.push(elapsed - 1);
        assert_eq!(judge(Game::Anvil, seed, 0.2, &extra, elapsed), Err(Refusal::Count));
        // ...and a run nobody pressed in at all is a spoiled one rather than
        // a refused one: the player walked away, and the bar is still spent.
        assert_eq!(judge(Game::Anvil, seed, 0.2, &[], elapsed), Ok(0.0));
        assert_eq!(verdict(0.0), Verdict::Ruined);
        // Struck before the marker moved.
        let mut early = good.clone();
        early[0] = MIN_FIRST_MS - 1;
        assert_eq!(judge(Game::Anvil, seed, 0.2, &early, elapsed), Err(Refusal::Early));
        // Four blows crammed into the first sweep: the window rule. The
        // first is fine and the second is in a sweep that has already been
        // scored, which is exactly the replay this stops.
        let crammed = vec![200, 400, 600, 800];
        assert_eq!(judge(Game::Anvil, seed, 0.2, &crammed, elapsed), Err(Refusal::OutOfTurn));
        // Presses cannot be *reordered* without leaving their windows --
        // the windows are disjoint and in order, so rule 2 already says it.
        // The order check inside `judge` stays anyway: it is two lines, and
        // it is what keeps the loop honest if the windows ever overlap.
        let step = Game::Anvil.step_ms();
        // A hand that cannot exist: two blows across a window boundary, a
        // millisecond apart.
        let rushed = vec![step - 1, step, 2 * step + 10, 3 * step + 10];
        assert_eq!(judge(Game::Anvil, seed, 0.2, &rushed, elapsed), Err(Refusal::Rushed));
        // ...and the clock: a whole run claimed before it could have happened.
        assert_eq!(judge(Game::Anvil, seed, 0.2, &good, 0), Err(Refusal::Ahead));
        // A run the server watched take its time is fine, and so is one that
        // took a little longer than the client said -- lag is not cheating.
        assert!(judge(Game::Anvil, seed, 0.2, &good, elapsed * 2).is_ok());
    }

    #[test]
    fn the_marker_crosses_the_bar_and_comes_back_inside_one_press() {
        for game in [Game::Anvil, Game::Wheel, Game::Whet, Game::Saw] {
            let step = game.step_ms();
            assert!(marker_at(game, 0) < 0.01, "the marker does not start at the end of the bar");
            assert!(marker_at(game, step / 2) > 0.99, "the marker does not reach the far end");
            assert!(marker_at(game, step - 1) < 0.01, "the marker does not come back");
            // Never off the bar, at any moment of any run.
            for ms in 0..game.run_ms() {
                let at = marker_at(game, ms);
                assert!((0.0..=1.0).contains(&at), "the marker is at {at} after {ms} ms");
            }
            // ...and a sweet spot is always somewhere the marker actually
            // goes, clear of both ends.
            for step in 0..game.presses() {
                let target = target(0x5EED, step);
                assert!((0.2..0.9).contains(&target), "a sweet spot sits at {target}");
            }
        }
    }

    #[test]
    fn a_spoiled_run_costs_material_and_never_costs_more_than_was_put_in() {
        for job in Job::ALL {
            for verdict in [Verdict::Fine, Verdict::Fair, Verdict::Ruined] {
                let out = outcome(job, verdict);
                for &(block, amount) in &out.back {
                    let paid = job
                        .inputs()
                        .iter()
                        .find(|&&(input, _)| block_kind(input) == block_kind(block))
                        .map(|&(_, amount)| amount)
                        .unwrap_or(0);
                    assert!(
                        amount <= paid,
                        "{job:?} hands back {amount} of {block} having taken {paid}"
                    );
                }
            }
            // A fine run is never worse than a fair one, and a ruined one
            // never better: the whole promise of the screen.
            let fine = outcome(job, Verdict::Fine);
            let fair = outcome(job, Verdict::Fair);
            let ruined = outcome(job, Verdict::Ruined);
            let count = |o: &Outcome| o.made.map_or(0, |(_, n)| n);
            assert!(count(&fine) >= count(&fair), "{job:?} pays worse for a fine run");
            assert!(count(&fair) >= count(&ruined), "{job:?} pays better for a spoiled run");
            assert!(ruined.extra_wear >= fine.extra_wear, "{job:?} spares the hammer on a bad run");
        }
    }

    #[test]
    fn a_fair_run_is_never_a_worse_deal_than_the_crafting_menu() {
        // The argument in `verdict`: a player who presses roughly in time
        // gets what the menu would have given them. Checked against the
        // rows those jobs shadow, so a change to either side shows here.
        use crate::types::{BLOCK_BRONZE_HELM, BLOCK_JUG_RAW, BLOCK_NAILS, BLOCK_VESSEL_RAW};
        let made = |job: Job| outcome(job, Verdict::Fair).made;
        assert_eq!(made(Job::Nails), Some((BLOCK_NAILS, 12)), "the kiln's own row gives a dozen");
        assert_eq!(made(Job::Helm), Some((BLOCK_BRONZE_HELM, 1)));
        assert_eq!(made(Job::Vessel), Some((BLOCK_VESSEL_RAW, 1)));
        assert_eq!(made(Job::Jug), Some((BLOCK_JUG_RAW, 1)));
        // ...and a run at the wheel costs exactly what the wheel's menu row
        // costs, so the game is about skill and not about price.
        let row = crate::crafting::RECIPES
            .iter()
            .find(|r| r.name == "thrown vessel")
            .expect("the wheel's vessel row is gone");
        assert_eq!(row.inputs, Job::Vessel.inputs());
    }

    #[test]
    fn every_sawhorse_job_is_a_row_of_the_table_and_a_fair_cut_is_that_row() {
        for job in Job::ALL.into_iter().filter(|job| job.game() == Game::Saw) {
            let row = job.recipe().unwrap_or_else(|| panic!("{job:?} plays no row of the table"));
            assert_eq!(row.inputs, job.inputs(), "{job:?} costs something other than its row");
            assert_eq!(outcome(job, Verdict::Fair).made, Some(row.output), "{job:?} makes something other than its row");
            assert_eq!(crate::types::block_name(row.output.0), job.name(), "{job:?} is named for another piece");
        }
        // ...and nothing else pretends to be one.
        assert!(Job::Nails.recipe().is_none() && Job::Hone.recipe().is_none());
    }

    #[test]
    fn a_blunt_saw_is_a_narrower_window_than_a_sharp_one_and_iron_is_wider_than_copper() {
        use crate::tools::{with_edge, BLUNTEST};
        use crate::types::{BLOCK_COPPER_SAW, BLOCK_IRON_SAW};
        let sharp = tolerance(Some(BLOCK_COPPER_SAW));
        let blunt = tolerance(Some(with_edge(BLOCK_COPPER_SAW, BLUNTEST)));
        assert!(blunt < sharp, "a blunt saw cuts as true as a sharp one: {blunt} vs {sharp}");
        assert!(tolerance(Some(BLOCK_IRON_SAW)) > sharp, "iron forgives no more than copper");
        assert!(is_saw(with_edge(BLOCK_IRON_SAW, 2)) && !is_saw(crate::types::BLOCK_IRON_HAMMER));
    }
}
