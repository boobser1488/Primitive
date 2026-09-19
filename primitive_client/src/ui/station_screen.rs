//! The anvil's, the potter's wheel's, the sawhorse's and the honing stone's
//! screen: pick a job, then keep time.
//!
//! **One module for both games**, and it is not laziness. What is on the
//! screen is the same object in both: a list of jobs, a bar with a marker
//! sweeping it, and a sweet spot to land on. The two differ in what the bar
//! *means* -- where the hammer falls, how wide the wall is drawn -- and in
//! what comes out, and both of those are already `minigame`'s to say. Two
//! modules would have been the same layout code twice, and the second copy is
//! where the hit-test stops being the inverse of the drawing.
//!
//! **Nothing here decides anything.** The presses are collected and sent; the
//! verdict comes back from the server (`protocol::ServerMessage::StationResult`)
//! and is shown. The client draws the marker from the server's own seed with
//! the server's own functions, so what a player sees is what they are judged
//! on -- but it is judged there, not here.
//!
//! ## The layout, and why it is written once
//!
//! Every rectangle on this screen comes out of [`Panel`], and both the drawing
//! and the hit-testing read the same one. That is the house rule for a screen
//! (`ui::widgets`), and this is the screen where breaking it would be least
//! visible: the strike button is pressed in a hurry, at speed, and a button
//! that answers a few hundredths of a unit away from where it is drawn reads
//! as "the game dropped my blow" rather than as a layout bug.

use std::time::Instant;

use primitive_shared::minigame::{self, Game, Job, Verdict};
use primitive_shared::types::BlockId;

use crate::engine::texture::{FaceLayers, FontAtlas};
use crate::ui::hotbar::HotbarVertex;
use crate::ui::lang::{Language, Msg};
use crate::ui::widgets::{self, Painter, Rect};

/// How big the screen is in its own space, for `Layout::fit`.
///
/// **Half-extents**, which is the convention `Layout::fit` and every other
/// screen here use: a panel drawn about the middle runs out of glass at half
/// the window, so what it is measured against is half the room. Written as
/// the full size once, it made the screen refuse to grow past a scale of one
/// on any window -- the cap came out twice as tight as it should be, which
/// looks like an interface-size setting that does nothing.
pub const EXTENT: (f32, f32) = (PANEL_HALF_WIDTH, PANEL_HALF_HEIGHT);

const PANEL_HALF_WIDTH: f32 = 0.62;
const PANEL_HALF_HEIGHT: f32 = 0.52;
/// Air inside the panel's edge.
const PAD: f32 = 0.05;
/// One job row, and the gap under it.
const ROW_HEIGHT: f32 = 0.12;
const ROW_GAP: f32 = 0.025;
/// How tall the striking bar is.
const BAR_HEIGHT: f32 = 0.16;
/// How wide the button that shuts the screen is. Not a whole row: it sits in
/// the corner under the last of the words, where a back button goes.
const CLOSE_WIDTH: f32 = 0.34;
/// Text sizes: the heading, and the line under the bar. A job row letters
/// itself to the button it is in (`Painter::button`).
const TITLE_SCALE: f32 = 1.0;
const NOTE_SCALE: f32 = 0.78;

/// What the frame loop should do about a gesture on this screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Intent {
    /// Ask the server to start this job. It takes the materials.
    Begin(Job),
    /// The run is complete: here is every press, in milliseconds from the
    /// moment the server said to begin.
    Run(Vec<u32>),
    /// Shut the screen.
    Close,
}

/// A run in progress on this client.
struct Run {
    job: Job,
    seed: u32,
    /// When `StationBegun` arrived. The client's own clock: what it sends is
    /// offsets from this, and the server checks them against *its* clock.
    began: Instant,
    /// One entry per blow *struck* -- shorter than the run when blows were
    /// missed. See `press`.
    presses: Vec<u32>,
    /// Set once the whole run has been handed in, so a second press in the
    /// last few milliseconds does not send it twice.
    sent: bool,
}

/// The screen, open or not.
pub struct StationScreen {
    /// `None` when shut. The server decides when it opens, exactly as it does
    /// for a chest: the click sends a question and the screen appears with
    /// the answer.
    open: Option<Open>,
    /// A question is outstanding. Anything the server answers while this is
    /// set is wanted -- see `ChestScreen::asked_to_open`, whose problem this
    /// is too: a screen shut a frame before the answer arrived would open
    /// again for a station the player has walked away from.
    asked: bool,
    cursor: Option<(f32, f32)>,
}

struct Open {
    game: Game,
    /// The width of the sweet spot, decided by the server from the hammer in
    /// the hand. Drawn at exactly this width, because it is the width the run
    /// will be judged by.
    tolerance: f32,
    run: Option<Run>,
    /// The job the client has asked to begin and not yet been answered about.
    ///
    /// **Kept here rather than sent back down**: `StationBegun` carries the
    /// seed and nothing else, because the job is not the server's to remind
    /// the client of -- the client is the one that asked. A second "begin"
    /// while one is outstanding overwrites this, and the server refuses it
    /// anyway (`station_begin`, one run at a time).
    pending: Option<Job>,
    /// The last verdict, kept on the screen until another job is started.
    result: Option<(Verdict, Option<(BlockId, u32)>)>,
}

impl Default for StationScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl StationScreen {
    pub fn new() -> Self {
        StationScreen { open: None, asked: false, cursor: None }
    }

    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }

    /// A question has gone to the server.
    pub fn asked_to_open(&mut self) {
        self.asked = true;
    }

    /// The server said yes.
    pub fn show(&mut self, game: Game, tolerance: f32) {
        if !self.asked {
            return;
        }
        self.asked = false;
        self.open = Some(Open { game, tolerance, run: None, pending: None, result: None });
    }

    /// The server took the materials and started the clock.
    pub fn begun(&mut self, seed: u32) {
        if let Some(open) = self.open.as_mut() {
            let Some(job) = open.pending.take() else {
                return;
            };
            open.result = None;
            open.run = Some(Run { job, seed, began: Instant::now(), presses: Vec::new(), sent: false });
        }
    }

    /// The server has judged the run.
    pub fn finished(&mut self, verdict: Verdict, made: Option<(BlockId, u32)>) {
        if let Some(open) = self.open.as_mut() {
            open.run = None;
            open.result = Some((verdict, made));
        }
    }

    /// The server refused the run, or it went wrong: clear the bar, keep the
    /// screen. The words are the ordinary error notice, which is where every
    /// other refusal in the game already lands.
    pub fn run_refused(&mut self) {
        if let Some(open) = self.open.as_mut() {
            open.run = None;
        }
    }

    pub fn close(&mut self) {
        self.open = None;
        self.asked = false;
        self.cursor = None;
    }

    /// Everything on this screen that changes because something happened --
    /// for the frame loop's "has the interface changed" key. Not the marker,
    /// which moves with nothing happening: that is [`Self::is_running`].
    pub fn ui_key(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.cursor.map(|(x, y)| (x.to_bits(), y.to_bits())).hash(&mut h);
        if let Some(open) = self.open.as_ref() {
            (open.game as u8).hash(&mut h);
            open.tolerance.to_bits().hash(&mut h);
            open.pending.map(|job| job as u8).hash(&mut h);
            open.result.map(|(verdict, made)| (verdict as u8, made)).hash(&mut h);
            open.run.as_ref().map(|run| (run.job as u8, run.seed, run.presses.len(), run.sent)).hash(&mut h);
        } else {
            u8::MAX.hash(&mut h);
        }
        h.finish()
    }

    /// A run is up and its marker is moving: the screen is different every
    /// frame, with nothing happening to say so.
    pub fn is_running(&self) -> bool {
        self.open.as_ref().is_some_and(|open| open.run.as_ref().is_some_and(|run| !run.sent))
    }

    pub fn set_cursor(&mut self, cursor: Option<(f32, f32)>) {
        self.cursor = cursor;
    }

    pub fn grow_by(&self, layout: widgets::Layout) -> f32 {
        layout.fit(EXTENT)
    }

    /// Which job is under the cursor, if any. Exposed for the tests, which
    /// assert that a row answers where it is drawn.
    pub fn hovered(&self) -> Option<Job> {
        let open = self.open.as_ref()?;
        let at = self.cursor?;
        let panel = Panel::for_game(open.game);
        jobs_of(open.game)
            .iter()
            .enumerate()
            .find(|&(index, _)| panel.row(index).contains(at.0, at.1))
            .map(|(_, &job)| job)
    }

    /// Which game a blow would land on, if a run is up at all.
    ///
    /// **Asked before the press, not after.** The caller wants a sound
    /// for the hammer or the wheel, and `press` and `click` answer with
    /// an [`Intent`] that is `None` for every blow but the last -- so
    /// "did that click strike something" cannot be read off the return.
    /// A job picked from the list is not a blow and answers `None` here,
    /// which is what keeps clicking a menu row from ringing an anvil.
    pub fn striking(&self) -> Option<Game> {
        let open = self.open.as_ref()?;
        open.run.as_ref().filter(|run| !run.sent).map(|_| open.game)
    }

    /// A left click. Picks a job, or lands a blow if a run is up.
    pub fn click(&mut self) -> Option<Intent> {
        let at = self.cursor?;
        let open = self.open.as_ref()?;
        let panel = Panel::for_game(open.game);
        if panel.close.contains(at.0, at.1) {
            return Some(Intent::Close);
        }
        // **While a run is up, the whole panel is the hammer.** A player
        // watching a marker should not also be aiming a mouse: anywhere on the
        // screen is a blow, and the button is drawn only to say so. The
        // rejected version had a small strike button, and missing it cost a
        // blow -- which is a game about clicking a button rather than about
        // timing.
        if open.run.is_some() {
            return self.press();
        }
        let job = self.hovered()?;
        if let Some(open) = self.open.as_mut() {
            open.pending = Some(job);
        }
        Some(Intent::Begin(job))
    }

    /// A blow: the strike key, a tap, or a click while a run is up.
    pub fn press(&mut self) -> Option<Intent> {
        let open = self.open.as_mut()?;
        let run = open.run.as_mut()?;
        if run.sent {
            return None;
        }
        let game = open.game;
        let at = elapsed_ms(run.began);
        // **One press a sweep, and a sweep with nothing in it is a miss.**
        // Nothing is filled in for a blow the player did not strike: the run
        // that goes up is short, and the server scores the empty sweeps as
        // nothing (`minigame::judge`). A filler timestamp would have to land
        // somewhere on the bar, and at the wrong end of a wide sweet spot a
        // "miss" scored better than half a hit.
        let window = (at / game.step_ms()) as usize;
        // A second blow inside the same sweep, one before the marker has
        // moved, or one a hair after the last across a sweep's edge, is let
        // pass rather than counted: each is a rule the server throws the
        // whole run out for, and a player leaning on the key must not lose
        // their bar to it. See `minigame::press_counts`.
        if !minigame::press_counts(game, run.presses.last().copied(), at) {
            return None;
        }
        run.presses.push(at);
        if window + 1 == game.presses() {
            // The last sweep has had its blow; there is nothing left to
            // watch, so the run goes up now rather than after the marker has
            // finished a lap nobody is playing.
            run.sent = true;
            return Some(Intent::Run(run.presses.clone()));
        }
        None
    }

    /// Once a frame. Ends a run the player stopped pressing in: the windows
    /// have all gone by, so whatever was struck is what there is.
    pub fn poll(&mut self) -> Option<Intent> {
        let open = self.open.as_mut()?;
        let game = open.game;
        let run = open.run.as_mut()?;
        if run.sent || elapsed_ms(run.began) < game.run_ms() {
            return None;
        }
        run.sent = true;
        Some(Intent::Run(run.presses.clone()))
    }

    /// A screen already `at_ms` into a run, with no server to open it.
    ///
    /// Test-only, and it exists because the honest path is three messages
    /// long: a click, an answer, a begin and a seed. The snapshot harness
    /// (`ui::snapshot`) has none of them, and neither do the tests that are
    /// about what a running bar *looks* like.
    #[cfg(test)]
    pub fn mid_run(game: Game, tolerance: f32, job: Job, seed: u32, at_ms: u32) -> StationScreen {
        let mut screen = StationScreen::new();
        screen.asked_to_open();
        screen.show(game, tolerance);
        if let Some(open) = screen.open.as_mut() {
            open.pending = Some(job);
        }
        screen.begun(seed);
        if let Some(run) = screen.open.as_mut().and_then(|open| open.run.as_mut()) {
            run.began = Instant::now() - std::time::Duration::from_millis(u64::from(at_ms));
        }
        screen
    }

    /// Everything on the screen, appended to the overlay.
    pub fn build_into(&self, font: FontAtlas, layers: &FaceLayers, language: Language, out: &mut Vec<HotbarVertex>) {
        let Some(open) = self.open.as_ref() else {
            return;
        };
        let panel = Panel::for_game(open.game);
        let mut p = Painter::onto(font, std::mem::take(out));
        p.scrim(widgets::SCRIM);
        // **The deep panel and an amber heading from the left edge**, which
        // is what the pack, the chest, the hearth and the death notice are.
        // This screen was a flat slab with its heading centred in the ink,
        // written beside the others rather than out of them, and walking
        // from a chest to an anvil changed the skin of the interface.
        p.deep_panel(panel.frame);
        // **The heading names the job once a run is up.** A player four
        // seconds into a run is watching a marker and has stopped reading;
        // what they may still need is which of the two things on the list
        // they actually started, because the screen they picked it on is
        // gone.
        let title = match open.run.as_ref() {
            Some(run) => job_label(run.job, language),
            None => std::borrow::Cow::Borrowed(language.text(match open.game {
                Game::Anvil => Msg::AnvilTitle,
                Game::Wheel => Msg::WheelTitle,
                Game::Saw => Msg::SawhorseTitle,
                Game::Whet => Msg::HoningTitle,
            })),
        };
        p.text(&title, panel.frame.x0 + PAD, panel.title_top, TITLE_SCALE, widgets::ACCENT);
        p.button(panel.close, language.text(Msg::Cancel), self.cursor.is_some_and(|at| panel.close.contains(at.0, at.1)), true);

        match open.run.as_ref() {
            Some(run) => self.draw_run(&mut p, &panel, open, run, language),
            None => self.draw_jobs(&mut p, &panel, open, layers, language),
        }
        *out = p.into_vertices();
    }

    fn draw_jobs(&self, p: &mut Painter, panel: &Panel, open: &Open, layers: &FaceLayers, language: Language) {
        for (index, &job) in jobs_of(open.game).iter().enumerate() {
            let row = panel.row(index);
            let hovered = self.cursor.is_some_and(|at| row.contains(at.0, at.1));
            // **The words in the part of the button right of the picture**,
            // not centred on the whole of it: in the sawhorse's half-width
            // rows a centred "Storage box" ran into the chest drawn at its
            // left end. The slab is still the whole row, so the click is.
            let label = job_label(job, language);
            p.button(row, "", hovered, true);
            let words = panel.words(index);
            p.label_in(words, &label, widgets::button_label_scale(words, &label, 1.0), widgets::INK);
            // **The picture of what comes off it, beside the words.** Three
            // rows at the wheel say "unfired" three times, and a player
            // scanning for the jug reads a shape faster than a word -- the
            // reason every slot in the pack is a picture. Drawn *inside* the
            // button and not beside it, so the button is still the whole of
            // what answers a click (`Panel::icon`,
            // `a_job_is_clicked_on_its_picture_as_on_its_words`).
            if let Some(made) = job_product(job) {
                crate::ui::inventory_screen::textured(
                    p,
                    panel.icon(index),
                    crate::ui::inventory_screen::icon_layer(layers, made),
                    crate::ui::hotbar::icon_tint(made, [1.0, 1.0, 1.0, 1.0]),
                );
            }
        }
        // What the last run came to, and what came off it -- "well struck,
        // 18 nails", because a verdict with no count under it makes a player
        // open their pack to find out whether it worked.
        let note = match open.result {
            Some((verdict, made)) => {
                let said = language.text(match verdict {
                    Verdict::Fine => Msg::RunFine,
                    Verdict::Fair => Msg::RunFair,
                    Verdict::Ruined => Msg::RunRuined,
                });
                match made {
                    Some((block, count)) => {
                        format!("{said} - {}", crate::ui::names::stack_line(block, count, language))
                    }
                    None => said.to_string(),
                }
            }
            None => language.text(Msg::StationPickJob).to_string(),
        };
        let colour = match open.result {
            Some((Verdict::Fine, _)) => widgets::TEXT_GOOD,
            Some((Verdict::Ruined, _)) => widgets::TEXT_BAD,
            _ => widgets::INK_DIM,
        };
        p.text_centred(&note, panel.frame.centre_x(), panel.note_top, NOTE_SCALE, colour);
    }

    fn draw_run(&self, p: &mut Painter, panel: &Panel, open: &Open, run: &Run, language: Language) {
        let game = open.game;
        let at = elapsed_ms(run.began);
        let step = (at / game.step_ms()) as usize;
        let step = step.min(game.presses() - 1);
        p.well(panel.bar, widgets::WELL);
        // The sweet spot, at the width the server will judge by.
        let centre = minigame::target(run.seed, step);
        let low = (centre - open.tolerance).clamp(0.0, 1.0);
        let high = (centre + open.tolerance).clamp(0.0, 1.0);
        p.quad(panel.along(low, high), widgets::TEXT_GOOD);
        // ...and the heart of it, which is what a fine blow needs.
        let heart = open.tolerance * (1.0 - 0.55);
        p.quad(
            panel.along((centre - heart).clamp(0.0, 1.0), (centre + heart).clamp(0.0, 1.0)),
            widgets::ACCENT,
        );
        // The marker. Drawn last so it is never hidden by the spot it is
        // crossing -- the one thing on this screen the eye is actually
        // tracking.
        let marker = minigame::marker_at(game, at);
        let half = MARKER_HALF_WIDTH;
        p.quad(panel.along((marker - half).max(0.0), (marker + half).min(1.0)), widgets::INK);
        // How many blows are left, as pips rather than a number: a count that
        // has to be *read* is a count nobody reads while a marker is moving.
        for blow in 0..game.presses() {
            let pip = panel.pip(blow, game.presses());
            let struck = blow < run.presses.len();
            // The blows to come in the dim ink, not in `WELL_DARK`: a dark
            // pip on the dark panel measured next to nothing, and three
            // blows left read as no pips at all.
            p.quad(pip, if struck { widgets::ACCENT } else { widgets::INK_DIM });
        }
        p.text_centred(
            language.text(crate::ui::lang::by_input(Msg::StationStrike, Msg::StationStrikeTouch)),
            panel.frame.centre_x(),
            panel.note_top,
            NOTE_SCALE,
            widgets::INK_DIM,
        );
    }
}

/// How wide the marker is drawn, as a fraction of the bar. Narrow enough to
/// read as a position and wide enough to see at speed.
const MARKER_HALF_WIDTH: f32 = 0.012;

/// Milliseconds since `began`, saturating: a screen left open over a clock
/// change must not wrap into a run that already finished.
fn elapsed_ms(began: Instant) -> u32 {
    began.elapsed().as_millis().min(u128::from(u32::MAX)) as u32
}

/// What a job makes, for its picture: what a good run leaves in the pack.
/// The good run and not the fair one because they are the same thing and a
/// ruined run makes nothing -- and a row with no picture would be a row that
/// looks like it makes nothing.
///
/// **The whetstone for the hone**, which makes nothing: its row is the
/// gesture, and the stone is the picture a player already knows for it.
pub fn job_product(job: Job) -> Option<BlockId> {
    if job == Job::Hone {
        return Some(primitive_shared::types::BLOCK_WHETSTONE);
    }
    minigame::outcome(job, Verdict::Fine).made.map(|(block, _)| block)
}

/// What a job's row and a running bar's heading say.
///
/// The thing it makes, by its block name -- except the hone, which makes
/// nothing and is named as the gesture the crafting menu already calls it
/// ("hone"), so the stone and the menu say the same word for the same work.
pub fn job_label(job: Job, language: Language) -> std::borrow::Cow<'static, str> {
    if job == Job::Hone {
        return crate::ui::names::recipe("hone", language);
    }
    crate::ui::names::identified(job.name(), language)
}

/// The jobs a station offers, in the order it lists them.
pub fn jobs_of(game: Game) -> &'static [Job] {
    match game {
        Game::Anvil => &[Job::Nails, Job::Helm],
        Game::Wheel => &[Job::Vessel, Job::Jug, Job::Mould, Job::Bowl],
        Game::Whet => &[Job::Hone],
        // The camp's pieces first, then the house's.
        Game::Saw => &[Job::Stool, Job::Chest, Job::Barrel, Job::Chair, Job::Table, Job::Door, Job::Bed],
    }
}

/// How many rows fit between the heading and the line under the list. A
/// station with more jobs than this lays them in two columns.
const ROWS_IN_A_COLUMN: usize = 4;

/// Every rectangle on the screen, worked out once.
///
/// **The whole of the layout, and the only copy of it.** `build_into` draws
/// out of this and `click` hit-tests against it, so a button cannot move
/// without its hit area moving with it -- the rule `ui::widgets` states and
/// `a_station_row_is_clicked_where_it_is_drawn` holds to.
pub struct Panel {
    pub frame: Rect,
    pub close: Rect,
    pub bar: Rect,
    title_top: f32,
    note_top: f32,
    rows_top: f32,
    /// One, or two for the sawhorse's seven pieces.
    ///
    /// **Two columns rather than a taller panel or a scrolling list.** A
    /// taller panel is the size jump `for_game` refuses; a list that scrolls
    /// hides the bed below a stool, and the one thing a list of seven must
    /// not do is make a player hunt for the piece they came to cut.
    columns: usize,
}

impl Panel {
    pub fn for_game(game: Game) -> Panel {
        // One shape for both, because they hold the same things: a heading,
        // a short list, a bar and a line of words. A wheel panel sized to its
        // own three rows would jump when a player walked from one station to
        // the other, and a screen that changes size for no reason the player
        // caused reads as a glitch.
        let frame = Rect::centred(0.0, 0.0, PANEL_HALF_WIDTH * 2.0, PANEL_HALF_HEIGHT * 2.0);
        let title_top = frame.y1 - PAD - widgets::cell_height(TITLE_SCALE);
        let rows_top = title_top - 0.06;
        let bar = Rect::new(frame.x0 + PAD, -BAR_HEIGHT / 2.0, frame.x1 - PAD, BAR_HEIGHT / 2.0);
        let close = Rect::new(
            frame.x1 - PAD - CLOSE_WIDTH,
            frame.y0 + PAD,
            frame.x1 - PAD,
            frame.y0 + PAD + ROW_HEIGHT * 0.7,
        );
        // **Above the button and not beside it.** The line is centred on the
        // panel and the button sits in the right-hand corner, so a line long
        // enough -- "Бейте мышью или пробелом по метке" is half again as long
        // as the English -- ran straight through it. Measured off the button's
        // own top edge, so the two cannot drift apart.
        let note_top = close.y1 + 0.03 + widgets::cell_height(NOTE_SCALE);
        let columns = if jobs_of(game).len() > ROWS_IN_A_COLUMN { 2 } else { 1 };
        Panel { frame, close, bar, title_top, note_top, rows_top, columns }
    }

    /// Job row `index`, counted down from under the heading -- across first,
    /// then down, when there are two columns.
    pub fn row(&self, index: usize) -> Rect {
        let (line, column) = (index / self.columns, index % self.columns);
        let top = self.rows_top - line as f32 * (ROW_HEIGHT + ROW_GAP);
        let span = (self.frame.width() - 2.0 * PAD - (self.columns - 1) as f32 * ROW_GAP) / self.columns as f32;
        let x0 = self.frame.x0 + PAD + column as f32 * (span + ROW_GAP);
        Rect::new(x0, top - ROW_HEIGHT, x0 + span, top)
    }

    /// Where job row `index` draws its picture: a square at the row's left
    /// end, inset from the edge the way a slot's icon is. **Inside the row**,
    /// which is the whole of why the hit-test did not have to change.
    pub fn icon(&self, index: usize) -> Rect {
        let row = self.row(index);
        let inset = ROW_HEIGHT * 0.12;
        let side = row.height() - 2.0 * inset;
        Rect::new(row.x0 + inset * 1.5, row.y0 + inset, row.x0 + inset * 1.5 + side, row.y1 - inset)
    }

    /// Where job row `index` letters its name: the row right of its picture,
    /// with the same air on the far side. See `draw_jobs`.
    pub fn words(&self, index: usize) -> Rect {
        let (row, icon) = (self.row(index), self.icon(index));
        let air = icon.x0 - row.x0;
        Rect::new(icon.x1 + air, row.y0, row.x1 - air, row.y1)
    }

    /// A slice of the bar between two fractions of its width.
    pub fn along(&self, from: f32, to: f32) -> Rect {
        let width = self.bar.width();
        Rect::new(
            self.bar.x0 + from * width,
            self.bar.y0,
            self.bar.x0 + to.max(from) * width,
            self.bar.y1,
        )
    }

    /// One of the pips that count the blows, under the bar.
    pub fn pip(&self, index: usize, of: usize) -> Rect {
        let span = 0.05;
        let total = of as f32 * span;
        let left = -total / 2.0 + index as f32 * span;
        let top = self.bar.y0 - 0.03;
        Rect::new(left + span * 0.15, top - 0.022, left + span * 0.85, top)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opened(game: Game) -> StationScreen {
        let mut screen = StationScreen::new();
        screen.asked_to_open();
        screen.show(game, minigame::tolerance(None));
        screen
    }

    fn drawn(screen: &StationScreen) -> Vec<HotbarVertex> {
        let mut out = Vec::new();
        screen.build_into(FontAtlas::for_test(), &FaceLayers::empty_for_test(), Language::English, &mut out);
        out
    }

    #[test]
    fn a_station_row_is_clicked_where_it_is_drawn() {
        for game in [Game::Anvil, Game::Wheel, Game::Whet, Game::Saw] {
            let mut screen = opened(game);
            let panel = Panel::for_game(game);
            for (index, &job) in jobs_of(game).iter().enumerate() {
                let row = panel.row(index);
                // The middle of the row, and each of its four corners pulled a
                // hair inside: a click anywhere on the button is that button.
                let inside = [
                    (row.centre_x(), row.centre_y()),
                    (row.x0 + 0.001, row.y0 + 0.001),
                    (row.x1 - 0.001, row.y1 - 0.001),
                ];
                for at in inside {
                    screen.set_cursor(Some(at));
                    assert_eq!(screen.hovered(), Some(job), "{game:?} row {index} missed at {at:?}");
                    assert_eq!(screen.click(), Some(Intent::Begin(job)));
                }
                // ...and just outside the top edge is not.
                screen.set_cursor(Some((row.centre_x(), row.y1 + 0.005)));
                assert_ne!(screen.hovered(), Some(job), "{game:?} row {index} answers above itself");
            }
            // Every row is inside the panel it is drawn on, and no two
            // overlap: a list that ran off the bottom would be a job nobody
            // can click.
            for index in 0..jobs_of(game).len() {
                let row = panel.row(index);
                assert!(row.y0 > panel.frame.y0 && row.y1 < panel.frame.y1, "row {index} is off the panel");
            }
        }
    }

    #[test]
    fn a_job_is_clicked_on_its_picture_as_on_its_words() {
        for game in [Game::Anvil, Game::Wheel, Game::Whet, Game::Saw] {
            let mut screen = opened(game);
            let panel = Panel::for_game(game);
            for (index, &job) in jobs_of(game).iter().enumerate() {
                assert!(job_product(job).is_some(), "{job:?} has no picture to draw");
                let (row, icon) = (panel.row(index), panel.icon(index));
                // The picture is inside its own button, square, and clear of
                // the words centred in it.
                assert!(
                    icon.x0 > row.x0 && icon.x1 < row.x1 && icon.y0 > row.y0 && icon.y1 < row.y1,
                    "{game:?} row {index}: the picture {icon:?} sticks out of its button {row:?}"
                );
                assert!((icon.width() - icon.height()).abs() < 1e-5, "{game:?} row {index}: the picture is not square");
                // The longest of the four languages, at the size the button
                // letters it at.
                let words = Language::ALL
                    .iter()
                    .map(|&language| {
                        let text = job_label(job, language);
                        widgets::measure(&text, widgets::button_label_scale(panel.words(index), &text, 1.0))
                    })
                    .fold(0.0f32, f32::max);
                assert!(
                    icon.x1 < panel.words(index).centre_x() - words / 2.0,
                    "{game:?} row {index}: the picture runs into the words"
                );
                // ...and a click on it is a click on the job.
                screen.set_cursor(Some((icon.centre_x(), icon.centre_y())));
                assert_eq!(screen.hovered(), Some(job));
                assert_eq!(screen.click(), Some(Intent::Begin(job)), "{game:?}: a click on the picture of {job:?}");
            }
        }
    }

    #[test]
    fn at_every_window_shape_and_size_and_on_a_phone_every_job_is_on_screen_and_clicked_where_it_is_drawn() {
        // Through the frame loop's own two transforms: the drawing grown
        // about the centre (`scale_about`, `anchor::CENTRE`) and the pointer
        // brought back by `Layout::hit` -- the pair `place_cursor` uses.
        let check = |aspect: f32, requested: f32| {
            let layout = widgets::Layout::for_screen(aspect, requested);
            for game in [Game::Anvil, Game::Wheel, Game::Whet, Game::Saw] {
                let mut screen = opened(game);
                let grown = screen.grow_by(layout);
                let panel = Panel::for_game(game);
                let (cx, cy) = widgets::anchor::CENTRE(aspect);
                let on_screen = |x: f32, y: f32| (cx + (x - cx) * grown, cy + (y - cy) * grown);
                let (x0, y0) = on_screen(panel.frame.x0, panel.frame.y0);
                let (x1, y1) = on_screen(panel.frame.x1, panel.frame.y1);
                assert!(
                    x0 >= -aspect - 1e-4 && x1 <= aspect + 1e-4 && y0 >= -1.0 - 1e-4 && y1 <= 1.0 + 1e-4,
                    "{game:?} at {aspect}:1, size {requested}: the panel runs off the window"
                );
                for (index, &job) in jobs_of(game).iter().enumerate() {
                    let row = panel.row(index);
                    let pointer = on_screen(row.centre_x(), row.centre_y());
                    screen.set_cursor(Some(layout.hit(pointer, grown)));
                    assert_eq!(screen.hovered(), Some(job), "{game:?} at {aspect}:1, size {requested}: row {index}");
                }
            }
        };
        // Landscape only, and square: the phone build is locked to landscape
        // (`android:screenOrientation="sensorLandscape"`), and a desktop
        // window narrower than it is tall is not a shape anything here is
        // drawn for -- no screen shrinks below its own size (`Layout::fit`).
        for aspect in [4.0 / 3.0, 16.0 / 9.0, 20.0 / 9.0, 21.0 / 9.0, 1.0] {
            for requested in [0.75, 1.0, 1.5, 2.0] {
                check(aspect, requested);
                widgets::as_a_phone(|| check(aspect, requested));
            }
        }
    }

    #[test]
    fn a_run_is_begun_struck_and_handed_in_and_the_screen_shows_what_came_of_it() {
        // The whole of the client's half, the way the frame loop drives it:
        // a click on a row, the server's seed, the blows, the hand-in -- and
        // the key the interface is rebuilt by changes at every step, which is
        // what makes each of them reach the screen at all.
        let mut screen = opened(Game::Wheel);
        let mut keys = vec![screen.ui_key()];
        let row = Panel::for_game(Game::Wheel).row(0);
        screen.set_cursor(Some((row.centre_x(), row.centre_y())));
        keys.push(screen.ui_key());
        assert_eq!(screen.click(), Some(Intent::Begin(Job::Vessel)));
        keys.push(screen.ui_key());
        assert!(!screen.is_running(), "the bar moved before the server said begin");
        screen.begun(9);
        assert!(screen.is_running(), "a begun run is not redrawn every frame");
        keys.push(screen.ui_key());
        screen.finished(Verdict::Fine, Some((primitive_shared::types::BLOCK_VESSEL_RAW, 2)));
        assert!(!screen.is_running());
        keys.push(screen.ui_key());
        for pair in keys.windows(2) {
            assert_ne!(pair[0], pair[1], "a step of the run left the interface key as it was, so it was never drawn");
        }
    }

    #[test]
    fn the_sawhorse_lays_its_seven_pieces_in_two_columns_that_neither_overlap_nor_run_into_the_note() {
        let panel = Panel::for_game(Game::Saw);
        let jobs = jobs_of(Game::Saw);
        assert_eq!(jobs.len(), 7, "the sawhorse lost or gained a piece");
        for a in 0..jobs.len() {
            let row = panel.row(a);
            assert!(row.y0 > panel.note_top, "{:?} runs down into the line under the list", jobs[a]);
            assert!(row.x0 >= panel.frame.x0 && row.x1 <= panel.frame.x1, "{:?} is off the panel", jobs[a]);
            for b in a + 1..jobs.len() {
                let other = panel.row(b);
                let apart = row.x1 <= other.x0 || other.x1 <= row.x0 || row.y1 <= other.y0 || other.y1 <= row.y0;
                assert!(apart, "{:?} and {:?} are drawn on top of each other", jobs[a], jobs[b]);
            }
        }
        // ...and the honing stone has one row, a whole panel wide.
        assert_eq!(Panel::for_game(Game::Whet).row(0).width(), Panel::for_game(Game::Anvil).row(0).width());
    }

    #[test]
    fn the_wheel_lists_the_bowl_and_every_row_fits_above_the_note() {
        let panel = Panel::for_game(Game::Wheel);
        assert!(jobs_of(Game::Wheel).contains(&Job::Bowl), "the bowl is not on the wheel");
        let last = panel.row(jobs_of(Game::Wheel).len() - 1);
        // The line under the list and the last row do not share a pixel.
        assert!(
            last.y0 > panel.note_top,
            "the last job ({:?}) runs down into the line under the list at {}",
            last,
            panel.note_top
        );
    }

    #[test]
    fn the_close_button_shuts_the_screen_and_nothing_else_does() {
        let mut screen = opened(Game::Anvil);
        let panel = Panel::for_game(Game::Anvil);
        screen.set_cursor(Some((panel.close.centre_x(), panel.close.centre_y())));
        assert_eq!(screen.click(), Some(Intent::Close));
        // A click on bare panel does nothing at all -- it does not start a
        // job and it does not shut the screen out from under the player.
        screen.set_cursor(Some((panel.frame.x0 + 0.005, panel.frame.y0 + 0.005)));
        assert_eq!(screen.click(), None);
    }

    #[test]
    fn a_run_is_one_press_a_window_and_is_handed_in_whole() {
        let game = Game::Anvil;
        // A fifth of a second in: inside the first window, and past the
        // moment the marker has moved (`minigame::MIN_FIRST_MS`).
        let mut screen = StationScreen::mid_run(game, minigame::tolerance(None), Job::Nails, 0x1234, 200);
        // Struck now, which is inside the first window. A second blow in the
        // same window is ignored rather than added -- the server refuses a
        // run that is not one press a blow, and a player leaning on the key
        // must not lose their bar to that.
        assert_eq!(screen.press(), None);
        assert_eq!(screen.press(), None);
        let Some(open) = screen.open.as_ref() else { panic!("the screen shut itself") };
        let run = open.run.as_ref().expect("the run is gone");
        assert_eq!(run.presses.len(), 1, "two blows landed in one sweep");
        assert!(!run.sent);
    }

    /// The hammer sounds for a blow and not for picking a job off the
    /// list, and it stops sounding the moment the run has gone up.
    #[test]
    fn a_blow_is_a_blow_and_choosing_a_job_is_not() {
        let mut screen = StationScreen::new();
        assert_eq!(screen.striking(), None, "a shut screen was being hammered");
        screen.asked_to_open();
        screen.show(Game::Anvil, minigame::tolerance(None));
        assert_eq!(screen.striking(), None, "the list of jobs rang like an anvil");

        let game = Game::Wheel;
        let mut screen =
            StationScreen::mid_run(game, minigame::tolerance(None), Job::Jug, 5, game.step_ms() / 2);
        assert_eq!(screen.striking(), Some(game));
        // Every sweep but the last leaves the run up; the last hands it in
        // and there is nothing left to strike.
        for window in 0..game.presses() {
            let at = window as u32 * game.step_ms() + game.step_ms() / 2;
            let run = screen.open.as_mut().and_then(|o| o.run.as_mut()).expect("a run");
            run.began = Instant::now() - std::time::Duration::from_millis(u64::from(at));
            assert_eq!(screen.striking(), Some(game), "sweep {window} was not strikable");
            screen.press();
        }
        assert_eq!(screen.striking(), None, "a handed-in run was still ringing");
    }

    #[test]
    fn a_run_nobody_pressed_in_is_still_a_run_and_is_a_spoiled_one() {
        // The player opened the screen, started a job and walked away. The
        // server has their clay; what goes up has to be *something*, and an
        // empty run is what it is -- nothing is filled in on their behalf.
        let game = Game::Wheel;
        let mut screen =
            StationScreen::mid_run(game, minigame::tolerance(None), Job::Jug, 77, game.run_ms() + 50);
        let Some(Intent::Run(presses)) = screen.poll() else { panic!("a finished run was never handed in") };
        assert!(presses.is_empty(), "a blow nobody struck was invented");
        let score = minigame::judge(game, 77, minigame::tolerance(None), &presses, game.run_ms() + 50)
            .expect("the client built a run its own server would refuse");
        assert_eq!(minigame::verdict(score), Verdict::Ruined);
        // ...and it is handed in once.
        assert_eq!(screen.poll(), None);
    }

    #[test]
    fn a_run_the_client_builds_is_a_run_the_server_will_take() {
        // Every shape the screen can produce -- a full run, a run with a
        // sweep missed in the middle, a run where only the last blow landed
        // -- has to pass `judge`. A client that built a run its own server
        // refuses would cost the player the material for nothing, and the
        // rule and the collector are in different crates.
        for game in [Game::Anvil, Game::Wheel, Game::Whet, Game::Saw] {
            for skip in 0..game.presses() {
                let mut screen = StationScreen::mid_run(game, minigame::tolerance(None), jobs_of(game)[0], 31, 0);
                let mut handed = None;
                for window in 0..game.presses() {
                    if window == skip {
                        continue;
                    }
                    // Halfway up the sweep, which is where a player aiming at
                    // a sweet spot would be.
                    let at = window as u32 * game.step_ms() + game.step_ms() / 4;
                    let run = screen.open.as_mut().and_then(|o| o.run.as_mut()).expect("a run");
                    run.began = Instant::now() - std::time::Duration::from_millis(u64::from(at));
                    if let Some(intent) = screen.press() {
                        handed = Some(intent);
                    }
                }
                let presses = match handed.or_else(|| screen.poll()) {
                    Some(Intent::Run(presses)) => presses,
                    _ => {
                        let run = screen.open.as_ref().and_then(|o| o.run.as_ref()).expect("a run");
                        run.presses.clone()
                    }
                };
                assert_eq!(presses.len(), game.presses() - 1, "{game:?} skipping {skip}");
                minigame::judge(game, 31, minigame::tolerance(None), &presses, game.run_ms())
                    .expect("the client built a run the server refuses");
            }
        }
    }

    #[test]
    fn a_shut_screen_draws_nothing_and_an_open_one_draws_its_rows() {
        let mut screen = StationScreen::new();
        assert!(drawn(&screen).is_empty(), "a shut station screen drew something");
        // ...and an answer nobody asked for is ignored: a screen that opened
        // on an old reply would open for a station the player has left.
        screen.show(Game::Anvil, 0.2);
        assert!(!screen.is_open());
        let screen = opened(Game::Wheel);
        assert!(drawn(&screen).len() > 6);
    }
}
