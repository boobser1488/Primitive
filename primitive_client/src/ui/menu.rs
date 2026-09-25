//! The menus: main screen, server list, the add/edit form, connection
//! progress, and the in-game pause screen.
//!
//! This module owns the *model* and the *layout*. It knows nothing about
//! sockets or windows: `update` turns a click or a keypress into an
//! [`Action`], and `main.rs` decides what an action means. That split is
//! what lets the whole thing be tested without a GPU or a server.
//!
//! ## Servers are editable in the game
//!
//! The list previously lived only in `servers.toml`, so adding a server
//! meant quitting, finding a file, editing TOML by hand and starting
//! again -- and a typo in it silently reverted the whole list to the
//! default. Now the list is edited on screen and written back on every
//! change, and the file is a persistence format rather than a user
//! interface.
//!
//! ## Everything is clickable and everything has a key
//!
//! Both, always. Mouse because a menu you have to read a legend to
//! operate is not a menu; keys because the player's hand is already on
//! the keyboard between sessions and because Enter/Escape is how the
//! shortest paths through this screen should feel.

use serde::{Deserialize, Serialize};

use crate::logic::menu_scene::Place;
use crate::settings::ClientSettings;
use crate::ui::field::{Motion, TextField};
use crate::ui::widgets::{self, Painter, Rect};
use crate::logic::worlds::{self, Worlds};
use crate::ui::lang::{Language, Msg};
use primitive_shared::worldgen::{Preset, Scale, Zone};

/// The skin every screen in this file is drawn in. See `widgets::Theme`.
const MENU: widgets::Theme = widgets::Theme::DARK;

/// How dark the world behind a menu is pushed.
///
/// Nearly opaque: a menu is what you look at *instead of* the world, and
/// a legible list matters more than a glimpse of the sky behind it.
const MENU_SCRIM: [f32; 4] = [0.03, 0.035, 0.05, 0.86];

/// How bright the menu's own world actually gets, as a luminance, and
/// how hard the interface has to push it back.
///
/// **Measured, not guessed, and measured twice because there are two
/// answers.** `the_menu_backdrop_through_the_real_shader` -- the GPU
/// tool at the bottom of `engine::renderer` -- builds each of the four
/// places, renders it from the menu's own camera through the real
/// shaders, and prints the luminance the frame reaches at four
/// percentiles, converted out of sRGB into the linear light the
/// interface's own colours are written in. See `widgets::contrast` for
/// why that conversion is the difference between a number and a wrong
/// number.
///
/// Across three worlds, the three outdoor places all landed within a
/// few hundredths of each other -- 0.51 to 0.573 at the ninety-ninth
/// percentile, because what is bright in all three is the same sunset
/// sky. Underground the same measurement is 0.006 to 0.033: there is no
/// sky in it. So there are two figures here rather than four, and they
/// are the two things a backdrop can be: outdoors, or not.
///
/// Why a percentile and not the maximum. Outdoors the top hundredth of
/// the frame is the sun's own disc, which reaches 1.0; underground it
/// is a shaft of daylight where the cave breaks the surface. Sized to
/// *those*, the veil is 91% opaque and the scene is gone -- which is
/// the tiled wallpaper this replaced, arrived at by a longer road. Text
/// that lands on the sun's disc comes to 3.3:1, which is the large-text
/// floor rather than the body one; it is also exactly what the plain
/// backdrop has always done, because the sky pass draws that same sun
/// behind the menu with `MENU_SCRIM` over it.
///
/// Only the test measures; nothing in the game asks at run time, so it
/// is not compiled into the game. It stays here rather than in the test
/// module because it is what the veils below are *derived from*, and
/// anybody changing one should meet it on the way past. Same
/// arrangement, for the same reason, as `widgets::contrast`.
#[cfg(test)]
const SCENE_HIGHLIGHT_OUTDOORS: f32 = 0.58;
#[cfg(test)]
const SCENE_HIGHLIGHT_UNDERGROUND: f32 = 0.04;

/// How dark a shore, a wood or a meadow behind the menus is pushed.
///
/// The alpha is the smallest hundredth that puts `TEXT_DIM` over
/// `SCENE_HIGHLIGHT_OUTDOORS` at the 4.5:1 body-text floor. Both halves
/// of that -- that it is enough, and that it is not more than enough --
/// are checked by `every_word_over_the_menu_scene_is_readable_at_its_worst`.
const VEIL_OUTDOORS: [f32; 4] = [0.03, 0.035, 0.05, 0.85];

/// ...and a cave.
///
/// **Not a legibility figure, and this is the one veil that is not.**
/// A cave measures at a hundredth of a sunset, so the body-text floor
/// is met with no veil at all -- and the same 0.85 that a shore needs
/// would turn the one place with no sky in it into a black rectangle,
/// which is a menu with the feature switched off wearing the cost of
/// having it on. What this alpha is for is the interface: enough
/// separation that the dark panels read as panels standing in front of
/// rock rather than as holes in it.
const VEIL_UNDERGROUND: [f32; 4] = [0.03, 0.035, 0.05, 0.35];

/// Which of the two a place gets.
const fn veil_for(place: Place) -> [f32; 4] {
    match place {
        Place::Cave => VEIL_UNDERGROUND,
        _ => VEIL_OUTDOORS,
    }
}

/// What the finger has hold of on the arrangement screen.
///
/// The stick is not one of the buttons -- it is a different thing with a
/// different shape -- and an index that sometimes meant the stick and
/// sometimes a button is the kind of thing that goes wrong quietly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ControlUnderHand {
    Stick,
    Button(usize),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerEntry {
    pub name: String,
    pub address: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerList {
    pub servers: Vec<ServerEntry>,
}

const SERVERS_PATH: &str = "servers.toml";

impl ServerList {
    pub fn load_or_default(fallback_address: &str) -> Self {
        match std::fs::read_to_string(SERVERS_PATH) {
            Ok(text) => match toml::from_str::<Self>(&text) {
                Ok(list) => {
                    println!("loaded {SERVERS_PATH} ({} server(s))", list.servers.len());
                    list
                }
                Err(e) => {
                    eprintln!("{SERVERS_PATH} is invalid ({e}); using the configured address");
                    Self::single(fallback_address)
                }
            },
            Err(_) => {
                let list = Self::single(fallback_address);
                list.save();
                list
            }
        }
    }

    fn single(address: &str) -> Self {
        Self {
            servers: vec![ServerEntry {
                name: "Local server".to_string(),
                address: address.to_string(),
            }],
        }
    }

    /// Writes the list back. Called after every edit -- there is no
    /// "save" step for the player to forget.
    pub fn save(&self) {
        match toml::to_string_pretty(self) {
            Ok(text) => {
                if let Err(e) = std::fs::write(SERVERS_PATH, text) {
                    eprintln!("could not write {SERVERS_PATH}: {e}");
                }
            }
            Err(e) => eprintln!("could not serialise the server list: {e}"),
        }
    }
}

/// What the client knows about what is extending the server it is on.
///
/// **Three states rather than an `Option<ExtensionList>`**, because the
/// two empty ones are not the same thing and a screen that could not
/// tell them apart would be wrong about the commonest case. A list that
/// has not arrived yet and a server running nothing look identical as
/// data and read as opposite answers: "wait a moment" against "there is
/// nothing here". The screen prints a different sentence for each.
///
/// Reset to [`Extensions::Unasked`] on every connection, so the list a
/// player is shown is always the list of the server they are on -- see
/// `Menu::forget_extensions`.
/// No `PartialEq`: nothing compares two of these, and the list inside
/// one carries a mod's own prose, which has no business being compared
/// for equality anywhere.
#[derive(Debug, Clone, Default)]
pub enum Extensions {
    /// Never asked on this connection.
    #[default]
    Unasked,
    /// The question went out and the answer has not come back.
    Waiting,
    /// What the server said.
    Known(primitive_shared::protocol::ExtensionList),
}

impl Extensions {
    /// The rows to draw, which is nothing at all unless an answer has
    /// arrived.
    fn items(&self) -> &[primitive_shared::protocol::ExtensionInfo] {
        match self {
            Extensions::Known(list) => &list.items,
            _ => &[],
        }
    }
}

/// Which screen the player is on.
#[derive(Debug, Clone, PartialEq)]
pub enum Screen {
    Main,
    /// Singleplayer worlds.
    Worlds,
    /// The new-world form: name and seed.
    CreatingWorld,
    /// The one-field form that gives a world a new name.
    ///
    /// **Its own screen rather than an edit in the row**, which was the
    /// other candidate. A row that turns into a field is a row whose
    /// height, hit test and scrolling all have two answers, on the one
    /// list in the game a phone scrolls with a thumb -- and the field
    /// would then be under the finger that opened it. A screen with one
    /// box on it is the same arrangement the new-world form already has,
    /// and the player has seen it.
    ///
    /// The index is carried in the screen rather than remembered in a
    /// field, for `Confirm`'s reason: it is impossible to arrive here and
    /// rename a world other than the one that was asked about.
    RenamingWorld(usize),
    Servers,
    /// The add/edit server form. `Some(index)` edits an existing entry.
    Editing(Option<usize>),
    /// Everything the game can change without a text editor.
    Settings,
    /// Where the thumb controls are arranged, by moving them.
    ///
    /// Its own screen rather than more rows on Settings, and for a
    /// sharper reason than the key bindings have: this one is not a
    /// list at all. What it shows is the controls themselves, at the
    /// size and in the place they will actually be, and the whole
    /// screen is the thing being edited.
    ///
    /// Offered only where there are thumb controls to arrange, which is
    /// exactly where the key bindings are no use -- the two swap places
    /// in the settings footer.
    TouchControls,
    /// Key bindings. Its own screen rather than more rows on Settings:
    /// eleven actions is already as long as that list, and rebinding
    /// swallows the next keypress, which the rest of the screen must
    /// not do.
    Controls,
    /// Who made what.
    Credits,
    /// What is extending the server this world is on: scripted plugins
    /// and native mods together, with what each one says about itself.
    ///
    /// **Reachable from the pause screen and from nowhere else**, and
    /// that is the whole of the placement argument. The list is a fact
    /// about a *server*, and the main menu is the part of the game with
    /// no server behind it -- an entry there would open a screen whose
    /// only honest content is "ask again once you are somewhere".
    Extensions,
    /// A yes/no gate in front of something irreversible.
    ///
    /// The confirmed action is carried in the screen rather than
    /// remembered in a field, so it is impossible to arrive here and
    /// confirm something other than what was asked about.
    Confirm {
        /// What is being asked, as a message rather than a string, so
        /// the screen re-renders in whatever language is current when
        /// it is *drawn* rather than when it was opened.
        question: Msg,
        detail: String,
        confirm_label: Msg,
        action: Box<Action>,
    },
    Connecting {
        label: String,
    },
    Failed {
        label: String,
        reason: String,
    },
    /// Shown over the world, so it deliberately doesn't cover it.
    Paused,
}

/// Two buttons side by side, brought back onto the glass.
///
/// `split` is how far each sits from the middle and `width` how wide it
/// is, both at the size that was asked for. At the top of the
/// interface-size range that pair is wider than a 4:3 window -- so both
/// are scaled by whatever it takes to fit, together, which keeps the
/// gap between them proportional instead of squeezing one of them into
/// the other.
fn side_by_side(layout: widgets::Layout, split: f32, width: f32) -> (f32, f32) {
    let reach = split + width / 2.0;
    let room = layout.edge();
    if reach <= room || reach <= 0.0 {
        return (split, width);
    }
    let shrink = room / reach;
    (split * shrink, width * shrink)
}

/// Air between two buttons standing side by side under a panel: the four
/// hundredths the settings screen has always left between CONTROLS and DONE.
const BUTTON_GAP: f32 = 0.04;

/// Columns `from..=to` of a row of `count` equal buttons laid across
/// `span`, with `gap` between neighbours.
///
/// **Why the buttons under a panel are cut from the panel.** Each screen
/// sized its own: three thirds of the width with a tenth taken off, four
/// quarters with a twenty-fifth, and a BACK half a unit wide under both.
/// No two screens agreed and nothing lined up with anything -- the world
/// list's row stopped five hundredths inside the panel's edges, the server
/// list's two, and BACK lined up with neither. Laid out of the panel's own
/// width the row's outer edges *are* the panel's, and a button alone on the
/// next row is one or two of the same columns, so the eye runs straight
/// down the screen along one set of lines.
///
/// Rejected: one button width for every screen. It is the obvious
/// consistency and it is wrong both ways -- four of the forms' buttons are
/// wider than a desktop panel, and one of the lists' is a sliver under a
/// phone's panel twice as wide.
fn columns(span: (f32, f32), count: usize, gap: f32, from: usize, to: usize) -> (f32, f32) {
    let count = count.max(1) as f32;
    let width = ((span.1 - span.0) - gap * (count - 1.0)) / count;
    (
        span.0 + from as f32 * (width + gap),
        span.0 + to as f32 * (width + gap) + width,
    )
}

/// Where a form's rows go and how big they are.
///
/// **The clearest case in this file of adapting rather than scaling.**
/// Two arrangements:
///
/// * **A label above its field**, which is what a desktop draws. It
///   reads well and it is tall: three rows of it come to most of a
///   screen.
/// * **A label beside its field**, on a phone -- the settings screen's
///   row. It is half the height, which is what puts every field in the
///   top half of the glass, where the on-screen keyboard cannot cover
///   the one being typed into.
///
/// The second is not a nicety. A field you cannot see while you are
/// typing into it is the same class of problem as a keyboard that never
/// appears, and dodging the keyboard by however tall it turns out to be
/// is not available -- see `Layout::keyboard_top`.
///
/// The desktop arrangement is not merely preferred there, it is pinned:
/// see `not_one_vertex_of_the_desktop_menu_moved`.
struct FormRows {
    /// Top of the panel the rows sit in.
    top: f32,
    /// Distance from one row to the next.
    pitch: f32,
    /// Height of the box that is typed into.
    field: f32,
    /// How far below a row's top the field starts -- the room the label
    /// takes on its own line. Zero when the label is beside it.
    label_drop: f32,
    /// Air above the first row, and below the last.
    head: f32,
    tail: f32,
    /// How much of the row the label owns when it sits beside the
    /// field, and zero when it does not.
    label_column: f32,
    /// How big the writing on the labels is.
    label_size: f32,
}

impl FormRows {
    /// Lays `count` rows out for this screen.
    ///
    /// `desktop` is the arrangement that has to come out unchanged on a
    /// desktop: the panel top, the pitch, and the air above and below.
    fn plan(layout: widgets::Layout, count: usize, desktop: (f32, f32, f32, f32)) -> Self {
        let (top, pitch, head, tail) = desktop;
        if let Some(_keyboard) = layout.keyboard_top() {
            // Packed against the top of the glass and onto one line
            // each, so that even the last of them clears the keyboard.
            let field = layout.at(0.11).max(layout.finger());
            let pitch = field + layout.at(0.02);
            return Self {
                top: 0.86,
                pitch,
                field,
                label_drop: 0.0,
                head: layout.at(0.03),
                tail: layout.at(0.03),
                label_column: layout.at(0.42),
                label_size: layout.content(),
            };
        }
        // The vertical band the stack has, and what it comes to at a
        // desktop's size -- see `Layout::within`.
        let stack = head + 0.16 + pitch * (count as f32 - 1.0) + 0.11 + tail + 0.44;
        let content = layout.within(top + 1.0 - 0.03, stack);
        Self {
            top,
            pitch: content.at(pitch),
            field: content.at(0.11).max(content.finger()),
            label_drop: content.at(0.16),
            head: content.at(head),
            tail: content.at(tail),
            label_column: 0.0,
            label_size: content.at(0.9),
        }
    }

    /// The bottom of a panel holding `count` of these.
    fn panel_bottom(&self, count: usize) -> f32 {
        self.top - self.head - self.label_drop - self.pitch * (count as f32 - 1.0)
            - if self.label_column > 0.0 { self.field } else { 0.0 }
            - self.tail
    }

    /// Where a row's label is written, and where its field goes.
    ///
    /// One function answering both, because the two have to agree and
    /// the arrangement they agree under is the thing that varies.
    fn row(&self, panel: Rect, pad: f32, index: usize) -> (f32, f32, Rect) {
        let row_top = self.top - self.head - self.pitch * index as f32;
        let field_y0 = row_top - self.label_drop - if self.label_column > 0.0 { self.field } else { 0.0 };
        if self.label_column > 0.0 {
            // Beside: the label sits in the left of the row and the box
            // takes the rest.
            let field = Rect::new(
                panel.x1 - pad - (panel.width() - pad * 2.0 - self.label_column),
                field_y0,
                panel.x1 - pad,
                field_y0 + self.field,
            );
            let cap = widgets::PIXEL * self.label_size * 7.0;
            (panel.x0 + pad, field.centre_y() + cap / 2.0, field)
        } else {
            let field = Rect::new(panel.x0 + pad, field_y0, panel.x1 - pad, field_y0 + self.field);
            (panel.x0 + pad, row_top, field)
        }
    }
}

/// A window onto a list of full-size rows that is longer than the panel
/// holding it: how tall a row is, how many of them fit, where each one
/// is drawn, and the lane the scrollbar runs down.
///
/// **One copy of this arithmetic, deliberately.** The settings screen
/// and the key-bindings screen are the same screen with different rows
/// in it, and for a long time they were not: settings scrolled, and the
/// bindings screen divided its panel by however many bindings there
/// were and drew every one of them at whatever height was left over.
/// At eighteen bindings that is a row a third of a finger tall, with
/// the key button inside it thinner still -- a list that looks right
/// and cannot be pressed. That screen's own comment admitted what it
/// was doing ("a player who wants both is asking for a scrolling
/// bindings list, which this is not"), which is the shape of a bug
/// waiting to be reported rather than a design.
///
/// The second copy of "how many rows fit" was what let the two screens
/// drift apart in the first place, so there is one. A new list on this
/// menu that asks here gets the settings screen's spacing, its finger
/// floor and its scrollbar without any of them being retyped.
struct ListRows {
    row_height: f32,
    gap: f32,
    pad: f32,
    /// The scrollbar's lane, taken out of the rows whether or not there
    /// is one to draw -- same reasoning as the world list: a list that
    /// reflows the moment it grows past its panel is a list whose rows
    /// move under the finger.
    gutter: f32,
    /// The air inside a row, between it and the buttons on it.
    inset: f32,
    visible: usize,
}

impl ListRows {
    /// A row at a desktop's size, and the air between two of them.
    ///
    /// The numbers the settings screen was designed around. They are
    /// not derived from the row count on purpose: **a row's height is
    /// not the variable here -- how many are on screen is.**
    const HEIGHT: f32 = 0.105;
    const GAP: f32 = 0.014;

    fn plan(layout: widgets::Layout, panel: Rect) -> Self {
        let gap = layout.at(Self::GAP);
        let inset = layout.at(0.012);
        // A row is as tall as the writing asks for, or as tall as the
        // finger that has to hit it -- and its buttons sit inside it by
        // `inset`, so the row has to be a finger *plus* that, or the
        // button comes out a finger short of one.
        let row_height = layout.at(Self::HEIGHT).max(layout.finger() + inset * 2.0);
        let pad = layout.at(0.03);
        // `n` rows have `n - 1` gaps between them, not `n`: counting a
        // gap after the last row threw away a whole row whenever the
        // arithmetic landed just short, which on the phone's shape it
        // did -- five rows drawn and a row's worth of empty panel under
        // them.
        // ...and a row that misses by a thousandth of a screen is a row
        // that fits. Without the slack the arithmetic lands on 4.99 and
        // 6.83 at two of the sizes this is drawn at, and the first of
        // those threw away a row that had room for itself.
        let visible =
            (((panel.height() - pad * 2.0 + gap + 0.002) / (row_height + gap)) as usize).max(1);
        Self { row_height, gap, pad, gutter: layout.at(0.022), inset, visible }
    }

    /// Where the `index`-th row *of those on screen* is drawn.
    ///
    /// **Everything that hit-tests a row comes through here**, which is
    /// the only way the two can be exact inverses of each other. A hot
    /// rect worked out anywhere else is a click that lands on the wrong
    /// setting the moment the list is scrolled.
    fn row(&self, panel: Rect, index: usize) -> Rect {
        let y =
            panel.y1 - self.pad - self.row_height - index as f32 * (self.row_height + self.gap);
        Rect::new(panel.x0 + self.pad, y, panel.x1 - self.pad - self.gutter, y + self.row_height)
    }

    /// The track the scrollbar is drawn in, down the gutter.
    fn track(&self, layout: widgets::Layout, panel: Rect) -> Rect {
        Rect::new(
            panel.x1 - self.pad - self.gutter + layout.at(0.006),
            panel.y0 + self.pad,
            panel.x1 - self.pad,
            panel.y1 - self.pad,
        )
    }
}

/// Which row of the new-world form the help line under it is about.
///
/// Every row on that form now has something to say -- what a seed is,
/// what a world type is, where you wake, how big the country is -- and
/// there is room under the panel for exactly one line of it. This is
/// which one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FormRow {
    Name,
    Seed,
    /// What the form opens explaining, because it is the row a player
    /// who has just pressed NEW is most likely to be undecided about.
    #[default]
    Preset,
    Zone,
    Scale,
}

/// Which field of the current form has focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Name,
    Address,
    Seed,
}

/// A setting the player can change in the game.
///
/// Deliberately a small list. Everything here either changes what the
/// game looks like or who you are on a server -- the things worth a
/// button. The couple of dozen tuning knobs behind them (fog ratios,
/// mesh budgets, worker threads) stay in the file, where the people who
/// touch them already live.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Setting {
    Username,
    RenderDistance,
    Fov,
    Sensitivity,
    /// How big the interface is drawn.
    ///
    /// High on the screen and right under the mouse row, because the
    /// player most likely to want it is the one who cannot comfortably
    /// read the screen they are looking at -- and asking them to scroll
    /// past ten graphics rows to find it is asking the wrong person to
    /// do the hard thing.
    UiScale,
    /// The two volumes. Together and directly under the mouse row,
    /// because "how do I turn the music down" is one of the two things
    /// anybody opens this screen to do and it should not be below the
    /// graphics. Two rather than three: the effects were cut from the
    /// game (see `audio`), so a slider for them would be a knob wired
    /// to nothing.
    MasterVolume,
    MusicVolume,
    Vsync,
    /// What share of the screen's pixels the game is drawn at.
    ///
    /// Directly under vsync and above every other picture row, because
    /// it is the one that moves the frame rate most and the one a
    /// player on a slow device should find first: everything below it
    /// trades a *part* of the frame, and this trades all of it. See
    /// `ClientSettings::resolution_scale`.
    Resolution,
    Fog,
    AmbientOcclusion,
    Anisotropy,
    /// How finely the sky is drawn.
    ///
    /// Next to the other two picture-quality rows rather than
    /// next to cloud cover, which is *weather*: cloudiness
    /// changes what the sky is, this changes how well it is
    /// drawn, and a settings screen that files them together
    /// makes a player hunt for the one that costs frames.
    SkyQuality,
    /// How far out a canopy stays see-through. A row of steps now, where
    /// it was a switch: see `ClientSettings::transparent_leaves_chunks`.
    TransparentLeaves,
    /// How much colour the light carries. See `engine::lighting`.
    ///
    /// Directly above the shadows, because the two are one question
    /// asked twice -- what the sunlight looks like -- and at the top step
    /// the first changes what the second draws (a softer edge).
    Lighting,
    /// Shadows from the sun.
    ///
    /// Under the leaves because it is the other switch that trades
    /// frames for how the world looks, and a player weighing one is
    /// weighing the other. Off by default -- see
    /// `ClientSettings::shadows`.
    Shadows,
    /// How far the sun's shadows reach. Under the switch it tunes, and
    /// saying nothing while that switch is off, for `LodQuality`'s reason.
    ShadowDistance,
    /// Which plants cast. Under the distance, as the last of the rows about
    /// the sun's shadows, and saying nothing while they are off, for
    /// `ShadowDistance`'s reason.
    PlantShadows,
    DetailDistance,
    /// How far out stones and sticks on the ground keep their thickness.
    /// Under the grass-and-stone distance, which says how far they are
    /// drawn at all: the two are one question about the same things.
    ReliefDistance,
    /// Where distant terrain starts being meshed out of bigger blocks.
    ///
    /// Next to the grass distance because they answer the same
    /// question -- how much of what is far away is worth drawing --
    /// and a player looking for frames should find the two together.
    LodDistance,
    LodQuality,
    Cloudiness,
    MenuBackground,
    MenuBackgroundScene,
    /// What language the interface is in.
    ///
    /// First on the screen, above the username, and that is the whole
    /// of its placement argument: a player who cannot read the
    /// interface is looking for exactly one row, and it should be the
    /// one their eye lands on.
    Language,
}

impl Setting {
    /// Every setting on the screen, top to bottom.
    pub const ALL: [Setting; 26] = [
        Setting::Language,
        Setting::Username,
        Setting::RenderDistance,
        Setting::Fov,
        Setting::Sensitivity,
        Setting::UiScale,
        Setting::MasterVolume,
        Setting::MusicVolume,
        Setting::Vsync,
        Setting::Resolution,
        Setting::Fog,
        Setting::AmbientOcclusion,
        Setting::Anisotropy,
        Setting::SkyQuality,
        Setting::TransparentLeaves,
        Setting::Lighting,
        Setting::Shadows,
        Setting::ShadowDistance,
        Setting::PlantShadows,
        Setting::DetailDistance,
        Setting::ReliefDistance,
        Setting::LodDistance,
        Setting::LodQuality,
        Setting::Cloudiness,
        Setting::MenuBackground,
        Setting::MenuBackgroundScene,
    ];

    /// What this row is called, as a message the language table knows.
    pub fn msg(&self) -> Msg {
        match self {
            Setting::Username => Msg::Name,
            Setting::RenderDistance => Msg::RenderDistance,
            Setting::Fov => Msg::FieldOfView,
            // The same row, named for whatever actually turns the
            // view on this machine. A phone has no mouse, and a row
            // called "mouse sensitivity" on one is a row a player
            // reads past looking for the setting they want.
            Setting::Sensitivity => crate::ui::lang::by_input(
                Msg::MouseSensitivity,
                Msg::LookSensitivity,
            ),
            Setting::UiScale => Msg::UiScale,
            Setting::MasterVolume => Msg::Volume,
            Setting::MusicVolume => Msg::Music,
            Setting::Vsync => Msg::Vsync,
            Setting::Resolution => Msg::Resolution,
            Setting::Fog => Msg::Fog,
            Setting::AmbientOcclusion => Msg::AmbientOcclusion,
            Setting::Anisotropy => Msg::Anisotropy,
            Setting::SkyQuality => Msg::SkyQuality,
            Setting::TransparentLeaves => Msg::TransparentLeaves,
            Setting::Lighting => Msg::LightingQuality,
            Setting::Shadows => Msg::Shadows,
            Setting::ShadowDistance => Msg::ShadowDistance,
            Setting::PlantShadows => Msg::PlantShadows,
            Setting::DetailDistance => Msg::DetailDistance,
            Setting::ReliefDistance => Msg::ReliefDistance,
            Setting::LodDistance => Msg::LodDistance,
            Setting::LodQuality => Msg::LodQuality,
            Setting::Cloudiness => Msg::Cloudiness,
            Setting::MenuBackground => Msg::MenuBackground,
            Setting::MenuBackgroundScene => Msg::MenuBackgroundScene,
            Setting::Language => Msg::LanguageRow,
        }
    }

    /// The label, in the language the player has chosen.
    pub fn label_in(&self, settings: &ClientSettings) -> &'static str {
        settings.language.text(self.msg())
    }

    /// What the row shows on the right.
    pub fn value(&self, settings: &ClientSettings) -> String {
        let language = settings.language;
        match self {
            // In the language itself: see `Language::name`.
            Setting::Language => settings.language.name().to_string(),
            Setting::Username => settings.username.clone(),
            Setting::RenderDistance => format!(
                "{} {}",
                settings.render_distance_chunks,
                crate::ui::lang::counted(language, settings.render_distance_chunks as u64, Msg::Chunks)
            ),
            Setting::Fov => {
                // No space before the sign: "72°", the way a number of
                // degrees is written in all four languages.
                format!("{:.0}{}", settings.fov_degrees, language.text(Msg::Degrees))
            }
            // Shown scaled up: the stored value is around 0.0025, and a
            // row reading "0.003" tells the player nothing about whether
            // a step made a difference.
            Setting::Sensitivity => format!("{:.0}", settings.mouse_sensitivity * 10_000.0),
            // A multiplier, shown as one: "1.0" and "2.0" say what they
            // do, where a percentage of an interface nobody measured
            // would not.
            Setting::UiScale => format!("{:.2}", settings.ui_scale),
            // As a percentage, and `OFF` at nothing rather than "0%":
            // a slider reading zero and one that is switched off are the
            // same state, and only one of them says so.
            Setting::MasterVolume => percent_or_off(settings.master_volume, language),
            Setting::MusicVolume => percent_or_off(settings.music_volume, language),
            Setting::Vsync => on_off(settings.vsync, language),
            // A percentage, and the word rather than a number when the
            // game is choosing: "AUTO" is the honest label for a row
            // whose value depends on a screen the settings file has
            // never seen. What it came out as is in the F3 line and in
            // the startup log, which is where a number belongs.
            Setting::Resolution => match settings.resolution_scale {
                Some(scale) => format!("{:.0}%", scale * 100.0),
                None => language.text(Msg::Auto).to_string(),
            },
            Setting::Fog => on_off(settings.fog_enabled, language),
            Setting::AmbientOcclusion => format!("{:.0}%", settings.ambient_occlusion * 100.0),
            Setting::Anisotropy => {
                if settings.anisotropy <= 1 {
                    language.text(Msg::Off).to_string()
                } else {
                    format!("{}x", settings.anisotropy)
                }
            }
            // Named levels rather than the divisor itself. The number
            // is a property of the renderer -- how many screen pixels
            // one sky pixel covers -- and "1/3" answers a question a
            // player did not ask.
            Setting::SkyQuality => language
                .text(match settings.sky_scale {
                    1 => Msg::QualityHigh,
                    2 => Msg::QualityMedium,
                    _ => Msg::QualityLow,
                })
                .to_string(),
            // Off at zero, for `LodDistance`'s reason -- "0 chunks" is not
            // a distance but the see-through canopy not happening -- and a
            // word at the far end, where a number would be a thousand.
            Setting::TransparentLeaves => match settings.transparent_leaves_chunks {
                chunks if chunks <= 0 => language.text(Msg::Off).to_string(),
                chunks if chunks >= crate::engine::lod::LEAVES_SEE_THROUGH_EVERYWHERE => {
                    language.text(Msg::Everywhere).to_string()
                }
                chunks => format!("{} {}", chunks, crate::ui::lang::counted(language, chunks as u64, Msg::Chunks)),
            },
            // The words every graphics menu uses, not the step's name in
            // the code. "Balanced" is a word about the implementation --
            // what it balances is cost against colour, and a player
            // choosing a step wants to know which way it leans.
            Setting::Lighting => language
                .text(match settings.lighting {
                    crate::engine::lighting::Quality::Simple => Msg::QualityLow,
                    crate::engine::lighting::Quality::Balanced => Msg::QualityMedium,
                    crate::engine::lighting::Quality::High => Msg::QualityHigh,
                })
                .to_string(),
            Setting::Shadows => language
                .text(match settings.shadows {
                    crate::engine::shadow::Mode::Off => Msg::Off,
                    crate::engine::shadow::Mode::Hard => Msg::ShadowsHard,
                    crate::engine::shadow::Mode::Soft => Msg::ShadowsSoft,
                })
                .to_string(),
            Setting::ShadowDistance => {
                if settings.shadows.is_on() {
                    format!(
                        "{:.0} {}",
                        settings.shadow_distance,
                        crate::ui::lang::counted(language, settings.shadow_distance as u64, Msg::Blocks)
                    )
                } else {
                    language.text(Msg::Off).to_string()
                }
            }
            Setting::PlantShadows => {
                use crate::engine::shadow::PlantShadows;
                language
                    .text(match settings.plant_shadows {
                        _ if !settings.shadows.is_on() => Msg::Off,
                        PlantShadows::Off => Msg::Off,
                        PlantShadows::Trees => Msg::PlantShadowsTrees,
                        PlantShadows::All => Msg::PlantShadowsAll,
                    })
                    .to_string()
            }
            Setting::DetailDistance => format!("{:.0}%", settings.detail_distance * 100.0),
            Setting::ReliefDistance => {
                if settings.relief_chunks <= 0 {
                    language.text(Msg::Off).to_string()
                } else {
                    format!(
                        "{} {}",
                        settings.relief_chunks,
                        crate::ui::lang::counted(language, settings.relief_chunks as u64, Msg::Chunks)
                    )
                }
            }
            // Off reads as off. "0 chunks" would be a distance, and
            // this row's zero is not a distance -- it is the feature
            // not happening.
            Setting::LodDistance => {
                if settings.lod_distance_chunks <= 0 {
                    language.text(Msg::Off).to_string()
                } else {
                    format!(
                        "{} {}",
                        settings.lod_distance_chunks,
                        crate::ui::lang::counted(language, settings.lod_distance_chunks as u64, Msg::Chunks)
                    )
                }
            }
            // **Says nothing while the simplification is off**, because
            // there is no coarse chunk for it to have an opinion about:
            // a row that offers a choice which changes nothing is a row
            // that teaches the player the setting is broken.
            Setting::LodQuality => {
                if settings.lod_distance_chunks <= 0 {
                    language.text(Msg::Off).to_string()
                } else {
                    language
                        .text(match settings.lod_quality {
                            crate::engine::lod::Quality::Fine => Msg::LodFine,
                            crate::engine::lod::Quality::Normal => Msg::LodNormal,
                            crate::engine::lod::Quality::Coarse => Msg::LodCoarse,
                        })
                        .to_string()
                }
            }
            Setting::Cloudiness => format!("{:.0}%", settings.cloudiness * 100.0),
            Setting::MenuBackground => on_off(settings.menu_background, language),
            // Translated, unlike the block name this row used to show.
            // A block's name is an identifier -- it is in `blocks.toml`
            // and in save files, and the interface shows it as it is
            // written there. A landscape is not: "SHORE" is a word
            // about what the player will see, and there is nothing to
            // keep it in step with.
            Setting::MenuBackgroundScene => language
                .text(match settings.menu_background_place() {
                    None => Msg::SceneRandom,
                    Some(Place::Shore) => Msg::SceneShore,
                    Some(Place::Forest) => Msg::SceneForest,
                    Some(Place::Plains) => Msg::ScenePlains,
                    Some(Place::Cave) => Msg::SceneCave,
                })
                .to_string(),
        }
    }

    /// True for settings that are a switch rather than a range, so the
    /// screen can draw one wide button instead of a `-`/`+` pair.
    pub fn is_toggle(&self) -> bool {
        matches!(
            self,
            Setting::Vsync
                | Setting::Fog
                | Setting::MenuBackground
                | Setting::Shadows
        )
    }

    /// True for the setting that is greyed out until another one is on.
    pub fn depends_on_menu_background(&self) -> bool {
        matches!(self, Setting::MenuBackgroundScene)
    }

    /// True for the one setting that is typed rather than stepped.
    pub fn is_text(&self) -> bool {
        matches!(self, Setting::Username)
    }

    /// Applies one step. Clamping is left to `ClientSettings::sanitize`,
    /// which is the same code a hand-edited file goes through.
    pub fn step(&self, settings: &mut ClientSettings, delta: i32) {
        if let Setting::Language = self {
            settings.language = settings.language.step(delta);
            return;
        }
        let d = delta as f32;
        match self {
            // Handled above, before the numeric ones.
            Setting::Language => {}
            Setting::Username => {}
            Setting::RenderDistance => settings.render_distance_chunks += delta,
            Setting::Fov => settings.fov_degrees += 5.0 * d,
            Setting::Sensitivity => settings.mouse_sensitivity += 0.0002 * d,
            // A twentieth at a time. Coarse enough that crossing the
            // useful range is a few presses, fine enough that a player
            // can stop where it looks right rather than between two
            // sizes that both do not.
            Setting::UiScale => settings.ui_scale += 0.05 * d,
            Setting::MasterVolume => settings.master_volume += 0.05 * d,
            Setting::MusicVolume => settings.music_volume += 0.05 * d,
            Setting::Vsync => settings.vsync = !settings.vsync,
            // AUTO sits below the lowest share, so that stepping *left*
            // off the bottom of the row arrives at the choice the game
            // makes for itself rather than stopping at 60%. A player
            // who has been trying steps by hand and wants to stop
            // deciding can reach it the same way they reached
            // everything else.
            Setting::Resolution => {
                let stops = crate::settings::RESOLUTION_SCALES;
                // The step the row is standing on: the nearest one,
                // because a hand-edited 0.73 must still step to a
                // neighbour rather than jump to an end.
                let current = settings.resolution_scale.map(|scale| {
                    stops
                        .iter()
                        .enumerate()
                        .min_by(|(_, a), (_, b)| {
                            (*a - scale).abs().total_cmp(&(*b - scale).abs())
                        })
                        .map_or(stops.len() - 1, |(index, _)| index) as i32
                });
                // AUTO is index -1 on this row and nothing in the file.
                let next = current.unwrap_or(-1) + delta;
                settings.resolution_scale = if next < 0 {
                    None
                } else {
                    Some(stops[(next as usize).min(stops.len() - 1)])
                };
            }
            Setting::Fog => settings.fog_enabled = !settings.fog_enabled,
            Setting::AmbientOcclusion => settings.ambient_occlusion += 0.05 * d,
            Setting::Anisotropy => {
                // Steps through the powers of two rather than adding,
                // because those are the only values wgpu accepts.
                let steps: [u16; 5] = [1, 2, 4, 8, 16];
                let current = steps
                    .iter()
                    .position(|v| *v == settings.anisotropy)
                    .unwrap_or(0) as i32;
                let next = (current + delta).clamp(0, steps.len() as i32 - 1);
                settings.anisotropy = steps[next as usize];
            }
            Setting::SkyQuality => {
                // Ordered worst to best, so that pressing *right* --
                // which every other row on this screen answers by
                // making the number bigger -- makes the picture better.
                // The stored value runs the other way: it is a divisor,
                // and 1 is the finest sky there is.
                let steps: [u32; 3] = [3, 2, 1];
                let current = steps
                    .iter()
                    .position(|v| *v == settings.sky_scale)
                    .unwrap_or(0) as i32;
                let next = (current + delta).clamp(0, steps.len() as i32 - 1);
                settings.sky_scale = steps[next as usize];
            }
            // Walked along the stops, not added to, and never wrapped: a
            // player stepping past "everywhere" should stay there rather
            // than land on solid everywhere.
            Setting::TransparentLeaves => {
                settings.transparent_leaves_chunks =
                    walk_stops(&crate::settings::TRANSPARENT_LEAVES_STOPS, settings.transparent_leaves_chunks, delta)
            }
            // Walked, not wrapped, for the reason `LodQuality` gives:
            // a player who overshot the top step should step back down,
            // not round through the cheapest.
            Setting::Lighting => {
                let all = crate::engine::lighting::Quality::ALL;
                let at = all.iter().position(|q| *q == settings.lighting).unwrap_or(0) as i32;
                settings.lighting = all[(at + delta).clamp(0, all.len() as i32 - 1) as usize];
            }
            // Still the one wide switch it was, pressed round three steps
            // rather than two: a row of "-" and "+" would have moved every
            // row under it, for a choice a player makes once.
            Setting::Shadows => settings.shadows = settings.shadows.step(delta),
            // Walked along the stops, not added to: see `SHADOW_DISTANCES`.
            Setting::ShadowDistance => {
                let stops = crate::settings::SHADOW_DISTANCES;
                let at = stops
                    .iter()
                    .position(|d| (*d - settings.shadow_distance).abs() < 0.5)
                    .unwrap_or(3) as i32;
                settings.shadow_distance = stops[(at + delta).clamp(0, stops.len() as i32 - 1) as usize];
            }
            // Walked, not wrapped: see `PlantShadows::step`.
            Setting::PlantShadows => settings.plant_shadows = settings.plant_shadows.step(delta),
            Setting::DetailDistance => settings.detail_distance += 0.1 * d,
            Setting::ReliefDistance => {
                settings.relief_chunks = walk_stops(&crate::settings::RELIEF_STOPS, settings.relief_chunks, delta)
            }
            // Two chunks a step, and stepping below the clamp's floor
            // lands on "off" rather than sticking at four: the row has
            // to be able to reach the state that turns the feature off,
            // and there is nothing between "four chunks" and "never".
            Setting::LodDistance => {
                settings.lod_distance_chunks = match settings.lod_distance_chunks {
                    0 if delta > 0 => 4,
                    0 => 0,
                    current => (current + delta * 2).max(0),
                }
            }
            // Three stops, walked rather than wrapped: a row that goes
            // round leaves a player who overshot with no way back
            // except round again, and three is short enough that the
            // ends are where they expect them.
            Setting::LodQuality => {
                let all = crate::engine::lod::Quality::ALL;
                let at = all
                    .iter()
                    .position(|q| *q == settings.lod_quality)
                    .unwrap_or(1) as i32;
                let next = (at + delta).clamp(0, all.len() as i32 - 1);
                settings.lod_quality = all[next as usize];
            }
            Setting::Cloudiness => settings.cloudiness += 0.1 * d,
            Setting::MenuBackground => settings.menu_background = !settings.menu_background,
            // Five stops, not four: "any of them" is a choice about the
            // backdrop like the other four, and the only one that makes
            // the next launch different from this one. It sits at the
            // head of the ring because it is the default, and a row
            // stepped once and stepped back should land where it
            // started.
            Setting::MenuBackgroundScene => {
                let stops = [
                    None,
                    Some(Place::Shore),
                    Some(Place::Forest),
                    Some(Place::Plains),
                    Some(Place::Cave),
                ];
                let current = stops
                    .iter()
                    .position(|stop| *stop == settings.menu_background_place())
                    .unwrap_or(0) as i32;
                let count = stops.len() as i32;
                let next = ((current + delta) % count + count) % count;
                settings.menu_background_scene = match stops[next as usize] {
                    Some(place) => place.name().to_string(),
                    None => crate::settings::MENU_SCENE_ANY.to_string(),
                };
            }
        }
        settings.sanitize();
    }
}

/// A volume, as a percentage -- except at the bottom of its range,
/// where the honest word is `OFF`.
fn percent_or_off(value: f32, language: Language) -> String {
    if value <= 0.0001 {
        language.text(Msg::Off).to_string()
    } else {
        format!("{:.0}%", value * 100.0)
    }
}

fn on_off(value: bool, language: Language) -> String {
    language
        .text(if value { Msg::On } else { Msg::Off })
        .to_string()
}

/// One step along a sorted row of stops from the stop nearest `current`,
/// held at both ends. Nearest rather than equal, so a value a hand-edited
/// file holds between two stops still steps somewhere sensible before
/// `sanitize` has seen it -- the same question `settings::nearest_stop`
/// asks at load.
fn walk_stops(stops: &[i32], current: i32, delta: i32) -> i32 {
    let at = stops
        .iter()
        .enumerate()
        .min_by_key(|(_, stop)| (i64::from(**stop) - i64::from(current)).abs())
        .map_or(0, |(i, _)| i as i32);
    stops[(at + delta).clamp(0, stops.len() as i32 - 1) as usize]
}


/// Something the player asked for. `main.rs` carries these out; nothing
/// in this module performs them.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    OpenWorlds,
    OpenServers,
    OpenSettings,
    OpenControls,
    /// Open the screen where the thumb controls are dragged about.
    OpenTouchControls,
    /// Put the thumb controls back where the game shipped them.
    ResetTouchControls,
    OpenCredits,
    /// Open the extensions screen, and ask the server for the list.
    /// The asking is `main.rs`'s -- the menu owns no socket.
    OpenExtensions,
    /// Highlight one row of that list, which is what decides whose
    /// description is in the other pane.
    SelectExtension(usize),
    Quit,
    Back,
    Select(usize),
    Connect(usize),
    Add,
    Edit(usize),
    Delete(usize),
    Focus(Field),
    Save,
    Cancel,
    Retry,
    Resume,
    LeaveWorld,

    // ---- worlds ----
    SelectWorld(usize),
    PlayWorld(usize),
    NewWorld,
    /// Opens the confirmation gate; `ConfirmedDeleteWorld` is what
    /// actually removes anything.
    AskDeleteWorld(usize),
    ConfirmedDeleteWorld(usize),
    /// Open the rename form on this world, with its current name in the
    /// box.
    RenameWorld(usize),
    /// Write the name that is in the box. Carried out by `main.rs`,
    /// which owns the saves folder.
    CommitRename(usize),
    /// Duplicate a world, keeping the original exactly as it is.
    ///
    /// **No confirmation gate, unlike DELETE**, and the asymmetry is the
    /// point: a gate exists in front of a thing that cannot be undone,
    /// and the worst a stray COPY can do is put one more row in a list
    /// that has a DELETE beside it.
    CopyWorld(usize),
    CreateWorld,
    /// Step the world type on the new-world form: `+1` forward, `-1`
    /// back. A step rather than a `Set`, so the row behaves like every
    /// other stepped setting and the keyboard and the mouse drive it the
    /// same way.
    StepPreset(i32),
    /// Step the zone on the new-world form, the same way. See
    /// `worldgen::Zone`.
    StepZone(i32),
    /// Step how big the country is drawn. See `worldgen::Scale`.
    StepScale(i32),
    /// Roll a seed into the new-world form's seed box. See `random_seed`.
    RollSeed,

    // ---- settings ----
    Tweak(Setting, i32),
    /// One change to where the thumb controls sit. Carried out of the
    /// menu because the menu only reads the settings; `main` is what
    /// owns them and what writes the file.
    /// Start listening for the key to put on this action.
    RebindKey(crate::ui::keybinds::Action),
    /// Put every binding back the way it shipped.
    ResetKeys,
    EditUsername,
    CommitUsername,
}

pub struct Menu {
    pub servers: ServerList,
    pub selected: usize,
    /// Row highlighted on the worlds screen. Separate from `selected`
    /// (the server list) so switching between the two doesn't move both.
    pub world_selected: usize,
    /// Worlds shown by the last `build`; see `set_world_count`.
    world_count: usize,
    /// First world row on screen, and how many rows fit.
    ///
    /// **A position of its own rather than something derived from the
    /// selection.** It used to be `selected - (visible - 1)`, which puts
    /// the highlighted row on the bottom line of the window and keeps it
    /// there: the list scrolled on every single press, the rows slid
    /// under a stationary highlight, and there was no way to look at the
    /// bottom of a long list without selecting something down there. A
    /// list that scrolls is a list with a place in it, and the selection
    /// merely has to stay inside the window -- see `show_world`.
    world_scroll: usize,
    /// Rows the last `build` had room for, so the wheel and the arrow
    /// keys can bound the scroll without being handed the panel.
    world_visible: usize,
    /// The same pair for the server list.
    ///
    /// **It used to have neither.** The first visible row was derived
    /// from the selection, which reads as scrolling only while the
    /// arrow keys are what moves it: the wheel could not shift the view
    /// at all, and on a phone -- no arrow keys, and a thumb dragged down
    /// the list -- the list simply refused to move.
    server_scroll: usize,
    server_visible: usize,
    /// First settings row on screen, and how many fit -- the settings
    /// screen scrolls exactly the way the world list does. It used to
    /// squeeze however many rows there were into one panel instead,
    /// which was correct for any count and readable for none: at
    /// seventeen settings the rows had shrunk to a squint.
    settings_scroll: usize,
    settings_visible: usize,
    /// The same pair again for the key-bindings screen, which is the
    /// settings screen with bindings in it.
    ///
    /// **It had neither, and drew all eighteen rows at once.** The
    /// height they had to share shrank with every binding added, so the
    /// screen got quietly worse each time the game got a new key -- and
    /// the row a player wanted was never off the panel, only too thin
    /// to aim at. See `ListRows`.
    controls_scroll: usize,
    controls_visible: usize,
    /// What is extending the server, and the same scroll/selection pair
    /// every other list on this menu carries.
    ///
    /// Kept on the menu rather than in `MenuContext` because it arrives
    /// once, as a message, and nothing else in the client reads it --
    /// the server list is here for the same reason. What `main.rs` owns
    /// is the socket it came down.
    extensions: Extensions,
    extension_selected: usize,
    extension_scroll: usize,
    extension_visible: usize,
    pub screen: Screen,
    /// The three things a form is typed into.
    ///
    /// **`TextField` rather than `String`, since the day the fields
    /// grew a caret.** See `ui::field`: a `String` with `push` and
    /// `pop` on it is a field whose only edit is "add to the end", and
    /// every complaint about these fields was a consequence of that one
    /// fact.
    pub name_input: TextField,
    pub address_input: TextField,
    pub seed_input: TextField,
    /// What the new-world form will build. Kept on the menu rather than
    /// passed around, exactly like the name and the seed typed beside
    /// it, and reset with them when the form is opened.
    pub world_preset: Preset,
    /// Where on the planet the new-world form will lay it. Kept and reset
    /// exactly like the preset beside it. See `worldgen::Zone`.
    pub world_zone: Zone,
    /// How big a country the new-world form will draw. See
    /// `worldgen::Scale`.
    ///
    /// **The one choice on this form that is not better or worse.** The
    /// landforms are the planet at the Earth's size; the regional world
    /// is an archipelago where the next island is a walk rather than a
    /// voyage. Which of those a player wants is a question about the game
    /// they mean to play, which is what this form is for -- and it was a
    /// constant in `Worlds::create_in` that nobody could see.
    pub world_scale: Scale,
    /// Which row the line under the form is explaining: whichever was
    /// touched last.
    ///
    /// **A row rather than a flag**, and it grew from one because two
    /// booleans for three rows is a state that can say "both". One line
    /// explains the thing just changed, because a form with a help line
    /// under every row would push the buttons off a phone held sideways,
    /// and a line about the row the player is not looking at is a line
    /// nobody reads.
    explaining: FormRow,
    pub focus: Field,
    /// True while the name row of the settings screen is being typed
    /// into. The row turns into a text field and swallows keys.
    pub editing_username: bool,
    /// A name that was typed and not yet handed to the settings.
    ///
    /// **Because pressing Enter was load-bearing, and a phone has no
    /// reason to.** The name only reached `ClientSettings` through
    /// `Action::CommitUsername`, and the only two keys that sent it
    /// were Enter and Tab. On a desktop that is merely unusual; on a
    /// phone the soft keyboard is dismissed by tapping away from the
    /// field or by the back gesture, and neither is a key, so the
    /// player typed a name, left the screen the ordinary way, and the
    /// name was silently gone. Reported as "the username is not saved
    /// on the phone".
    ///
    /// So leaving the field now *keeps* what is in it, and the two
    /// ways of saying "never mind" -- Escape and Cancel -- are the only
    /// ways to throw it away. Kept here rather than written straight
    /// into the settings because the menu does not own them: it is
    /// drained at the point the settings are saved. See
    /// `take_typed_username`.
    username_pending: Option<String>,
    /// The window, in pixels.
    ///
    /// **The one thing the menu needs that is not a fraction.** Every
    /// other screen is laid out in interface space and never asks how
    /// many pixels that is; the arrangement screen shows the thumb
    /// controls at the size a finger actually is, which
    /// `platform::touch` works out in pixels from the screen's shorter
    /// side. Kept as a field set by the frame loop rather than carried
    /// in `MenuContext`, because most of the places that build a context
    /// have no window at all -- the snapshot tests draw the whole
    /// interface without one.
    window_px: (u32, u32),

    /// The arrangement being edited, while that screen is open.
    ///
    /// A working copy rather than the settings themselves, so the screen
    /// can be left without saving and the frame loop has one place to
    /// read the answer from.
    arrangement: crate::settings::TouchLayout,

    /// The interface size the drag was begun at.
    ///
    /// Carried because the pointer arrives without it and a control has
    /// to keep following the finger between presses. `None` means
    /// nothing is being carried.
    drag_scale: Option<f32>,

    /// Which control the finger has hold of, and how far the grab was
    /// from its middle, in pixels.
    ///
    /// The offset matters: without it a control jumps so that its middle
    /// is under the finger the instant it is touched, which reads as the
    /// button being snatched rather than picked up.
    dragging: Option<(ControlUnderHand, (f32, f32))>,

    /// The action waiting for a key, if the player is rebinding one.
    /// While it is set the controls screen swallows the next keypress.
    rebinding: Option<crate::ui::keybinds::Action>,
    /// A one-line note under the list ("server added", "address is
    /// required"), cleared when the player navigates away.
    pub notice: Option<(Notice, bool)>,
    /// Cursor position in UI coordinates, or `None` if it has left the
    /// window.
    pub cursor: Option<(f32, f32)>,
    /// Which button the arrow keys have landed on, on the screens that
    /// are a plain list of buttons.
    ///
    /// Mutually exclusive with `cursor` by construction: moving the mouse
    /// clears it and pressing an arrow key clears the cursor. Two
    /// highlights on screen at once leaves the player unsure which one
    /// Enter would press.
    button_focus: Option<usize>,
    /// Where to go back to from the connecting and failure screens.
    came_from: Box<Screen>,
    /// Rebuilt by `build`; the hit-test targets for the current screen.
    hot: Vec<(Rect, Action)>,
    /// The boxes the text fields were drawn in, and how big their
    /// writing was.
    ///
    /// **Beside `hot` rather than folded into it, and remembered for
    /// the same reason.** A click on a field has to land on a
    /// *character*, and working out which one takes the rectangle, the
    /// value and the size it was lettered at -- see
    /// `widgets::caret_at_x`. The action alone cannot carry that, and
    /// re-deriving the rectangle when the click arrives would be a
    /// second copy of the layout: the whole point of `hot` is that the
    /// pass which draws is the pass which decides where things are.
    field_boxes: Vec<(Rect, Field)>,
    /// The content scale the last build lettered with, which is the
    /// other half of what `caret_at_x` needs.
    content_drawn: f32,
    /// Whether the platform's own editor owns the focused field.
    ///
    /// **A click must not move the caret while it does.** On Android
    /// the input method holds the text *and* the caret, and every edit
    /// arrives as a whole new line through the mirror (see
    /// `set_focused_text`), which leaves our caret at the end. A caret
    /// this side dropped somewhere by a tap would be drawn in the
    /// middle of the value and would then be wrong about where the next
    /// character is going -- a lie that looks like a feature. Set once
    /// a frame by the loop that runs the mirror.
    ime_owns_text: bool,
    /// Where the text caret is in its blink, in seconds, wrapped to one.
    ///
    /// **This carried the wrong comment for a long time.** It described
    /// "the control the player has picked up in the layout editor" --
    /// left behind when that editor was taken out, and orphaned onto the
    /// next field down. The editor is back (see `Screen::TouchControls`)
    /// and the state it needs is `dragging`, which says so itself.
    caret_phase: f32,
}

/// Longest name and address the form will accept. Both are generous;
/// the point is only that neither can grow without bound.
const MAX_NAME: usize = 32;
const MAX_ADDRESS: usize = 64;
/// A `u32` is ten digits at most; anything longer cannot be a seed.
const MAX_SEED_DIGITS: usize = 10;

/// A seed nobody chose: what a blank seed box makes a world from, and what
/// the roll button writes into it.
///
/// **The standard library's own random keys, stirred with the clock.** Every
/// `RandomState` is seeded from the operating system's randomness once per
/// process and stepped on for every one made after, so two rolls in one
/// second are two numbers and two machines rolling at once are two numbers.
///
/// Rejected: the clock alone, which is two identical worlds for two players
/// who press CREATE in the same second, and on some phones a clock that
/// steps in whole milliseconds; and a random-number crate, which is a
/// dependency for one number a world.
pub fn random_seed() -> u32 {
    use std::hash::{BuildHasher, Hasher};
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |since| since.as_nanos());
    hasher.write_u128(now);
    let mixed = hasher.finish();
    (mixed ^ (mixed >> 32)) as u32
}

/// What to say, in the language the player has chosen.
///
/// A free function taking the context rather than a method, so a screen
/// that already borrows `self` mutably to build itself can still ask.
fn say(ctx: &MenuContext, msg: Msg) -> &'static str {
    ctx.settings.language.text(msg)
}

/// What a screen with nothing on it should say, and why it is empty.
///
/// Three different sentences, and telling them apart is the whole
/// value of the screen: the answer has not arrived yet, the server
/// runs nothing, or nothing *could* be run here. A fourth was missing
/// -- see `Msg::ExtensionsNoLoaderPhone`.
///
/// A free function taking the fact rather than reading it, so that both
/// answers are reachable from a test on one machine: `loads_mods` is a
/// compile-time constant in the game and a parameter here.
fn empty_extensions_line(extensions: &Extensions, loads_mods: bool) -> Msg {
    match extensions {
        Extensions::Unasked | Extensions::Waiting => Msg::ExtensionsAsking,
        Extensions::Known(list) if list.native_api.is_none() && !list.scripts_supported => {
            if loads_mods {
                Msg::ExtensionsNoLoader
            } else {
                Msg::ExtensionsNoLoaderPhone
            }
        }
        Extensions::Known(_) => Msg::ExtensionsNone,
    }
}

/// Whether this build could load a native mod at all.
///
/// **A fact about the package, not about the server it is talking to.**
/// `primitive_client/Cargo.toml` asks for the server's `mods` feature on
/// every target *except* Android, because a mod is a library a player
/// drops into a folder and an APK has no folder an ordinary person can
/// reach -- so a phone reports no native loader for a reason that has
/// nothing to do with the world being local. The screen has to say
/// which of the two it is looking at: "this world runs inside the game"
/// reads as "join a server and you will get mods", and on a phone that
/// is only half true, because the half that runs the mods is not in the
/// package.
///
/// Compiled out rather than detected, and deliberately not
/// `lang::touch_primary`: `PRIMITIVE_TOUCH_UI=1` lays a desktop out for
/// a thumb, and that desktop still loads mods.
///
/// **`PRIMITIVE_NO_MODS_UI=1` says the phone's answer on a desktop**,
/// and it is here for the same reason `PRIMITIVE_TOUCH_UI` is: without
/// it this sentence is a string only an APK can print, on a screen only
/// a paused world can open, on a device nobody can drive -- which is to
/// say it would ship having never been looked at. With it,
/// `ui::snapshot` draws the picture. It changes nothing else: whether a
/// mod is *loaded* is decided by what the server answers, not by this.
fn this_build_loads_mods() -> bool {
    static ANSWER: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ANSWER.get_or_init(|| {
        !cfg!(target_os = "android") && std::env::var_os("PRIMITIVE_NO_MODS_UI").is_none()
    })
}

/// What to call an extension point, on the row and in the pane.
fn kind_msg(kind: primitive_shared::protocol::ExtensionKind) -> Msg {
    match kind {
        primitive_shared::protocol::ExtensionKind::Script => Msg::ExtensionScript,
        primitive_shared::protocol::ExtensionKind::Native => Msg::ExtensionNative,
    }
}

/// How many characters of `scale` fit across `width`, for
/// `widgets::wrap` -- which counts in characters because the font's
/// advance is one constant. See `engine::font`, and the note there on
/// why proportional widths were rejected.
fn chars_that_fit(width: f32, scale: f32) -> usize {
    let per_char = widgets::measure("x", scale).max(1e-4);
    ((width / per_char) as usize).max(8)
}

/// What to call a world type on the new-world form.
///
/// A function rather than a method on `Preset`, because `Preset` lives
/// in the shared crate and the shared crate does not speak four
/// languages -- `Preset::name` is the identifier a file is written with,
/// and this is the label a person reads.
fn preset_label(preset: Preset) -> Msg {
    match preset {
        Preset::Normal => Msg::PresetNormal,
        Preset::Test => Msg::PresetTest,
    }
}

/// ...and the line under it saying what that means.
fn preset_help(preset: Preset) -> Msg {
    match preset {
        Preset::Normal => Msg::PresetNormalHelp,
        Preset::Test => Msg::PresetTestHelp,
    }
}

/// What to call a zone on the new-world form. A function here for
/// `preset_label`'s reason: the shared crate does not speak four languages.
fn zone_label(zone: Zone) -> Msg {
    match zone {
        Zone::Tropics => Msg::ZoneTropics,
        Zone::DryBelt => Msg::ZoneDryBelt,
        Zone::Temperate => Msg::ZoneTemperate,
        Zone::North => Msg::ZoneNorth,
    }
}

/// ...and the line under the form saying what living there means.
fn zone_help(zone: Zone) -> Msg {
    match zone {
        Zone::Tropics => Msg::ZoneTropicsHelp,
        Zone::DryBelt => Msg::ZoneDryBeltHelp,
        Zone::Temperate => Msg::ZoneTemperateHelp,
        Zone::North => Msg::ZoneNorthHelp,
    }
}

fn scale_label(scale: Scale) -> Msg {
    match scale {
        Scale::Landforms => Msg::ScaleLandforms,
        Scale::Earth => Msg::ScaleEarth,
        Scale::Regional => Msg::ScaleRegional,
    }
}

fn scale_help(scale: Scale) -> Msg {
    match scale {
        Scale::Landforms => Msg::ScaleLandformsHelp,
        Scale::Earth => Msg::ScaleEarthHelp,
        Scale::Regional => Msg::ScaleRegionalHelp,
    }
}

/// The next scale along, wrapping.
///
/// **Newest first, which is the other way round from `Scale::ALL`.** That
/// list is oldest-first because it is a history; this is a control, and a
/// control that opens on the newest generator should step to the
/// second-newest when it is pressed once, not back through two versions
/// of the world to the archipelago.
fn step_scale(scale: Scale, delta: i32) -> Scale {
    const ORDER: [Scale; 3] = [Scale::Landforms, Scale::Earth, Scale::Regional];
    let at = ORDER.iter().position(|&s| s == scale).unwrap_or(0) as i32;
    let count = ORDER.len() as i32;
    ORDER[(at + delta).rem_euclid(count) as usize]
}

/// What a world's row says about it under its name.
///
/// **Four facts, and the order is the order they are asked in.** When
/// was I last here; how long did I live there and what time of year is
/// it now; how much disk is this; and, last because it is the one a
/// player rarely wants, which seed it was made from. Last is also where
/// the truncation starts, and that is the same decision: a row too
/// narrow for all of it loses the seed rather than the date.
///
/// A world nobody has opened says so and stops -- a row reading "day 1,
/// spring, 4 kB" about a folder that has never been entered is four
/// facts and no information.
fn world_detail(world: &worlds::World, now: u64, language: Language) -> String {
    let say = |msg: Msg| language.text(msg);
    let mut parts: Vec<String> = Vec::new();
    match world.played_on() {
        Some(date) => {
            parts.push(world.played_description(now, language));
            parts.push(date);
        }
        None => parts.push(say(Msg::NeverPlayed).to_string()),
    }
    if let (Some(day), Some(season)) = (world.day(), world.season()) {
        parts.push(format!("{} {day}, {}", say(Msg::Day), say(season_name(season))));
    }
    if world.bytes > 0 {
        parts.push(worlds::size_description(world.bytes));
    }
    if let Some(seed) = world.seed {
        parts.push(format!("{} {seed}", say(Msg::SeedShort)));
    }
    // Three spaces between facts rather than a bullet: the font has no
    // middle dot, and a comma would read as part of the fact before it.
    parts.join("   ")
}

fn season_name(season: primitive_shared::season::Season) -> Msg {
    use primitive_shared::season::Season;
    match season {
        Season::Spring => Msg::SeasonSpring,
        Season::Summer => Msg::SeasonSummer,
        Season::Autumn => Msg::SeasonAutumn,
        Season::Winter => Msg::SeasonWinter,
    }
}

/// A one-line note under a list.
///
/// Two kinds because the notes come from two places: fixed phrases the
/// language table knows ("an address is required"), and lines built
/// around a name the player typed ("saved My Server"), which no table
/// can hold. A `Translated` note is resolved when it is *drawn*, so it
/// follows a language change instead of freezing in the old one.
#[derive(Debug, Clone, PartialEq, Hash)]
pub enum Notice {
    Text(String),
    Translated(Msg),
}

impl From<String> for Notice {
    fn from(text: String) -> Self {
        Notice::Text(text)
    }
}

impl Menu {
    pub fn new(servers: ServerList) -> Self {
        Self {
            servers,
            selected: 0,
            world_selected: 0,
            world_count: 0,
            world_scroll: 0,
            server_scroll: 0,
            server_visible: 0,
            world_visible: 1,
            settings_scroll: 0,
            settings_visible: 1,
            controls_scroll: 0,
            controls_visible: 1,
            extensions: Extensions::Unasked,
            extension_selected: 0,
            extension_scroll: 0,
            extension_visible: 1,
            screen: Screen::Main,
            name_input: TextField::new(),
            address_input: TextField::new(),
            seed_input: TextField::new(),
            world_preset: Preset::Normal,
            world_zone: Zone::Temperate,
            world_scale: Scale::default(),
            explaining: FormRow::default(),
            focus: Field::Name,
            editing_username: false,
            username_pending: None,
            window_px: (1280, 720),
            arrangement: crate::settings::TouchLayout::default(),
            dragging: None,
            drag_scale: None,
            rebinding: None,
            notice: None,
            cursor: None,
            button_focus: None,
            came_from: Box::new(Screen::Main),
            hot: Vec::new(),
            field_boxes: Vec::new(),
            content_drawn: 1.0,
            ime_owns_text: false,
            caret_phase: 0.0,
        }
    }

    /// Moves the mouse pointer, taking the highlight away from the
    /// keyboard.
    /// Tells the menu how big the window is, in pixels.
    ///
    /// Only the arrangement screen reads it; see the field.
    /// Told once a frame whether the platform's editor holds the
    /// focused field. See the field of the same name.
    pub fn set_ime_owns_text(&mut self, owns: bool) {
        self.ime_owns_text = owns;
    }

    pub fn set_screen_size(&mut self, width: u32, height: u32) {
        self.window_px = (width.max(1), height.max(1));
    }

    /// The arrangement as it stands, for the caller to save and apply.
    pub fn arrangement(&self) -> crate::settings::TouchLayout {
        self.arrangement
    }

    /// Starts the arrangement screen from what the player already has.
    ///
    /// A copy, so that leaving without saving leaves the settings alone
    /// -- and so the frame loop has exactly one place to read the
    /// answer back from.
    pub fn begin_arranging(&mut self, from: crate::settings::TouchLayout) {
        self.arrangement = from;
        self.dragging = None;
    }

    /// Whether the arrangement screen is the one on the glass.
    pub fn is_arranging(&self) -> bool {
        matches!(self.screen, Screen::TouchControls)
    }

    pub fn set_cursor(&mut self, position: Option<(f32, f32)>) {
        self.cursor = position;
        if position.is_some() {
            self.button_focus = None;
        }
        // A control that is being held follows the pointer here rather
        // than through a second call from the frame loop, so that the
        // drag cannot be one frame behind the finger -- and so that
        // there is no way to move the cursor and forget to move what it
        // is carrying.
        if let (Some(at), Some(scale)) = (position, self.drag_scale) {
            self.drag_control(at, scale);
        }
    }

    /// Takes hold of whatever control is under the pointer.
    ///
    /// Returns whether anything was picked up, so the caller knows
    /// whether the press was spent on a control or should fall through.
    pub fn grab_at_cursor(&mut self, ui_scale: f32) -> bool {
        let Some(at) = self.cursor else {
            return false;
        };
        if self.grab_control(at, ui_scale) {
            self.drag_scale = Some(ui_scale);
            true
        } else {
            false
        }
    }

    /// The buttons the arrow keys walk, on screens that are just a
    /// vertical stack of them.
    fn focus_actions(&self) -> Vec<Action> {
        match self.screen {
            Screen::Main => vec![
                Action::OpenWorlds,
                Action::OpenServers,
                Action::OpenSettings,
                Action::OpenCredits,
                Action::Quit,
            ],
            Screen::Paused => vec![
                Action::Resume,
                Action::OpenSettings,
                Action::OpenExtensions,
                Action::LeaveWorld,
                Action::Quit,
            ],
            _ => Vec::new(),
        }
    }

    fn move_button_focus(&mut self, delta: i32) {
        let count = self.focus_actions().len() as i32;
        if count == 0 {
            return;
        }
        self.button_focus = Some(match self.button_focus {
            Some(current) => (((current as i32 + delta) % count + count) % count) as usize,
            // Nothing focused yet: forwards goes to the first entry,
            // backwards to the last.
            //
            // This used to treat "nothing" as index -1 and fall through
            // to the same arithmetic, which got Down right and Up wrong:
            // -1 - 1 wraps to `count - 2`, so pressing Up on a fresh
            // menu skipped the last entry and landed on the one above
            // it. The menu had three items when that was written, so it
            // looked like an off-by-one nobody could see.
            None if delta < 0 => (count - 1) as usize,
            None => 0,
        });
        // The keyboard now owns the highlight.
        self.cursor = None;
    }

    pub fn tick(&mut self, dt: f32) {
        self.caret_phase = (self.caret_phase + dt).rem_euclid(1.0);
    }

    fn caret_visible(&self) -> bool {
        self.caret_phase < 0.55
    }

    /// The notice as drawable text, in the current language.
    fn notice_line(&self, ctx: &MenuContext) -> Option<(String, bool)> {
        self.notice.as_ref().map(|(notice, good)| {
            let text = match notice {
                Notice::Text(text) => text.clone(),
                Notice::Translated(msg) => say(ctx, *msg).to_string(),
            };
            (text, *good)
        })
    }

    pub fn selected_entry(&self) -> Option<&ServerEntry> {
        self.servers.servers.get(self.selected)
    }

    /// How many worlds the last `build` was given, so key handling can
    /// bound the selection without being handed the list again.
    fn set_world_count(&mut self, count: usize) {
        self.world_count = count;
        if self.world_selected >= count {
            self.world_selected = count.saturating_sub(1);
        }
        // Deleting the world you were scrolled to must not leave the
        // list parked below its own end, staring at blank rows.
        self.world_scroll = self.clamp_scroll(self.world_scroll as i32);
    }

    pub fn move_world_selection(&mut self, delta: i32) {
        let count = self.world_count as i32;
        if count == 0 {
            return;
        }
        self.world_selected = (((self.world_selected as i32 + delta) % count + count) % count) as usize;
        self.show_world();
    }

    /// The wheel, on whichever screen is up.
    ///
    /// One entry point rather than the caller knowing which lists
    /// scroll: the window event arrives with no idea what is on screen,
    /// and every screen that grows a list later should start scrolling
    /// without anybody touching `main`.
    pub fn scroll(&mut self, rows: i32) {
        match self.screen {
            Screen::Worlds => self.scroll_worlds(rows),
            Screen::Settings => self.scroll_settings(rows),
            // ...and the bindings, which reach this the same two ways
            // the settings do: a wheel, and a thumb drag, which the
            // backend turns into a wheel before it ever gets here (see
            // `touch::Gesture::Scrolled` in `lib.rs`). So a phone got a
            // scrollable bindings list without a control of its own.
            Screen::Controls => self.scroll_controls(rows),
            Screen::Servers => self.scroll_servers(rows),
            Screen::Extensions => self.scroll_extensions(rows),
            _ => {}
        }
    }

    /// Scrolls the extensions list; same shape as `scroll_worlds`, and
    /// the same rule -- the wheel looks around the list and the arrow
    /// keys choose in it.
    pub fn scroll_extensions(&mut self, rows: i32) {
        self.extension_scroll = self.clamp_extension_scroll(self.extension_scroll as i32 + rows);
    }

    /// See `clamp_scroll` -- the same rule, for the extensions list.
    fn clamp_extension_scroll(&self, wanted: i32) -> usize {
        let last = self
            .extensions
            .items()
            .len()
            .saturating_sub(self.extension_visible.max(1)) as i32;
        wanted.clamp(0, last.max(0)) as usize
    }

    /// Brings the highlighted extension into the window. `show_world`,
    /// for the third list.
    fn show_extension(&mut self) {
        let visible = self.extension_visible.max(1);
        if self.extension_selected < self.extension_scroll {
            self.extension_scroll = self.extension_selected;
        } else if self.extension_selected >= self.extension_scroll + visible {
            self.extension_scroll = self.extension_selected + 1 - visible;
        }
        self.extension_scroll = self.clamp_extension_scroll(self.extension_scroll as i32);
    }

    /// Moves the highlight, wrapping, and does nothing at all on an
    /// empty list -- which is the ordinary state of this one.
    pub fn move_extension_selection(&mut self, delta: i32) {
        let count = self.extensions.items().len() as i32;
        if count == 0 {
            return;
        }
        self.extension_selected =
            (((self.extension_selected as i32 + delta) % count + count) % count) as usize;
        self.show_extension();
    }

    /// What the server said about what is extending it.
    ///
    /// Handed in by `main.rs`, which owns the connection. The selection
    /// is pulled back inside the new list rather than left where it was:
    /// a reconnect to a server with fewer mods on it would otherwise
    /// highlight a row that is not there, and the detail pane would be
    /// blank with no row to explain it.
    pub fn set_extensions(&mut self, list: primitive_shared::protocol::ExtensionList) {
        self.extension_selected = self
            .extension_selected
            .min(list.items.len().saturating_sub(1));
        self.extensions = Extensions::Known(list);
        self.extension_scroll = self.clamp_extension_scroll(self.extension_scroll as i32);
    }

    /// Throws the list away, because it described a server this client
    /// is no longer talking to.
    ///
    /// **The failure this prevents is a screen that lies quietly**: a
    /// player who leaves one server for another and opens this screen
    /// would be shown the first server's mods, correct-looking and
    /// wrong, until the second server's answer happened to arrive.
    pub fn forget_extensions(&mut self) {
        self.extensions = Extensions::Unasked;
        self.extension_selected = 0;
        self.extension_scroll = 0;
    }

    /// Whether the screen is still waiting on an answer, so `main.rs`
    /// knows whether asking again would be a repeat.
    pub fn extensions_awaited(&self) -> bool {
        matches!(self.extensions, Extensions::Waiting)
    }

    /// Scrolls the server list; same shape as `scroll_worlds`.
    pub fn scroll_servers(&mut self, rows: i32) {
        self.server_scroll = self.clamp_server_scroll(self.server_scroll as i32 + rows);
    }

    /// Scrolls the world list without moving the selection.
    ///
    /// The two are deliberately separate: the wheel looks around the
    /// list and the arrow keys choose in it. Moving the selection with
    /// the wheel is how a player scrolls past the world they meant to
    /// open, presses Enter and loads a different one.
    pub fn scroll_worlds(&mut self, rows: i32) {
        self.world_scroll = self.clamp_scroll(self.world_scroll as i32 + rows);
    }

    /// Scrolls the settings list; same shape as `scroll_worlds`.
    pub fn scroll_settings(&mut self, rows: i32) {
        self.settings_scroll = self.clamp_settings_scroll(self.settings_scroll as i32 + rows);
    }

    /// See `clamp_scroll` -- the same rule, for the server list.
    fn clamp_server_scroll(&self, wanted: i32) -> usize {
        let last = self
            .servers
            .servers
            .len()
            .saturating_sub(self.server_visible.max(1)) as i32;
        wanted.clamp(0, last.max(0)) as usize
    }

    /// Brings the selected server into the window. The world list's
    /// `show_world`, for the other list.
    fn show_server(&mut self) {
        let visible = self.server_visible.max(1);
        if self.selected < self.server_scroll {
            self.server_scroll = self.selected;
        } else if self.selected >= self.server_scroll + visible {
            self.server_scroll = self.selected + 1 - visible;
        }
        self.server_scroll = self.clamp_server_scroll(self.server_scroll as i32);
    }

    /// Scrolls the key-bindings list; same shape as `scroll_settings`.
    pub fn scroll_controls(&mut self, rows: i32) {
        self.controls_scroll = self.clamp_controls_scroll(self.controls_scroll as i32 + rows);
    }

    /// See `clamp_scroll` -- the same rule, for the settings list.
    fn clamp_settings_scroll(&self, wanted: i32) -> usize {
        let last = Setting::ALL.len().saturating_sub(self.settings_visible.max(1)) as i32;
        wanted.clamp(0, last.max(0)) as usize
    }

    /// See `clamp_scroll` -- the same rule, for the key-bindings list.
    fn clamp_controls_scroll(&self, wanted: i32) -> usize {
        let last = crate::ui::keybinds::Action::ALL
            .len()
            .saturating_sub(self.controls_visible.max(1)) as i32;
        wanted.clamp(0, last.max(0)) as usize
    }

    /// The furthest down the list may be scrolled: far enough to put the
    /// last row on screen and not one row further.
    ///
    /// Overscrolling past the end is the thing every list gets wrong in
    /// the same way -- a page of blank rows below the last one, and the
    /// player wondering whether the world they are looking for failed to
    /// load.
    fn clamp_scroll(&self, wanted: i32) -> usize {
        let last = self.world_count.saturating_sub(self.world_visible) as i32;
        wanted.clamp(0, last.max(0)) as usize
    }

    /// Brings the selected row into the window, moving the list as
    /// little as it takes.
    ///
    /// Not "centre it": a list that recentres on every keypress is one
    /// where nothing on screen stays where the player last saw it. Only
    /// the row that has just left the window moves it, and only by
    /// enough to bring it back.
    fn show_world(&mut self) {
        let visible = self.world_visible.max(1);
        if self.world_selected < self.world_scroll {
            self.world_scroll = self.world_selected;
        } else if self.world_selected >= self.world_scroll + visible {
            self.world_scroll = self.world_selected + 1 - visible;
        }
        self.world_scroll = self.clamp_scroll(self.world_scroll as i32);
    }

    pub fn move_selection(&mut self, delta: i32) {
        let count = self.servers.servers.len() as i32;
        if count == 0 {
            return;
        }
        // Wraps, so holding a direction doesn't dead-end.
        self.selected = (((self.selected as i32 + delta) % count + count) % count) as usize;
        // ...and the view follows it, now that the view is a thing of
        // its own. See `show_server`.
        self.show_server();
    }

    /// Hit-tests the cursor against this frame's widgets.
    fn hovered(&self) -> Option<&Action> {
        let (x, y) = self.cursor?;
        self.hot
            .iter()
            .find(|(rect, _)| rect.contains(x, y))
            .map(|(_, action)| action)
    }

    /// A click at the current cursor position.
    pub fn click(&mut self) -> Option<Action> {
        let action = self.hovered().cloned()?;
        Some(self.apply(action))
    }

    /// A key press. Returns an action if this key means one on this
    /// screen; `None` if it was consumed (text entry) or ignored.
    pub fn key(&mut self, key: Key) -> Option<Action> {
        match &self.screen {
            // The controls screen deliberately handles almost nothing
            // here. While it is listening the raw keypress is what the
            // player is *choosing*, and it is taken by `awaiting_key`
            // before this is ever reached; Escape backs out either way.
            Screen::TouchControls => match key {
                // Nothing else: this screen is a finger, and the only
                // key that matters is the one that leaves it.
                //
                // Through `apply`, like every arm around it. Returning
                // the action unapplied runs the caller's side of Back
                // and leaves `self.screen` where it was, so the screen
                // never closes -- which is what the first version of
                // this line did.
                Key::Escape => Some(self.apply(Action::Back)),
                _ => None,
            },
            Screen::Controls => match key {
                Key::Escape => Some(self.apply(Action::Back)),
                // The arrows move the view, not a highlight -- the
                // settings screen's rule, for the same reason: nothing
                // here is "chosen", so a highlight would be a promise
                // Enter cannot keep.
                //
                // **Safe only because of where this is called from.**
                // While a binding is being listened for, the raw key is
                // what the player is choosing, and `lib.rs` takes it
                // through `awaiting_key` before `key` is ever reached.
                // So Up is a scroll here and a binding there, and this
                // arm cannot steal the capture.
                Key::Up => {
                    self.scroll_controls(-1);
                    None
                }
                Key::Down => {
                    self.scroll_controls(1);
                    None
                }
                _ => None,
            },
            // Escape puts a held control down before it leaves, so the
            // way out of "I picked the wrong one up" is the key already
            // in the player's hand rather than a rule to learn.
            Screen::Main | Screen::Paused => match key {
                Key::Up => {
                    self.move_button_focus(-1);
                    None
                }
                Key::Down => {
                    self.move_button_focus(1);
                    None
                }
                Key::Enter => {
                    let action = self.focus_actions().get(self.button_focus?).cloned()?;
                    Some(self.apply(action))
                }
                Key::Escape => Some(if matches!(self.screen, Screen::Paused) {
                    Action::Resume
                } else {
                    Action::Quit
                }),
                _ => None,
            },

            Screen::Servers => match key {
                Key::Up => {
                    self.move_selection(-1);
                    None
                }
                Key::Down => {
                    self.move_selection(1);
                    None
                }
                Key::Enter if !self.servers.servers.is_empty() => {
                    Some(self.apply(Action::Connect(self.selected)))
                }
                Key::Escape => Some(self.apply(Action::Back)),
                Key::Char('a') | Key::Char('A') => Some(self.apply(Action::Add)),
                Key::Char('e') | Key::Char('E') if !self.servers.servers.is_empty() => {
                    Some(self.apply(Action::Edit(self.selected)))
                }
                Key::Delete { .. } if !self.servers.servers.is_empty() => {
                    Some(self.apply(Action::Delete(self.selected)))
                }
                _ => None,
            },

            Screen::Worlds => match key {
                Key::Up => {
                    self.move_world_selection(-1);
                    None
                }
                Key::Down => {
                    self.move_world_selection(1);
                    None
                }
                Key::Enter if self.world_count > 0 => {
                    Some(self.apply(Action::PlayWorld(self.world_selected)))
                }
                Key::Escape => Some(self.apply(Action::Back)),
                Key::Char('n') | Key::Char('N') => Some(self.apply(Action::NewWorld)),
                // R and C, not F2 and Ctrl-C: this menu's keys are all
                // single letters (A and E on the server list, N here),
                // and a screen that learned a function key for one of
                // its six buttons would be a screen with two habits.
                Key::Char('r') | Key::Char('R') if self.world_count > 0 => {
                    Some(self.apply(Action::RenameWorld(self.world_selected)))
                }
                Key::Char('c') | Key::Char('C') if self.world_count > 0 => {
                    Some(self.apply(Action::CopyWorld(self.world_selected)))
                }
                Key::Delete { .. } if self.world_count > 0 => {
                    Some(self.apply(Action::AskDeleteWorld(self.world_selected)))
                }
                _ => None,
            },

            Screen::Editing(_) | Screen::CreatingWorld | Screen::RenamingWorld(_) => {
                let save = match self.screen {
                    Screen::CreatingWorld => Action::CreateWorld,
                    Screen::RenamingWorld(index) => Action::CommitRename(index),
                    _ => Action::Save,
                };
                match key {
                    Key::Tab => self.cycle_field(),
                    Key::Enter => return Some(self.apply(save)),
                    Key::Escape => return Some(self.apply(Action::Cancel)),
                    Key::Char(c) => self.type_char(c),
                    // Up and down do nothing on a form: there is no
                    // list to walk, and a form that scrolled something
                    // behind it while a field had focus would move the
                    // thing the player was looking at.
                    Key::Up | Key::Down => {}
                    other => {
                        if let Some(edit) = edit_for(other) {
                            self.edit(edit);
                        }
                    }
                }
                None
            }

            Screen::Settings => {
                if self.editing_username {
                    match key {
                        Key::Enter | Key::Tab => {
                            return Some(self.apply(Action::CommitUsername))
                        }
                        Key::Escape => {
                            // Abandoning the edit leaves the stored name
                            // alone -- Escape has to mean "never mind".
                            self.drop_typed_username();
                            return None;
                        }
                        Key::Char(c) => self.type_char(c),
                        // **The same `edit` the forms use.** This row
                        // used to carry its own copy of the rules --
                        // one `pop()` for Backspace and nothing else --
                        // so the caret keys worked on two screens out
                        // of three and the third was the one every
                        // player types their name into.
                        other => {
                            if let Some(edit) = edit_for(other) {
                                self.edit(edit);
                            }
                        }
                    }
                    return None;
                }
                match key {
                    Key::Escape | Key::Enter => Some(self.apply(Action::Back)),
                    // The arrows scroll rather than select: nothing on
                    // this screen is "chosen", so a highlight would be
                    // a promise Enter cannot keep.
                    Key::Up => {
                        self.scroll_settings(-1);
                        None
                    }
                    Key::Down => {
                        self.scroll_settings(1);
                        None
                    }
                    _ => None,
                }
            }

            Screen::Credits => match key {
                Key::Escape | Key::Enter => Some(self.apply(Action::Back)),
                _ => None,
            },

            // Up and down move the highlight rather than the view,
            // unlike the settings screen and like every other list:
            // here the highlight decides what the other pane shows, so
            // there is something for it to choose. Enter is Back
            // because there is nothing to open -- see
            // `Msg::ExtensionsReadOnly`.
            Screen::Extensions => match key {
                Key::Up => {
                    self.move_extension_selection(-1);
                    None
                }
                Key::Down => {
                    self.move_extension_selection(1);
                    None
                }
                Key::Escape | Key::Enter => Some(self.apply(Action::Back)),
                _ => None,
            },

            Screen::Confirm { action, .. } => match key {
                // Enter is *not* bound to the destructive answer. The
                // whole point of this screen is that the reflex of
                // pressing Enter cannot delete a world.
                Key::Escape => Some(self.apply(Action::Cancel)),
                Key::Char('y') | Key::Char('Y') => {
                    let action = (**action).clone();
                    Some(self.apply(action))
                }
                Key::Char('n') | Key::Char('N') => Some(self.apply(Action::Cancel)),
                _ => None,
            },

            Screen::Connecting { .. } => match key {
                Key::Escape => Some(self.apply(Action::Cancel)),
                _ => None,
            },

            Screen::Failed { .. } => match key {
                Key::Enter => Some(Action::Retry),
                Key::Escape => Some(self.apply(Action::Back)),
                _ => None,
            },

        }
    }

    /// A character typed into the focused field.
    ///
    /// Filtered to what the bitmap font can actually draw. Accepting a
    /// character that renders as a missing-glyph box would let a player
    /// type a name they can't read back.
    pub fn type_char(&mut self, c: char) {
        if !self.accepts_text() {
            return;
        }
        // The font's own list, not `is_ascii_graphic`: the fields must
        // accept exactly what the font can draw back, no more (a box
        // for a glyph that is not there) and no less (an interface in
        // Russian whose fields refuse Russian).
        if !crate::engine::texture::has_glyph(c) {
            return;
        }
        // A seed is a number. Filtering here rather than at save time
        // means the field can never show something that will be
        // rejected later.
        if self.focus == Field::Seed && !c.is_ascii_digit() {
            return;
        }
        let limit = match self.focus {
            Field::Name => MAX_NAME,
            Field::Address => MAX_ADDRESS,
            Field::Seed => MAX_SEED_DIGITS,
        };
        let field = self.field_mut();
        // A selection is about to be replaced, so a field that is
        // already full still takes the character: select-all-then-type
        // has to work on a field at its limit, which is exactly the
        // field somebody wants to throw away and rewrite.
        if field.chars() < limit || field.selection().is_some() {
            field.insert(c);
        }
    }

    /// True when keystrokes should go into a field rather than be read
    /// as shortcuts.
    pub fn accepts_text(&self) -> bool {
        matches!(
            self.screen,
            Screen::Editing(_) | Screen::CreatingWorld | Screen::RenamingWorld(_)
        ) || (matches!(self.screen, Screen::Settings) && self.editing_username)
    }

    fn field_mut(&mut self) -> &mut TextField {
        match self.focus {
            Field::Name => &mut self.name_input,
            Field::Address => &mut self.address_input,
            Field::Seed => &mut self.seed_input,
        }
    }

    /// The boxes the last build drew the text fields in.
    ///
    /// **Exposed for the check that every one of them raises a
    /// keyboard**, which is the property the seed field broke -- see
    /// `ui::ime`. Read rather than re-derived, for the same reason
    /// `hot` is: the pass that draws is the pass that decides where
    /// things are, and a second opinion about that is how an interface
    /// stops responding where it is drawn.
    ///
    /// Only compiled into the tests: nothing in the game asks, and a
    /// `pub` method with no caller is a warning, which this repository
    /// does not carry.
    #[cfg(test)]
    pub fn field_boxes(&self) -> &[(Rect, Field)] {
        &self.field_boxes
    }

    fn field(&self) -> &TextField {
        match self.focus {
            Field::Name => &self.name_input,
            Field::Address => &self.address_input,
            Field::Seed => &self.seed_input,
        }
    }

    /// One edit of the focused field, whatever screen it is on.
    ///
    /// **One place rather than an arm per screen.** The form screens
    /// and the settings screen's own name row are typed into by the
    /// same keys, and the settings row used to carry its own tiny copy
    /// of the rules -- one `pop()` for Backspace and nothing else at
    /// all, so the caret keys worked on two screens out of three.
    fn edit(&mut self, edit: Edit) {
        if !self.accepts_text() {
            return;
        }
        let field = self.field_mut();
        match edit {
            Edit::Backspace { word: false } => field.backspace(),
            Edit::Backspace { word: true } => field.backspace_word(),
            Edit::Delete { word: false } => field.delete(),
            Edit::Delete { word: true } => field.delete_word(),
            Edit::Move { motion, extend } => field.move_caret(motion, extend),
            Edit::SelectAll => field.select_all(),
        }
    }

    /// What the focused field holds.
    ///
    /// For platforms that keep their own copy of it -- an Android input
    /// method is an editor in its own right and holds the whole field --
    /// so that the two can be compared once a frame. See
    /// `platform::Window::ime_owns_text`.
    ///
    /// Empty where no field has focus, rather than an `Option`: the
    /// caller is asking "what should the input method be holding", and
    /// for a screen with no field the answer is "nothing", not "I do
    /// not know".
    pub fn focused_text(&self) -> &str {
        if !self.accepts_text() {
            return "";
        }
        self.field().text()
    }

    /// Replaces the focused field with what an outside editor says it
    /// holds.
    ///
    /// **Through `type_char`, one character at a time, and that is the
    /// whole design.** The rules about what a field may contain live
    /// there -- the font's glyph list, digits only for a seed, a length
    /// for each field -- and a setter that assigned the string straight
    /// into the field would be a second door into the same state with
    /// none of them. A world called `🙂🙂🙂` renders as three empty
    /// boxes and saves to a directory nobody can name.
    ///
    /// The cost is that the result can differ from what was passed in,
    /// which is exactly why the caller compares them afterwards and
    /// pushes the game's answer back -- see the `AboutToWait` arm.
    pub fn set_focused_text(&mut self, text: &str) {
        if !self.accepts_text() {
            return;
        }
        self.field_mut().clear();
        for c in text.chars() {
            self.type_char(c);
        }
    }

    /// Moves between the fields of whichever form is open.
    fn cycle_field(&mut self) {
        self.focus = match (&self.screen, self.focus) {
            (Screen::CreatingWorld, Field::Name) => Field::Seed,
            (Screen::CreatingWorld, _) => Field::Name,
            // One box, so Tab is a no-op rather than a jump into a
            // field this screen does not draw -- which would be a
            // keystroke that types into nothing visible.
            (Screen::RenamingWorld(_), _) => Field::Name,
            (_, Field::Name) => Field::Address,
            _ => Field::Name,
        };
    }

    /// Starts typing into the name row of the settings screen.
    pub fn begin_username_edit(&mut self, current: String) {
        self.name_input.set_text(current);
        self.focus = Field::Name;
        self.editing_username = true;
        // **On a phone, put the row somewhere the keyboard is not.** The
        // on-screen keyboard covers the bottom of the glass, and the row
        // being typed into can be anywhere in a list of eighteen -- so
        // it is scrolled to the top of the panel, which is the one part
        // of the screen the keyboard cannot reach. Harmless on a desktop
        // and not done there: moving a list the player did not move is a
        // surprise, and a desktop has nothing to hide behind.
        if crate::ui::lang::touch_primary() {
            let name = Setting::ALL
                .iter()
                .position(|setting| setting.is_text())
                .unwrap_or(0);
            self.settings_scroll = self.clamp_settings_scroll(name as i32);
        }
    }

    /// Carries out the parts of an action that belong to the menu: screen
    /// changes and edits to the list. Actions `main.rs` has to handle
    /// (connecting, quitting) are returned unchanged.
    pub fn apply(&mut self, action: Action) -> Action {
        let was = self.screen.clone();
        let result = self.apply_inner(action);
        if self.screen != was {
            // The table still describes the screen we just left.
            self.hot.clear();
            // Otherwise arriving on a new screen finds the highlight
            // already sitting on whichever row happened to share an
            // index with the last one -- so the pause screen would open
            // with QUIT lit up because the main menu was left there.
            self.button_focus = None;
        }
        result
    }

    /// Stops editing the name and keeps what was typed.
    ///
    /// Every ordinary way out of the field goes through here, which is
    /// the point: the list of routes that leave a screen grows, and one
    /// of them forgetting to keep the text is the bug this replaces.
    fn keep_typed_username(&mut self) {
        if self.editing_username {
            self.editing_username = false;
            self.username_pending = Some(self.name_input.text().to_string());
        }
    }

    /// ...and the other answer, for the two actions that mean "never
    /// mind". Escape has always meant that here and still does.
    fn drop_typed_username(&mut self) {
        self.editing_username = false;
    }

    /// Takes the typed name, if there is one waiting.
    ///
    /// Drained rather than read, so that a name is applied once and a
    /// second save does not re-apply a name the player has since
    /// changed by other means.
    pub fn take_typed_username(&mut self) -> Option<String> {
        self.username_pending.take()
    }

    fn apply_inner(&mut self, action: Action) -> Action {
        match &action {
            Action::OpenServers => {
                self.notice = None;
                self.screen = Screen::Servers;
            }
            Action::Back => {
                self.notice = None;
                // Leaving the settings screen is the save -- see the
                // `Action::Back` arm in `lib.rs` -- so it has to be the
                // save of the *name* too, or the one field on this
                // screen that is typed rather than stepped is the one
                // field that does not survive being left.
                self.keep_typed_username();
                self.screen = match self.screen {
                    // Back out of a connection attempt, or out of the
                    // settings, to wherever it was opened from rather
                    // than to a fixed screen. Leaving the settings for
                    // the main menu when they were opened from a paused
                    // world looks exactly like being disconnected.
                    Screen::Failed { .. } | Screen::Connecting { .. } | Screen::Settings => {
                        (*self.came_from).clone()
                    }
                    Screen::TouchControls | Screen::Controls => Screen::Settings,
                    Screen::Extensions => (*self.came_from).clone(),
                    Screen::CreatingWorld
                    | Screen::RenamingWorld(_)
                    | Screen::Confirm { .. } => Screen::Worlds,
                    _ => Screen::Main,
                };
            }
            Action::Select(index) => self.selected = *index,
            Action::Connect(index) => self.selected = *index,
            Action::Add => {
                self.name_input.clear();
                self.address_input.clear();
                self.focus = Field::Name;
                self.notice = None;
                self.screen = Screen::Editing(None);
            }
            Action::Edit(index) => {
                if let Some(entry) = self.servers.servers.get(*index) {
                    self.name_input.set_text(entry.name.clone());
                    self.address_input.set_text(entry.address.clone());
                    self.selected = *index;
                    self.focus = Field::Name;
                    self.notice = None;
                    self.screen = Screen::Editing(Some(*index));
                }
            }
            Action::Delete(index) => {
                if *index < self.servers.servers.len() {
                    let removed = self.servers.servers.remove(*index);
                    // Keep the selection on something that exists.
                    self.selected = self.selected.min(self.servers.servers.len().saturating_sub(1));
                    self.servers.save();
                    self.notice = Some((Notice::Text(format!("removed {}", removed.name)), true));
                }
            }
            Action::Focus(field) => {
                self.focus = *field;
                // The line under the new-world form follows the row the
                // player is on, and a box being typed into is a row they
                // are on: before this, tapping NAME or SEED left the help
                // explaining a world type nobody was looking at, and the
                // sentence saying what a seed *is* could only be reached
                // by stepping a different row.
                self.explaining = match field {
                    Field::Name => FormRow::Name,
                    Field::Seed => FormRow::Seed,
                    _ => self.explaining,
                };
                self.place_caret_under_cursor(*field);
            }
            Action::StepPreset(delta) => {
                self.world_preset = self.world_preset.step(*delta);
                self.explaining = FormRow::Preset;
            }
            Action::StepZone(delta) => {
                self.world_zone = self.world_zone.step(*delta);
                self.explaining = FormRow::Zone;
            }
            Action::StepScale(delta) => {
                self.world_scale = step_scale(self.world_scale, *delta);
                self.explaining = FormRow::Scale;
            }
            Action::RollSeed => {
                // Into the box, not straight into a world: the number is
                // the one thing about a world a player can write down and
                // give to a friend, so they see it before it is used.
                // On a phone the input method's copy follows on the next
                // frame (`ime::Mirror::sync`), as it does for any edit the
                // game makes to a field.
                self.seed_input.set_text(random_seed().to_string());
            }
            Action::Save => return self.save_form(),
            Action::Cancel => {
                self.notice = None;
                // Cancel means cancel.
                self.drop_typed_username();
                self.screen = match self.screen {
                    Screen::Connecting { .. } | Screen::Failed { .. } => (*self.came_from).clone(),
                    Screen::CreatingWorld
                    | Screen::RenamingWorld(_)
                    | Screen::Confirm { .. } => Screen::Worlds,
                    // Cancelling the add/edit server form returns to the
                    // server list.
                    _ => Screen::Servers,
                };
            }

            // ---- worlds ----
            Action::OpenWorlds => {
                self.notice = None;
                self.screen = Screen::Worlds;
                // Opening the screen puts the highlighted world on it.
                // Every *other* way the list moves leaves the scroll
                // alone -- see `build_worlds`.
                self.show_world();
            }
            Action::SelectWorld(index) | Action::PlayWorld(index) => {
                self.world_selected = *index;
            }
            Action::NewWorld => {
                self.name_input.clear();
                self.seed_input.clear();
                self.world_preset = Preset::Normal;
                // The zone too, for the preset's reason: a player who once
                // made a tropical world should not make every world after
                // it in the tropics without having chosen to.
                self.world_zone = Zone::Temperate;
                // ...and the scale, which is the newest generator: a
                // player who once made an archipelago should not make
                // every world after it out of islands by accident.
                self.world_scale = Scale::default();
                self.explaining = FormRow::default();
                self.focus = Field::Name;
                self.notice = None;
                self.screen = Screen::CreatingWorld;
            }
            Action::RenameWorld(index) => {
                self.world_selected = *index;
                self.notice = None;
                self.focus = Field::Name;
                // `main.rs` puts the world's current name in the box,
                // since it owns the saves folder -- the same division
                // `EditUsername` makes. Cleared here so a form that is
                // opened before that happens is empty rather than
                // holding the last thing anybody typed anywhere.
                self.name_input.clear();
                self.screen = Screen::RenamingWorld(*index);
            }
            Action::CommitRename(_) => {
                // Carried out by `main.rs`; this only closes the form.
                self.screen = Screen::Worlds;
            }
            Action::AskDeleteWorld(index) => {
                self.world_selected = *index;
                self.notice = None;
                // The name is put in the question rather than left
                // implicit: "delete this world?" next to a list is a
                // question about whichever row the player *thinks* is
                // selected.
                self.screen = Screen::Confirm {
                    question: Msg::DeleteThisWorld,
                    detail: String::new(),
                    confirm_label: Msg::Delete,
                    action: Box::new(Action::ConfirmedDeleteWorld(*index)),
                };
            }
            Action::ConfirmedDeleteWorld(_) => {
                // Carried out by `main.rs`, which owns the save
                // directory; this only closes the gate.
                self.screen = Screen::Worlds;
            }

            Action::OpenCredits => {
                self.notice = None;
                self.screen = Screen::Credits;
            }

            // ---- extensions ----
            Action::OpenExtensions => {
                self.notice = None;
                // Remembered the way the settings screen remembers it,
                // so Back goes to the paused world it was opened from
                // rather than to the main menu -- which reads as having
                // been disconnected.
                *self.came_from = self.screen.clone();
                self.screen = Screen::Extensions;
                // Asked once per opening, and only when there is no
                // answer yet. A screen that re-asked on every open would
                // spend a player's chat allowance on a list that changes
                // when the server restarts and at no other moment.
                if matches!(self.extensions, Extensions::Unasked) {
                    self.extensions = Extensions::Waiting;
                }
            }
            Action::SelectExtension(index) => {
                self.extension_selected = *index;
                self.show_extension();
            }

            // ---- settings ----
            Action::OpenSettings => {
                self.notice = None;
                self.drop_typed_username();
                // From the top, whatever was looked at last time: the
                // screen is opened to find something, not to resume.
                self.settings_scroll = 0;
                *self.came_from = self.screen.clone();
                self.screen = Screen::Settings;
            }
            Action::OpenTouchControls => {
                self.screen = Screen::TouchControls;
                self.dragging = None;
                self.notice = None;
            }
            Action::ResetTouchControls => {
                // The whole point of `TouchLayout::default` being
                // written out by hand rather than computed: RESET has
                // somewhere to go back to.
                self.arrangement = crate::settings::TouchLayout::default();
                self.dragging = None;
            }
            Action::OpenControls => {
                self.notice = None;
                self.rebinding = None;
                // From the top, like the settings screen: it is opened
                // to find a key, not to resume where somebody left off.
                self.controls_scroll = 0;
                self.screen = Screen::Controls;
            }
            Action::RebindKey(action) => {
                // The next keypress lands on this action. Carried on the
                // menu rather than acted on here, because the key has
                // not been pressed yet.
                self.rebinding = Some(*action);
                self.notice = None;
            }
            Action::ResetKeys => {
                self.rebinding = None;
            }
            Action::EditUsername => {
                // `main.rs` seeds the field, since it owns the settings.
                self.focus = Field::Name;
            }
            Action::CommitUsername => self.editing_username = false,

            _ => {}
        }
        action
    }

    /// Puts the caret where the pointer is, when the pointer is
    /// actually in the field.
    ///
    /// **The gate matters as much as the arithmetic.** `Action::Focus`
    /// is not only a click: a form that fails validation returns one to
    /// put the player in the field that is wrong (see `save_form`), and
    /// there the mouse is wherever it was left. Moving the caret to
    /// that would be a caret dropped at a coordinate nobody aimed with.
    /// So the caret moves only when the pointer is inside the box the
    /// field was drawn in -- which is what a click *is*.
    fn place_caret_under_cursor(&mut self, field: Field) {
        if self.ime_owns_text {
            return;
        }
        let Some((x, y)) = self.cursor else {
            return;
        };
        let Some((rect, _)) = self
            .field_boxes
            .iter()
            .copied()
            .find(|(rect, which)| *which == field && rect.contains(x, y))
        else {
            return;
        };
        let content = self.content_drawn;
        let at = {
            let text = self.field();
            widgets::caret_at_x(rect, content, text.text(), text.caret(), x)
        };
        self.field_mut().place_caret(at);
    }

    /// Validates and stores the form. Returns `Action::Save` on success
    /// and `Action::Focus` on failure -- so a rejected form puts the
    /// cursor in the field that needs fixing rather than just refusing.
    fn save_form(&mut self) -> Action {
        let address = self.address_input.text().trim().to_string();
        if address.is_empty() {
            self.notice = Some((Notice::Translated(Msg::AddressRequired), false));
            self.focus = Field::Address;
            return Action::Focus(Field::Address);
        }
        // A bare host is the commonest mistake and the fix is obvious, so
        // make it rather than rejecting the input.
        let address = if address.contains(':') {
            address
        } else {
            format!("{address}:7878")
        };
        let name = {
            let trimmed = self.name_input.text().trim();
            if trimmed.is_empty() {
                address.clone()
            } else {
                trimmed.to_string()
            }
        };

        let entry = ServerEntry {
            name: name.clone(),
            address,
        };
        match self.screen {
            Screen::Editing(Some(index)) if index < self.servers.servers.len() => {
                self.servers.servers[index] = entry;
                self.selected = index;
                self.notice = Some((Notice::Text(format!("saved {name}")), true));
            }
            _ => {
                self.servers.servers.push(entry);
                self.selected = self.servers.servers.len() - 1;
                self.notice = Some((Notice::Text(format!("added {name}")), true));
            }
        }
        self.servers.save();
        self.screen = Screen::Servers;
        Action::Save
    }

    /// Switches screen from outside the action system.
    ///
    /// Clearing the hit-test table is the point of having this at all.
    /// The table is rebuilt by `build`, so between changing screen and
    /// the next frame it still describes the screen just left -- and a
    /// click landing in that gap fires whatever used to be under the
    /// cursor. Opening the pause menu and immediately clicking could
    /// connect to a server.
    pub fn open(&mut self, screen: Screen) {
        self.screen = screen;
        self.hot.clear();
        self.button_focus = None;
        self.keep_typed_username();
        self.notice = None;
    }

    /// Fills in the detail line of a confirmation gate -- the name of
    /// the thing about to be destroyed.
    pub fn set_confirm_detail(&mut self, text: String) {
        if let Screen::Confirm { detail, .. } = &mut self.screen {
            *detail = text;
        }
    }

    /// **Through the same guard every other transition uses.** See
    /// [`Menu::open`]: the hit-test table describes the screen just
    /// left until the next frame rebuilds it, and a click landing in
    /// that gap fires whatever used to be under the cursor. These two
    /// set the screen from outside the action system -- one from a
    /// button, one from an async failure -- and were the only two that
    /// did it without clearing the table.
    pub fn begin_connecting(&mut self, label: String) {
        // Remembered so cancelling puts the player back where they
        // started. Cancelling a singleplayer world used to drop them on
        // the server list, which they had never visited.
        self.came_from = match self.screen {
            Screen::Connecting { .. } | Screen::Failed { .. } => self.came_from.clone(),
            _ => Box::new(self.screen.clone()),
        };
        self.screen = Screen::Connecting { label };
        self.button_focus = None;
        self.hot.clear();
    }

    pub fn fail(&mut self, reason: String) {
        let label = match &self.screen {
            Screen::Connecting { label } => label.clone(),
            _ => self
                .selected_entry()
                .map(|e| e.name.clone())
                .unwrap_or_default(),
        };
        self.screen = Screen::Failed { label, reason };
        self.hot.clear();
    }

    // --- layout ---

    /// Builds this frame's geometry and, as a side effect, this frame's
    /// hit-test table.
    ///
    /// Layout and hit-testing come from the same pass on purpose: two
    /// passes drift, and a menu whose buttons are a few pixels from where
    /// they are drawn is worse than one with no mouse support at all.
    /// A fingerprint of what `build` would draw, cheap enough to take
    /// every frame.
    ///
    /// The menus used to be rebuilt every frame whether or not anything
    /// on them moved -- and a menu is the *worst* screen to do that to,
    /// because it is all text, and text here is one quad per lit font
    /// pixel. Now `main` compares this key against last frame's and
    /// builds only on a difference.
    ///
    /// The one field hashed with a guard is the caret: it flips twice a
    /// second, which is a real change on a screen with a text field and
    /// noise on every other, where it would force two rebuilds a second
    /// of a screen that is not drawing it.
    ///
    /// The settings go in as their serialised form rather than field by
    /// field, so a setting added later cannot be forgotten here and
    /// leave its row on screen showing a stale value.
    pub fn ui_key(&self, ctx: &MenuContext) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        std::mem::discriminant(&self.screen).hash(&mut h);
        match &self.screen {
            Screen::Editing(existing) => existing.hash(&mut h),
            Screen::Confirm {
                question,
                detail,
                confirm_label,
                ..
            } => {
                question.hash(&mut h);
                detail.hash(&mut h);
                confirm_label.hash(&mut h);
            }
            Screen::TouchControls => {
                // **The arrangement is the screen.** Everywhere else
                // the picture is decided by which screen is up and
                // what is selected on it; here it is decided by where
                // the controls are, and a control dragged with a finger
                // has to be seen moving. Left out, the menu is rebuilt
                // only when something else changes and the button
                // appears to stick to the glass until it is let go.
                for placement in std::iter::once(&self.arrangement.stick)
                    .chain(self.arrangement.buttons.iter())
                {
                    placement.corner.hash(&mut h);
                    placement.inset.0.to_bits().hash(&mut h);
                    placement.inset.1.to_bits().hash(&mut h);
                    placement.shown.hash(&mut h);
                }
            }
            Screen::Connecting { label } => label.hash(&mut h),
            Screen::Failed { label, reason } => {
                label.hash(&mut h);
                reason.hash(&mut h);
            }
            _ => {}
        }
        self.selected.hash(&mut h);
        self.world_selected.hash(&mut h);
        self.world_scroll.hash(&mut h);
        // **And the server list's, which was missing.** The picture is
        // only rebuilt when this number changes, and a wheel over the
        // server list moves nothing else: no cursor position, no
        // selection. Left out, turning the wheel scrolled a list that
        // went on being drawn where it was -- and went on being
        // *pressed* where it was drawn, because the hit-test table is
        // rebuilt at the same moment. The list read as frozen.
        self.server_scroll.hash(&mut h);
        self.settings_scroll.hash(&mut h);
        // ...and the bindings list's, for exactly the reason above: a
        // scroll that does not reach this key is a list that goes on
        // being drawn -- and pressed -- where it was.
        self.controls_scroll.hash(&mut h);
        // The extensions list, and both of the things a player does to
        // it. **The list itself is in here because it arrives late**:
        // the answer comes back several frames after the screen opened,
        // and a key that did not move for it would leave "asking the
        // server..." on the glass until something else happened to
        // change -- which, on a screen with nothing but a BACK button,
        // could be never.
        self.extension_scroll.hash(&mut h);
        self.extension_selected.hash(&mut h);
        std::mem::discriminant(&self.extensions).hash(&mut h);
        for item in self.extensions.items() {
            item.name.hash(&mut h);
            item.enabled.hash(&mut h);
        }
        for entry in &self.servers.servers {
            entry.name.hash(&mut h);
            entry.address.hash(&mut h);
        }
        // The text *and* the caret: moving the caret moves a bar on
        // screen and nothing else, so without it the field would go on
        // being drawn with the caret where it used to be.
        for field in [&self.name_input, &self.address_input, &self.seed_input] {
            field.text().hash(&mut h);
            field.caret().hash(&mut h);
            field.selection().hash(&mut h);
        }
        std::mem::discriminant(&self.focus).hash(&mut h);
        self.editing_username.hash(&mut h);
        self.rebinding
            .as_ref()
            .map(std::mem::discriminant)
            .hash(&mut h);
        self.notice.hash(&mut h);
        self.button_focus.hash(&mut h);
        // The raw position, not "which button": rows light up on hover
        // and several screens lay them out data-dependently, so working
        // out which rect the cursor is in would mean doing the layout --
        // which is the work this key exists to skip. A moving mouse
        // rebuilds the menu; a resting one costs nothing.
        self.cursor
            .map(|(x, y)| (x.to_bits(), y.to_bits()))
            .hash(&mut h);
        let caret_on_screen = self.editing_username
            || matches!(
                self.screen,
                Screen::Editing(_) | Screen::CreatingWorld | Screen::RenamingWorld(_)
            );
        if caret_on_screen {
            self.caret_visible().hash(&mut h);
        }

        match toml::to_string(ctx.settings) {
            Ok(text) => text.hash(&mut h),
            // Unserialisable settings cannot be told apart, so make the
            // key different every time and fall back to rebuilding every
            // frame -- the old behaviour, and correct.
            Err(_) => std::time::Instant::now().hash(&mut h),
        }
        for world in ctx.worlds.list() {
            world.name.hash(&mut h);
            world.seed.hash(&mut h);
            world.last_played.hash(&mut h);
            // The calendar and the size are on the row now, and both
            // change while a world is being played -- a key that left
            // them out is a row that goes on saying the day before last.
            world.world_time.map(f32::to_bits).hash(&mut h);
            world.bytes.hash(&mut h);
        }
        // The world rows show a rough age ("3 min ago") measured from
        // the wall clock, so the clock's minute is part of the picture.
        (worlds::unix_now() / 60).hash(&mut h);
        // Which of the two veils this screen is drawn over. Not the
        // scene itself: the scene is behind the interface rather than
        // part of it, and a menu that rebuilt its geometry because a
        // leaf moved would be rebuilt every frame the sweep turns.
        match ctx.background {
            Backdrop::Bare => 0u8.hash(&mut h),
            Backdrop::Scene(place) => (1u8, place.name()).hash(&mut h),
        }
        h.finish()
    }

    /// The `Vec`-returning form, kept for the tests: they assert on
    /// one widget's output in isolation, which is exactly what appending
    /// into a shared list is designed not to produce.
    #[cfg(test)]
    pub fn build(&mut self, ctx: &MenuContext) -> Vec<crate::ui::hotbar::HotbarVertex> {
        let mut out = Vec::new();
        self.build_into(ctx, &mut out);
        out
    }

    /// The same screen, appended to a list the caller keeps between
    /// frames -- so a rebuild reuses the allocation instead of making a
    /// fresh one.
    pub fn build_into(
        &mut self,
        ctx: &MenuContext,
        out: &mut Vec<crate::ui::hotbar::HotbarVertex>,
    ) {
        self.hot.clear();
        self.field_boxes.clear();
        self.content_drawn = ctx.layout.content();
        self.set_world_count(ctx.worlds.list().len());
        // **The menu keeps the dark skin.** It is what you look at
        // *instead of* the world -- before there is one, or with it
        // paused behind a scrim -- and a big pale slab there is a wall
        // of grey in a dark room. The stone belongs to the screens that
        // are part of the world: the pack, a chest, a hearth.
        // Lettered for this screen, not for a desktop: a button drawn
        // for a finger and written on for a mouse is a big empty slab
        // with a small word in the middle of it. See `Painter::content`.
        let mut p = Painter::onto_themed(ctx.font, std::mem::take(out), widgets::Theme::DARK)
            .with_content(ctx.layout.content());
        let hover = self.cursor;

        match self.screen.clone() {
            Screen::Main => self.build_main(&mut p, hover, ctx),
            Screen::Worlds => self.build_worlds(&mut p, hover, ctx),
            Screen::CreatingWorld => self.build_world_form(&mut p, hover, ctx),
            Screen::Servers => self.build_servers(&mut p, hover, ctx),
            Screen::Editing(existing) => self.build_form(&mut p, hover, ctx, existing.is_some()),
            Screen::Settings => self.build_settings(&mut p, hover, ctx),
            Screen::TouchControls => self.build_touch_controls(&mut p, ctx),
            Screen::Controls => self.build_controls(&mut p, hover, ctx),
            Screen::Credits => self.build_credits(&mut p, hover, ctx),
            Screen::Extensions => self.build_extensions(&mut p, hover, ctx),
            Screen::Confirm {
                question,
                detail,
                confirm_label,
                action,
            } => {
                self.build_confirm(&mut p, hover, ctx, question, &detail, confirm_label, *action)
            }
            Screen::Connecting { label } => self.build_connecting(&mut p, hover, ctx, &label),
            Screen::Failed { label, reason } => {
                self.build_failed(&mut p, hover, ctx, &label, &reason)
            }
            Screen::Paused => self.build_paused(&mut p, hover, ctx),
            Screen::RenamingWorld(index) => {
                self.build_rename_world(&mut p, hover, ctx, index)
            }
        }

        *out = p.into_vertices();
    }

    fn build_worlds(&mut self, p: &mut Painter, cursor: Option<(f32, f32)>, ctx: &MenuContext) {
        // A list, so it gets the treatment the settings list got: rows
        // as tall as the finger that has to hit them, and *fewer of
        // them* when there is no room -- never the same number squeezed.
        let layout = ctx.layout;
        self.backdrop(p, ctx);

        // Built from the bottom of the glass upward, because that is
        // where the furniture under the list is pinned; the panel takes
        // what is left between it and the title.
        let help_top = -0.83;
        let button_height = layout.at(0.10).max(layout.finger());
        let back_y0 = help_top + layout.at(0.08);
        let row_y0 = back_y0 + button_height + layout.at(0.08);
        let notice_y = row_y0 + button_height + layout.at(0.11);

        let worlds = ctx.worlds.list();

        let half_width = layout.panel_half_width(0.95);
        let floor = notice_y + layout.at(0.06);
        // **Rule 2 at the top of `widgets`: a panel is sized to what is
        // in it, never to the worst case.** This one reached from the
        // furniture to a fixed 0.66 whatever was in the list, so the
        // screen a player actually has -- three or four worlds on a
        // panel with room for eight -- was a slab with a hole in the
        // bottom two thirds of it.
        //
        // **Its floor stays where it always was and its top is what
        // moves.** The other way round -- top pinned, floor rising --
        // would pull the whole list up the glass as worlds are added,
        // so the row a thumb reaches for would be somewhere new every
        // time one is made. The title follows the top down instead,
        // which is a word moving rather than a target moving.
        //
        // Three rows is the floor: below that the panel is smaller than
        // the sentence an empty list has to hold, and a list is a place
        // before it is a list.
        let pad = layout.at(0.03);
        let gap = layout.at(0.014);
        let row_height = layout.at(0.11).max(layout.finger());
        let wanted = worlds.len().max(3) as f32;
        let top = (floor + pad * 2.0 + row_height * wanted + gap * (wanted - 1.0)).min(0.66);
        let panel = Rect::new(-half_width, floor, half_width, top);
        p.panel(panel);
        // **The title follows the panel down**, and never further up
        // than the 0.86 it has always been at. A heading pinned to the
        // top of the glass over a panel that is now as tall as its
        // contents is a word on its own in an empty half of the screen,
        // which is the hole rule 2 moved rather than closed.
        self.title(
            p,
            say(ctx, Msg::Worlds),
            (panel.y1 + layout.at(0.06) + widgets::cell_height(3.0)).min(0.86),
        );
        if worlds.is_empty() {
            p.text_centred(
                say(ctx, crate::ui::lang::by_input(Msg::NoWorldsYet, Msg::NoWorldsYetTouch)),
                panel.centre_x(),
                // Down from the top by a fixed amount, or by a share of
                // the panel -- whichever is less. Scaled on its own it
                // ends up under the last row it is standing in for.
                panel.y1 - layout.at(0.41).min(panel.height() * 0.43),
                layout.content(),
                MENU.ink_dim,
            );
        }

        let mut y = panel.y1 - pad - row_height;
        // `n` rows have `n - 1` gaps between them; counting one after
        // the last throws away a row that had room for itself.
        let visible = (((panel.height() - pad * 2.0 + gap + 0.002) / (row_height + gap)) as usize)
            .max(1);
        // The panel decides how many rows fit, so the input handler is
        // told rather than asked: the wheel arrives between frames and
        // has no panel to measure.
        self.world_visible = visible;
        // Clamped, not re-shown. **The wheel is allowed to scroll the
        // selection off the screen**, which is the whole difference
        // between a list you can look around and one that snaps back to
        // the highlight the instant you let go of the wheel. The
        // selection is brought back into view when it *moves* (see
        // `show_world`) and when the screen is opened, and at no other
        // time.
        self.world_scroll = self.clamp_scroll(self.world_scroll as i32);
        let first = self.world_scroll;
        let now = worlds::unix_now();

        // Room for the scrollbar, taken out of the rows whether or not
        // there is one to draw: a list that reflows the moment it grows
        // past the window is a list where every row shifts sideways when
        // you add a world.
        let gutter = layout.at(0.022);

        // By index, not by iterator: the index *is* the row's identity
        // -- it goes into `Action::PlayWorld` and is compared against
        // the selection -- so enumerating a slice would only put it back.
        #[allow(clippy::needless_range_loop)]
        for index in first..worlds.len().min(first + visible) {
            let world = &worlds[index];
            let rect = Rect::new(panel.x0 + pad, y, panel.x1 - pad - gutter, y + row_height);
            let selected = index == self.world_selected;

            p.well(rect, if selected { MENU.row_selected } else { MENU.row });
            if selected || self.is_hovered(rect, cursor) {
                p.border(rect, 0.003, if selected { MENU.accent } else { MENU.dark });
            }
            // **What the world *is*, not what its file is.** The row
            // used to be a seed and "3 d ago", which answers neither of
            // the two questions anybody opens this screen with: which of
            // these was I last in, and which one is the one I got through
            // the winter in. See `world_detail`.
            let detail = world_detail(world, now, ctx.settings.language);
            p.row_labels(
                rect,
                layout.at(0.025),
                &world.name,
                // **The name is the bright ink whether the row is
                // chosen or not.** It was the quiet one, so a list of
                // four worlds was four dim names and four dim detail
                // lines at the same weight, with nothing to run an eye
                // down. What the highlight says is already said by the
                // row's own fill and its amber edge; what the ink has to
                // say is "this is the name and that is the note".
                if selected { widgets::TEXT } else { MENU.ink },
                &detail,
                MENU.ink_dim,
                layout.content(),
            );

            // Same rule as the server list: click to select, click the
            // selected row again to play.
            let action = if selected {
                Action::PlayWorld(index)
            } else {
                Action::SelectWorld(index)
            };
            self.hot.push((rect, action));
            y -= row_height + gap;
        }

        // The scrollbar: a track down the gutter with a thumb on it as
        // long a share of the track as the window is of the list.
        //
        // Only when there is something to scroll. A full-length thumb
        // that can never move is not information, it is furniture --
        // and, worse, it is furniture that says "there is more" to
        // anybody glancing at it.
        p.scrollbar(
            Rect::new(
                panel.x1 - pad - gutter + layout.at(0.006),
                panel.y0 + pad,
                panel.x1 - pad,
                panel.y1 - pad,
            ),
            first,
            visible,
            worlds.len(),
        );

        if let Some((text, good)) = self.notice_line(ctx) {
            let colour = if good { widgets::TEXT_GOOD } else { widgets::TEXT_BAD };
            p.text_centred(&text, 0.0, notice_y, layout.at(0.9), colour);
        }

        let any = !worlds.is_empty();
        let selected = self.world_selected;
        // Three across, cut from the panel's own width so the row ends
        // where the panel does, and BACK under the middle one -- see
        // `columns`. On a 22:9 screen the panel is twice as wide, and the
        // buttons are as wide as the room they stand under.
        let span = (panel.x0, panel.x1);
        let column_gap = layout.at(BUTTON_GAP);
        // **Six buttons in the two rows that were already there.** The
        // screen had PLAY, NEW, DELETE and a BACK alone under the middle
        // column, so a third of the furniture was empty air -- and the
        // two things a list of saves has always wanted, a new name and a
        // spare copy, had nowhere to be. RENAME and COPY take the two
        // columns beside BACK, which stays in the middle where it is
        // drawn today: nothing moved, two things arrived.
        //
        // The five that act on a world are dark until there is one, so
        // the screen a new install opens on offers exactly the one thing
        // there is to do.
        for (row_y, buttons) in [
            (
                row_y0,
                [
                    (say(ctx, Msg::Play), Action::PlayWorld(selected), any),
                    (say(ctx, Msg::New), Action::NewWorld, true),
                    (say(ctx, Msg::Delete), Action::AskDeleteWorld(selected), any),
                ],
            ),
            (
                back_y0,
                [
                    (say(ctx, Msg::Rename), Action::RenameWorld(selected), any),
                    (say(ctx, Msg::Back), Action::Back, true),
                    (say(ctx, Msg::CopyWorld), Action::CopyWorld(selected), any),
                ],
            ),
        ] {
            for (column, (label, action, enabled)) in buttons.into_iter().enumerate() {
                let (x0, x1) = columns(span, 3, column_gap, column, column);
                self.add_button(
                    p,
                    cursor,
                    Rect::new(x0, row_y, x1, row_y + button_height),
                    label,
                    action,
                    enabled,
                );
            }
        }

        p.text_centred(
            say(
                ctx,
                crate::ui::lang::by_input(Msg::WorldsHelpWithRename, Msg::WorldsHelpTouch),
            ),
            0.0,
            help_top,
            0.8,
            widgets::TEXT_DIM,
        );
    }

    /// The rename form: one box, two buttons, and the name that is in it.
    ///
    /// **The new-world form with four rows taken out**, deliberately: it
    /// is laid out by the same `FormRows`, so the box is the same size in
    /// the same place and dodges the on-screen keyboard the same way. A
    /// screen with one field on it that was arranged by hand would be the
    /// one form on this menu whose field a phone's keyboard could cover.
    fn build_rename_world(
        &mut self,
        p: &mut Painter,
        cursor: Option<(f32, f32)>,
        ctx: &MenuContext,
        index: usize,
    ) {
        let layout = ctx.layout;
        self.backdrop(p, ctx);
        let rows = FormRows::plan(layout, 1, (0.48, 0.22, 0.08, 0.03));
        if layout.keyboard_top().is_none() {
            self.title(p, say(ctx, Msg::RenameThisWorld), 0.74);
        }

        let half_width = layout.panel_half_width(0.95);
        let panel = Rect::new(-half_width, rows.panel_bottom(1), half_width, rows.top);
        p.panel(panel);

        let pad = layout.at(0.05);
        let (label_x, label_y, rect) = rows.row(panel, pad, 0);
        p.text(say(ctx, Msg::Name), label_x, label_y, rows.label_size, MENU.ink_dim);
        p.text_field(
            rect,
            &self.name_input,
            say(ctx, Msg::WorldNamePlaceholder),
            self.focus == Field::Name,
            self.caret_visible(),
        );
        self.hot.push((rect, Action::Focus(Field::Name)));
        self.field_boxes.push((rect, Field::Name));

        let content = layout.within(panel.y0 + 1.0 - 0.03, 0.44);
        let notice_y = panel.y0 - content.at(0.06);
        if let Some((text, good)) = self.notice_line(ctx) {
            let colour = if good { widgets::TEXT_GOOD } else { widgets::TEXT_BAD };
            p.text_centred(&text, 0.0, notice_y, content.at(0.9), colour);
        } else {
            // **The folder is not moving, and saying so is the point.**
            // A player renaming a save is asking "will this lose it"; the
            // honest answer is a sentence, and it is the same sentence
            // `Worlds::rename` explains itself with.
            p.text_centred(
                say(ctx, Msg::NameHelp),
                0.0,
                notice_y,
                content.at(0.8),
                widgets::TEXT_DIM,
            );
        }

        let button_height = content.at(0.10).max(content.finger());
        let button_y = notice_y - content.at(0.09) - button_height / 2.0;
        let span = (panel.x0, panel.x1);
        let column_gap = layout.at(BUTTON_GAP);
        let (y0, y1) = (button_y - button_height / 2.0, button_y + button_height / 2.0);
        let (x0, x1) = columns(span, 2, column_gap, 0, 0);
        self.add_button(
            p,
            cursor,
            Rect::new(x0, y0, x1, y1),
            say(ctx, Msg::Rename),
            Action::CommitRename(index),
            !self.name_input.text().trim().is_empty(),
        );
        let (x0, x1) = columns(span, 2, column_gap, 1, 1);
        self.add_button(p, cursor, Rect::new(x0, y0, x1, y1), say(ctx, Msg::Cancel), Action::Cancel, true);

        // **Not the new-world form's line**, which names a key this
        // screen has not got: there is one box here, so "tab switches
        // field" is an instruction to press a key that does nothing.
        p.text_centred(
            say(ctx, crate::ui::lang::by_input(Msg::RenameFormHelp, Msg::RenameFormHelpTouch)),
            0.0,
            button_y - button_height / 2.0 - content.at(0.11),
            0.8,
            widgets::TEXT_DIM,
        );
    }

    fn build_world_form(&mut self, p: &mut Painter, cursor: Option<(f32, f32)>, ctx: &MenuContext) {
        // **The screen standing between a phone and playing at all**,
        // drawn for a desktop at 40% of the width of a 22:9 screen with
        // empty margins all round it.
        //
        // Sideways it has room to spare, so the panel takes the width
        // and the fields get long -- which is what a field being typed
        // into wants anyway. Downwards it has almost none, and what
        // gives there is the *arrangement*: see `FormRows`.
        let layout = ctx.layout;
        self.backdrop(p, ctx);
        // The title goes where the rows need the room. It is decoration:
        // a player who has just pressed NEW knows what screen this is.
        // Four rows since the world is laid somewhere on the planet: the
        // zone is a choice nobody can make later (`worldgen::Zone`), so it
        // is on the form rather than in a settings file.
        //
        // **A tighter pitch since that fourth row, and it is a fix.** The
        // arrangement was drawn for three rows -- a panel top of 0.44, a
        // row every 0.28 and 0.13 of air over the first -- and CLIMATE was
        // added under it at the same pitch. The panel grew down by a whole
        // row, CREATE and CANCEL ended on the bottom edge of the glass,
        // and the line under them saying what the keys do was drawn off
        // it altogether. 0.22 keeps each label as close to its own box
        // as it was and closes the gap to the *next* label instead, which
        // is the gap that was only ever air; the panel starts a little
        // higher, under a title moved up to match.
        //
        // **Five rows since the country's size came onto it.** That was a
        // constant in `Worlds::create_in` -- every world was the newest
        // generator and nobody could ask for the archipelago -- and it is
        // the one choice here with no better answer, so it is a row.
        // `FormRows` takes the count, so the pitch closes to hold it and
        // the panel does not grow off the bottom of a phone; what pays
        // for the fifth row is the air between labels, which was only
        // ever air.
        const ROWS: usize = 5;
        let rows = FormRows::plan(layout, ROWS, (0.48, 0.22, 0.08, 0.03));
        if layout.keyboard_top().is_none() {
            self.title(p, say(ctx, Msg::NewWorld), 0.74);
        }

        let half_width = layout.panel_half_width(0.95);
        let panel = Rect::new(-half_width, rows.panel_bottom(ROWS), half_width, rows.top);
        p.panel(panel);

        let pad = layout.at(0.05);
        for (index, msg) in
            [Msg::Name, Msg::Seed, Msg::WorldType, Msg::Climate, Msg::WorldScale]
                .into_iter()
                .enumerate()
        {
            let (label_x, label_y, rect) = rows.row(panel, pad, index);
            p.text(say(ctx, msg), label_x, label_y, rows.label_size, MENU.ink_dim);
            match index {
                0 => {
                    p.text_field(
                        rect,
                        &self.name_input,
                        say(ctx, Msg::WorldNamePlaceholder),
                        self.focus == Field::Name,
                        self.caret_visible(),
                    );
                    self.hot.push((rect, Action::Focus(Field::Name)));
                    self.field_boxes.push((rect, Field::Name));
                }
                1 => {
                    // **What a blank field will actually do, as a
                    // placeholder rather than as a value**: roll a seed.
                    // It used to be the settings' one fixed number, so
                    // every world nobody typed a seed for was the same
                    // world ("возможность выбрать случайный сид"). A
                    // placeholder rather than a number written in, for the
                    // reason it always was one: a number in the box reads
                    // as one somebody chose.
                    //
                    // **And a button to roll one now**, at the end of the
                    // box and a finger wide, for the player who wants to
                    // see the number before the world is made from it --
                    // or to roll again. The box is what is left, and it is
                    // the box that hit-tests as the box.
                    let roll = layout.at(0.16).max(layout.finger()).min(rect.width() / 3.0);
                    let field = Rect::new(rect.x0, rect.y0, rect.x1 - roll - layout.at(0.01), rect.y1);
                    let button = Rect::new(rect.x1 - roll, rect.y0, rect.x1, rect.y1);
                    p.text_field(
                        field,
                        &self.seed_input,
                        say(ctx, Msg::SeedRandom),
                        self.focus == Field::Seed,
                        self.caret_visible(),
                    );
                    self.hot.push((field, Action::Focus(Field::Seed)));
                    self.field_boxes.push((field, Field::Seed));
                    self.add_button(p, cursor, button, say(ctx, Msg::RollSeed), Action::RollSeed, true);
                }
                4 => {
                    // How big the country is: the archipelago the game
                    // shipped with, or the planet. See `Scale`.
                    let value = say(ctx, scale_label(self.world_scale));
                    self.stepper(p, cursor, layout, rect, value, Action::StepScale(-1), Action::StepScale(1));
                }
                3 => {
                    // The zone: stepped, on the world type's terms, and
                    // for its reason -- a latitude typed as a number is a
                    // world laid on the ice by a typo.
                    //
                    // **What this row chooses is where the player wakes up
                    // on the planet, not which planet they get.** A seed is
                    // one globe (`worldgen::PLANET_ORIGIN_DEGREES`) and a
                    // zone is a place on it, so the same seed in two zones
                    // is two countries of one world rather than two worlds.
                    // The row is still headed `Msg::Climate`, which says
                    // the true thing about *living* there and nothing about
                    // the choice being a place; the wording that would say
                    // both lives in `ui::lang`, which this change did not
                    // own. Whoever moves it: the label wants to read "where
                    // you wake" rather than "climate", and the four help
                    // lines want the latitude in them, which they already
                    // have.
                    let value = say(ctx, zone_label(self.world_zone));
                    self.stepper(p, cursor, layout, rect, value, Action::StepZone(-1), Action::StepZone(1));
                }
                _ => {
                    // The world type: a stepped row rather than a text
                    // field, because it is a choice from a short list
                    // and typing "tset" should not be able to produce a
                    // world nobody meant to make.
                    let value = say(ctx, preset_label(self.world_preset));
                    self.stepper(p, cursor, layout, rect, value, Action::StepPreset(-1), Action::StepPreset(1));
                }
            }
        }

        // Under the panel, and following it down rather than pinned to
        // the bottom of the glass: on a screen with no vertical room the
        // panel is what moves, and furniture measured from the edge ends
        // up underneath it.
        let content = layout.within(panel.y0 + 1.0 - 0.03, 0.44);
        let notice_y = panel.y0 - content.at(0.06);
        if let Some((text, good)) = self.notice_line(ctx) {
            let colour = if good { widgets::TEXT_GOOD } else { widgets::TEXT_BAD };
            p.text_centred(&text, 0.0, notice_y, content.at(0.9), colour);
        } else {
            // What the chosen type *is*, under the row that chose it:
            // "TEST" means nothing until something says the world is a
            // flat field with one of everything already built on it.
            let help = match self.explaining {
                FormRow::Name => Msg::NameHelp,
                FormRow::Seed => Msg::SeedHelp,
                FormRow::Preset => preset_help(self.world_preset),
                FormRow::Zone => zone_help(self.world_zone),
                FormRow::Scale => scale_help(self.world_scale),
            };
            p.text_centred(
                say(ctx, help),
                0.0,
                notice_y,
                content.at(0.8),
                widgets::TEXT_DIM,
            );
        }

        let button_height = content.at(0.10).max(content.finger());
        let button_y = notice_y - content.at(0.09) - button_height / 2.0;
        // The two halves of the panel over them, so the pair ends where the
        // panel does, as the rows under the lists do. See `columns`.
        //
        // Tried first and dropped: the middle two quarters. It kept the pair
        // the width it had always been, and under a panel twice as wide as
        // the two of them it read as the same small afterthought it was.
        let span = (panel.x0, panel.x1);
        let column_gap = layout.at(BUTTON_GAP);
        let (y0, y1) = (button_y - button_height / 2.0, button_y + button_height / 2.0);
        let (x0, x1) = columns(span, 2, column_gap, 0, 0);
        self.add_button(p, cursor, Rect::new(x0, y0, x1, y1), say(ctx, Msg::Create), Action::CreateWorld, true);
        let (x0, x1) = columns(span, 2, column_gap, 1, 1);
        self.add_button(p, cursor, Rect::new(x0, y0, x1, y1), say(ctx, Msg::Cancel), Action::Cancel, true);

        p.text_centred(
            say(ctx, crate::ui::lang::by_input(Msg::WorldFormHelp, Msg::WorldFormHelpTouch)),
            0.0,
            button_y - button_height / 2.0 - content.at(0.11),
            0.8,
            widgets::TEXT_DIM,
        );
    }

    fn build_settings(&mut self, p: &mut Painter, cursor: Option<(f32, f32)>, ctx: &MenuContext) {
        // **The screen this whole layout pass was for.** It used to be a
        // fixed box -- `-1.15..1.15` by `-0.62..0.76` -- drawn once and
        // multiplied by whatever the interface scale allowed. On a 22:9
        // phone that is a short wide letterbox sitting in the middle of
        // a screen twice as wide as it, with eighteen rows squeezed into
        // it and the buttons that change them too small to hit.
        //
        // Nothing here is a constant any more except the numbers that
        // *should* stay the size they are on a desktop. Three kinds of
        // number, and which one a thing is is the whole design:
        //
        // * `layout.at(..)` -- the rows and the buttons, because those
        //   are what the player was asking about.
        // * bare literals -- the title, the help line, the air between
        //   the panel and the glass. A title scaled half again on a
        //   screen with no vertical room costs two rows.
        // * `layout.finger()` -- the floor under anything tapped.
        //
        // What gives, when the rows are a finger tall and the screen is
        // short, is **how many are visible**. That is the one thing that
        // can give: a row shorter than a finger is a row that cannot be
        // used, and a panel taller than the screen is rows that cannot
        // be seen at all.
        let layout = ctx.layout;
        self.backdrop(p, ctx);

        // Measured from the edge of the window rather than written down
        // as `0.90`, which is the top of a screen that is not being
        // scaled and half a title above the top of one that is.
        const EDGE: f32 = 1.0;
        let title_top = EDGE - 0.10;
        self.title(p, say(ctx, Msg::Settings), title_top);
        let help_top = -EDGE + 0.10;

        // The two buttons under the panel get pressed, so they get a
        // finger; everything else down here is only read.
        let footer_height = layout.at(0.10).max(layout.finger());
        let footer_y0 = help_top + 0.07;
        let footer_y1 = footer_y0 + footer_height;

        let half_width = layout.panel_half_width(1.15);
        let panel = Rect::new(-half_width, footer_y1 + 0.11, half_width, title_top - 0.14);
        p.panel(panel);

        // A window onto the list, exactly the way the world list is
        // one: full-size rows, the wheel and the arrow keys to move,
        // and a scrollbar in the gutter saying where you are.
        //
        // This screen used to divide the panel between however many
        // rows there were instead. That was correct for any number of
        // settings and readable for about twelve of them; at seventeen
        // every row had shrunk to a strip. A row's height is not the
        // variable here -- how many are on screen is.
        /// How large the word on a switch is next to the number on a
        /// slider.
        ///
        /// A shade smaller than the label beside it. The reading on a
        /// slider is a number with a unit -- `24 chunks`, `95 deg` --
        /// and it wants to be read at a glance; the reading on a switch
        /// is a *word*, and a word at the label's size reads as a second
        /// label arguing with the first. Three passes at this: 0.84 was
        /// still competing, 0.66 was quiet to the point of being hard to
        /// read -- the ON and OFF of a switch are the one reading on
        /// this screen a player checks at a glance, and at two thirds of
        /// the label they were the smallest text on it. 0.80 is quieter
        /// than the label and still a word rather than a footnote.
        const TOGGLE_TEXT_SCALE: f32 = 0.80;
        // See `ListRows`: the row height, the gap, the padding, the
        // gutter and "how many fit" are the key-bindings screen's as
        // well, and they are one piece of arithmetic because the two
        // screens are one list.
        let rows = ListRows::plan(layout, panel);
        let inset = rows.inset;
        let visible = rows.visible;
        self.settings_visible = visible;
        self.settings_scroll = self.clamp_settings_scroll(self.settings_scroll as i32);
        let first = self.settings_scroll;
        let count = Setting::ALL.len();
        // How much of a row its buttons own, and therefore what
        // everything else on it keeps clear of. Wide enough for the two
        // of them side by side with a finger's width each, which on a
        // desktop is the column the screen was always designed around.
        let edge = layout.at(0.02);
        let controls = layout
            .at(widgets::CONTROL_COLUMN)
            .max(layout.finger() * 2.0 + edge * 2.0);
        let style = widgets::RowStyle {
            text: layout.content(),
            value: 1.0,
            controls,
        };
        for (index, setting) in Setting::ALL.iter().skip(first).take(visible).copied().enumerate() {
            let row = rows.row(panel, index);

            if setting.is_text() {
                p.well(row, MENU.row);
                p.label_left(row, setting.label_in(ctx.settings), 0.025, layout.content(), MENU.ink);
                // The name field ends where the buttons on every other
                // row begin, so the right-hand edge of the screen is one
                // line rather than two.
                let field_rect = Rect::new(
                    row.x1 - layout.at(0.78).max(controls),
                    row.y0 + inset,
                    row.x1 - edge,
                    row.y1 - inset,
                );
                debug_assert!(
                    field_rect.x1 <= row.x1,
                    "the name field runs off the end of its row"
                );
                if self.editing_username {
                    p.text_field(
                        field_rect,
                        &self.name_input,
                        say(ctx, Msg::UsernamePlaceholder),
                        true,
                        self.caret_visible(),
                    );
                    self.field_boxes.push((field_rect, Field::Name));
                    // **A click inside the field being typed puts the
                    // caret in it**, exactly as it does on the two form
                    // screens. It used to commit the name, which was
                    // the only way to end the edit with a mouse before
                    // leaving the screen kept it -- and once the field
                    // has a caret you can aim at, a click landing on
                    // "done" instead is the field refusing the first
                    // thing anybody tries on it. Enter and Tab still
                    // commit, and so does leaving the screen: see
                    // `keep_typed_username`.
                    self.hot.push((field_rect, Action::Focus(Field::Name)));
                } else {
                    p.field(field_rect, &setting.value(ctx.settings), false, false);
                    self.hot.push((field_rect, Action::EditUsername));
                }
            } else {
                // A setting that does nothing while another is off is
                // shown greyed rather than hidden: knowing the option
                // exists is most of what the row is there for.
                let enabled =
                    !setting.depends_on_menu_background() || ctx.settings.menu_background;
                // A switch reads a word and everything else reads a
                // number. The word comes down a notch -- see
                // `RowStyle::value` -- and the numbers stay exactly the
                // size they were.
                p.setting_row(
                    row,
                    setting.label_in(ctx.settings),
                    &setting.value(ctx.settings),
                    enabled,
                    widgets::RowStyle {
                        value: if setting.is_toggle() { TOGGLE_TEXT_SCALE } else { 1.0 },
                        ..style
                    },
                );
                let column = row.x1 - controls;
                if setting.is_toggle() {
                    // One wide button: a switch has no "less" and "more".
                    let toggle = Rect::new(column, row.y0 + inset, row.x1 - edge, row.y1 - inset);
                    self.add_button(p, cursor, toggle, say(ctx, Msg::Toggle), Action::Tweak(setting, 1), enabled);
                } else {
                    let half = (row.x1 - edge - column - edge) / 2.0;
                    let minus = Rect::new(column, row.y0 + inset, column + half, row.y1 - inset);
                    let plus =
                        Rect::new(row.x1 - edge - half, row.y0 + inset, row.x1 - edge, row.y1 - inset);
                    self.add_button(p, cursor, minus, "-", Action::Tweak(setting, -1), enabled);
                    self.add_button(p, cursor, plus, "+", Action::Tweak(setting, 1), enabled);
                }
            }
        }

        // The scrollbar, drawn by the same rules as the world list's:
        // only when there is something to scroll.
        p.scrollbar(rows.track(layout, panel), first, visible, count);

        if let Some((text, good)) = self.notice_line(ctx) {
            let colour = if good { widgets::TEXT_GOOD } else { widgets::TEXT_BAD };
            p.text_centred(&text, 0.0, panel.y0 - 0.08, 0.9, colour);
        }

        // The CONTROLS screen is a list of keys to rebind, which is a
        // screen for hardware a phone has not got. Hidden rather than
        // greyed out: a row saying "you cannot have this" is still a row
        // taking space on a screen with none to spare, and DONE on its
        // own centres.
        // Never wider than the panel above them, which is what keeps
        // the pair inside the glass at the top of the scale: at 4.0 two
        // buttons at their asked-for width are wider than the window.
        let footer_half = layout
            .at(0.60)
            .max(layout.finger() * 2.0)
            .min(half_width - 0.02);
        if layout.is_touch() {
            // One button, centred. A phone used to have a second one
            // arranging the thumb controls, which is what stands here
            // instead of the key bindings: a phone has no keys to bind
            // and a desktop has no thumb controls to move, so the two
            // swap places rather than both being offered everywhere.
            self.add_button(
                p,
                cursor,
                Rect::new(-footer_half - 0.02, footer_y0, -0.02, footer_y1),
                say(ctx, Msg::ArrangeControls),
                Action::OpenTouchControls,
                true,
            );
            self.add_button(
                p,
                cursor,
                Rect::new(0.02, footer_y0, footer_half + 0.02, footer_y1),
                say(ctx, Msg::Done),
                Action::Back,
                true,
            );
        } else {
            self.add_button(
                p,
                cursor,
                Rect::new(-footer_half - 0.02, footer_y0, -0.02, footer_y1),
                say(ctx, Msg::Controls),
                Action::OpenControls,
                true,
            );
            self.add_button(
                p,
                cursor,
                Rect::new(0.02, footer_y0, footer_half + 0.02, footer_y1),
                say(ctx, Msg::Done),
                Action::Back,
                true,
            );
        }
        p.text_centred(
            say(ctx, Msg::SettingsHelp),
            0.0,
            help_top,
            0.8,
            widgets::TEXT_DIM,
        );
    }

    /// Whether a keypress should be swallowed as a new binding.
    pub fn awaiting_key(&self) -> Option<crate::ui::keybinds::Action> {
        self.rebinding
    }

    /// Stops listening. Called once the caller has stored the key.
    pub fn finish_rebind(&mut self, bound: bool) {
        self.rebinding = None;
        self.notice = Some(if bound {
            (Notice::Translated(Msg::KeyBound), true)
        } else {
            (Notice::Translated(Msg::KeyCannotBind), false)
        });
    }

    /// Where the thumb controls stand, for this window and this
    /// arrangement.
    ///
    /// **The same type, from the same function, that the game plays
    /// with.** The editor could have drawn its own boxes from the
    /// placements and it would have been simpler and wrong: what a
    /// player is arranging is where the controls *end up*, and where
    /// they end up is decided by three rules that run after the
    /// placement -- the wheel, the lift clear of the hotbar, and
    /// yielding to the wheel. An editor that skipped them would show a
    /// button somewhere the game will not put it.
    ///
    /// The wheel is opened here whatever it is in play, because a
    /// control that is not on the glass is a control that cannot be
    /// arranged.
    fn placed_controls(&self, ui_scale: f32) -> crate::platform::touch::Layout {
        crate::platform::touch::Layout::for_size(
            crate::platform::Size {
                width: self.window_px.0,
                height: self.window_px.1,
            },
            self.arrangement,
            ui_scale,
            true,
        )
    }

    /// A finger has gone down somewhere that was not a button.
    ///
    /// Returns whether it landed on a control, so the caller knows
    /// whether the press was spent.
    pub fn grab_control(&mut self, at: (f32, f32), ui_scale: f32) -> bool {
        if !self.is_arranging() {
            return false;
        }
        let touch = self.placed_controls(ui_scale);
        // Into the pixels the controls actually live in. `hud` draws
        // them unscaled -- they are already the right physical size --
        // so the trip back is unscaled too, or the grab lands somewhere
        // the control is not.
        let (px, py) = widgets::ui_to_cursor(at, self.window_px, 1.0);
        let mut found = None;
        for (slot, button) in touch.buttons.iter().enumerate() {
            if button.shown && button.contains(px, py, 0.0) {
                found = Some((ControlUnderHand::Button(slot), button.centre));
            }
        }
        if found.is_none() && touch.stick.contains(px, py, 0.0) {
            found = Some((ControlUnderHand::Stick, touch.stick.centre));
        }
        let Some((which, centre)) = found else {
            return false;
        };
        // **Held where it was taken hold of, not by its middle.**
        // Without the offset a control snaps its centre under the
        // finger the instant it is touched, which reads as the button
        // being snatched rather than picked up.
        self.dragging = Some((which, (px - centre.0, py - centre.1)));
        true
    }

    /// The finger has moved while holding a control.
    pub fn drag_control(&mut self, at: (f32, f32), ui_scale: f32) {
        let Some((which, offset)) = self.dragging else {
            return;
        };
        let (px, py) = widgets::ui_to_cursor(at, self.window_px, 1.0);
        let wanted = (px - offset.0, py - offset.1);
        let size = crate::platform::Size {
            width: self.window_px.0,
            height: self.window_px.1,
        };
        let slot = match which {
            ControlUnderHand::Stick => &mut self.arrangement.stick,
            ControlUnderHand::Button(index) => &mut self.arrangement.buttons[index],
        };
        *slot = crate::platform::touch::placement_for(wanted, size, *slot);
        let _ = ui_scale;
    }

    /// The finger has come off.
    pub fn release_control(&mut self) {
        self.dragging = None;
        self.drag_scale = None;
    }

    /// The screen where the thumb controls are moved about.
    ///
    /// **Not a list, and that is why it is its own screen.** What it
    /// shows is the controls themselves, at the size and in the place
    /// they will be in play, over the same darkened backdrop every menu
    /// has -- so that arranging them is looking at the thing being
    /// arranged rather than at a row of numbers describing it.
    fn build_touch_controls(&mut self, p: &mut Painter, ctx: &MenuContext) {
        let layout = ctx.layout;
        self.backdrop(p, ctx);

        // Drawn by the call the HUD makes, from the layout the game
        // hit-tests. See `placed_controls`.
        let touch = self.placed_controls(ctx.settings.ui_scale);
        crate::ui::hud::touch_controls(p, &touch, |_| false, ctx.settings.language);

        const EDGE: f32 = 1.0;
        self.title(p, say(ctx, Msg::ArrangeControls), EDGE - 0.10);
        // One line, at the top where the controls are not. The bottom
        // corners belong to the thumbs and to the things being moved.
        p.text_centred(
            say(ctx, Msg::DragToArrange),
            0.0,
            EDGE - 0.24,
            layout.content() * 0.9,
            MENU.ink_dim,
        );

        let footer_height = layout.at(0.10).max(layout.finger());
        let footer_y0 = -EDGE + 0.06;
        let footer_y1 = footer_y0 + footer_height;
        let half = layout.panel_half_width(0.42);
        self.add_button(
            p,
            self.cursor,
            Rect::new(-half - 0.02, footer_y0, -0.02, footer_y1),
            say(ctx, Msg::ResetControls),
            Action::ResetTouchControls,
            true,
        );
        self.add_button(
            p,
            self.cursor,
            Rect::new(0.02, footer_y0, half + 0.02, footer_y1),
            say(ctx, Msg::Done),
            Action::Back,
            true,
        );
    }

    fn build_controls(&mut self, p: &mut Painter, cursor: Option<(f32, f32)>, ctx: &MenuContext) {
        // The settings screen's twin, and now literally so: the same
        // `ListRows`, the same scrollbar, the same wheel and arrow
        // keys. See it for the three kinds of number and why the row
        // count is what gives.
        //
        // **It used to squeeze instead.** Every binding was drawn,
        // always, and the row height was whatever eighteen of them
        // could afford between them -- so each new binding made every
        // row shorter, and on a phone's shape the key buttons came out
        // a third of a finger tall. In the player's words: "в меню
        // кнопок сделай список как в обычных настройках, а не сжимай
        // элементы."
        //
        // Reached on a phone only when somebody asks for it -- the
        // settings screen offers the thumb-controls editor there
        // instead -- but a screen that only *usually* cannot be reached
        // is a screen that eventually is, so the rows have a finger
        // floor like every other list here.
        use crate::ui::keybinds::Action as Bind;

        let layout = ctx.layout;
        self.backdrop(p, ctx);
        const EDGE: f32 = 1.0;
        let title_top = EDGE - 0.10;
        self.title(p, say(ctx, Msg::Controls), title_top);
        let help_top = -EDGE + 0.10;

        let footer_height = layout.at(0.10).max(layout.finger());
        let footer_y0 = help_top + 0.07;
        let footer_y1 = footer_y0 + footer_height;

        let half_width = layout.panel_half_width(1.05);
        let panel = Rect::new(-half_width, footer_y1 + 0.11, half_width, title_top - 0.14);
        p.panel(panel);

        let rows = ListRows::plan(layout, panel);
        let inset = rows.inset;
        let visible = rows.visible;
        self.controls_visible = visible;
        self.controls_scroll = self.clamp_controls_scroll(self.controls_scroll as i32);
        let first = self.controls_scroll;
        let count = Bind::ALL.len();

        // How much of a row the key button owns. One button rather than
        // the settings screen's pair, and wider than either of them: it
        // holds a key's *name*, and "L SHIFT" at the width of a plus
        // sign is a button that has to be guessed at.
        //
        // **Never more than its share of the row**, which it used to
        // be: at the top of the interface-size range on a square window
        // the asked-for width is wider than the panel, so the button
        // began off the left-hand edge of the glass -- half a control
        // that cannot be pressed, over a label it had covered. Three
        // fifths leaves the longest binding name room to be read
        // beside the longest key name.
        let edge = layout.at(0.02);
        let row_width = panel.width() - rows.pad * 2.0 - rows.gutter;
        let controls = layout
            .at(0.44)
            .max(layout.finger() + edge * 2.0)
            .min(row_width * 0.6);
        for (index, action) in Bind::ALL.iter().copied().skip(first).take(visible).enumerate() {
            // Drawn here and pressed here, from the one rect -- see
            // `ListRows::row`. Only the rows on screen get a hot rect
            // at all: one left behind for a row that has been scrolled
            // away is a click that rebinds something the player cannot
            // see.
            let row = rows.row(panel, index);
            p.well(row, MENU.row);
            p.label_left(row, action.label(ctx.settings.language), layout.at(0.025), layout.at(0.95), MENU.ink);

            let button = Rect::new(
                row.x1 - controls,
                row.y0 + inset,
                row.x1 - edge,
                row.y1 - inset,
            );
            let listening = self.rebinding == Some(action);
            let label = if listening {
                say(ctx, Msg::PressAKey).to_string()
            } else {
                ctx.settings.keybinds.label(action).to_string()
            };
            // An unbound action is worth pointing at: it is a real
            // state, reached by giving its key to something else, and
            // the only way to notice is to look here.
            let unbound = ctx.settings.keybinds.key(action).is_none();
            if unbound && !listening {
                p.well(button, MENU.field);
                p.border(button, 0.003, widgets::TEXT_BAD);
                p.label_in(button, "--", layout.at(0.95), widgets::TEXT_BAD);
                self.hot.push((button, Action::RebindKey(action)));
            } else if listening {
                self.add_pressed_button(
                    p,
                    button,
                    &label,
                    Action::RebindKey(action),
                    widgets::Press::Held,
                    true,
                );
            } else {
                self.add_button(p, cursor, button, &label, Action::RebindKey(action), true);
            }
        }

        // The scrollbar, in the lane `ListRows` kept for it, by the
        // same rule as every other list on this menu: drawn only when
        // there is something below the fold.
        p.scrollbar(rows.track(layout, panel), first, visible, count);

        if let Some((text, good)) = self.notice_line(ctx) {
            let colour = if good { widgets::TEXT_GOOD } else { widgets::TEXT_BAD };
            p.text_centred(&text, 0.0, panel.y0 - 0.06, 0.85, colour);
        }

        let footer_half = layout
            .at(0.60)
            .max(layout.finger() * 2.0)
            .min(half_width - 0.02);
        self.add_button(
            p,
            cursor,
            Rect::new(-footer_half - 0.02, footer_y0, -0.02, footer_y1),
            say(ctx, Msg::ResetToDefaults),
            Action::ResetKeys,
            true,
        );
        self.add_button(
            p,
            cursor,
            Rect::new(0.02, footer_y0, footer_half + 0.02, footer_y1),
            say(ctx, Msg::Done),
            Action::Back,
            true,
        );
        p.text_centred(
            say(ctx, Msg::ControlsHelp),
            0.0,
            help_top,
            0.75,
            widgets::TEXT_DIM,
        );
    }

    fn build_credits(&mut self, p: &mut Painter, cursor: Option<(f32, f32)>, ctx: &MenuContext) {
        let layout = ctx.layout;
        self.backdrop(p, ctx);
        // At the main menu's height, which is the screen this one is
        // opened from.
        self.title(p, say(ctx, Msg::Credits), 0.62);

        // A fixed number of rows, so unlike the lists this one grows by
        // however much all of them together can afford.
        // Air, the rows, the space the version line sits in, and the
        // button under the panel: what the screen comes to at a
        // desktop's size, which is what `within` needs.
        //
        // **The foot is 0.22, and it was 0.35.** Four credits and a
        // version number sat in a panel whose lower third was empty, and
        // an empty third of a panel reads as something that failed to
        // load. What the foot holds is one line of small print and its
        // air, and this is that.
        const FOOT: f32 = 0.22;
        let stack = 0.05 + 0.115 * CREDITS.len() as f32 + FOOT + 0.18 + 0.105;
        let panel_top = 0.40;
        let content = layout.within(panel_top + 1.0 - 0.03, stack);

        let row_height = content.at(0.115);
        let head = content.at(0.05);
        let foot = content.at(FOOT);
        let half_width = layout.panel_half_width(1.0);
        let panel = Rect::new(
            -half_width,
            panel_top - head - row_height * CREDITS.len() as f32 - foot,
            half_width,
            panel_top,
        );
        p.panel(panel);

        // Role on the left, who did it on the right. Two columns rather
        // than one line each, because the question a credits screen
        // answers is "who did the art", and a list of names does not
        // answer it.
        let pad = content.at(0.04);
        let mut y = panel.y1 - head - row_height;
        for (role, who) in CREDITS {
            let row = Rect::new(panel.x0 + pad, y, panel.x1 - pad, y + row_height);
            p.label_left(row, say(ctx, *role), content.at(0.02), content.at(0.9), MENU.ink_dim);
            let width = widgets::measure(who, content.at(1.1));
            p.label_left(
                Rect::new(row.x1 - width, row.y0, row.x1, row.y1),
                who,
                0.0,
                content.at(1.1),
                MENU.ink,
            );
            y -= row_height;
        }

        p.text_centred(ctx.version, 0.0, panel.y0 + content.at(0.09), content.at(0.8), MENU.ink_dim);

        let height = content.at(0.105).max(content.finger());
        // The middle third of the panel, as under every list. See `columns`.
        let (x0, x1) = columns((panel.x0, panel.x1), 3, layout.at(BUTTON_GAP), 1, 1);
        let centre_y = panel.y0 - content.at(0.18);
        self.add_button(
            p,
            cursor,
            // Centred rather than measured from its top edge: there is
            // a screen of room under this panel, so a button grown for a
            // finger can spread both ways without meeting anything.
            Rect::new(x0, centre_y - height / 2.0, x1, centre_y + height / 2.0),
            say(ctx, Msg::Back),
            Action::Back,
            true,
        );
    }

    /// What is extending the server, in two panes: the list on the left
    /// and everything one of them says about itself on the right.
    ///
    /// **Two panes rather than rows that expand.** A row that unfolds
    /// puts a mod's description, its authors and its settings into the
    /// list, which means every other row moves whenever one is opened --
    /// and the thing under the finger is then no longer the thing that
    /// was tapped. The split keeps the list still: choosing changes only
    /// the other half of the screen. It costs horizontal room, which is
    /// the axis this interface has most of -- `y` is always [-1, 1] and
    /// `x` grows with the window, so a phone held sideways has more of
    /// it than a 4:3 desktop does.
    ///
    /// **Nothing here is pressable except a row and BACK.** Settings are
    /// shown and not edited; see `protocol::ExtensionList` for why a
    /// client that could write them would be writing files on somebody
    /// else's machine.
    fn build_extensions(&mut self, p: &mut Painter, cursor: Option<(f32, f32)>, ctx: &MenuContext) {
        let layout = ctx.layout;
        self.backdrop(p, ctx);
        self.title(p, say(ctx, Msg::Extensions), 0.86);

        let help_top = -0.83;
        let button_height = layout.at(0.10).max(layout.finger());
        let back_y0 = help_top + layout.at(0.08);
        let half_width = layout.panel_half_width(0.95);
        let panel = Rect::new(
            -half_width,
            back_y0 + button_height + layout.at(0.08),
            half_width,
            0.66,
        );
        p.panel(panel);

        let pad = layout.at(0.03);
        let gap = layout.at(0.014);
        let items = self.extensions.items().len();

        // The list gets a little under half, because the names in it are
        // short and the prose beside it is not.
        let list_width = (panel.width() - pad * 3.0) * 0.42;
        let list = Rect::new(panel.x0 + pad, panel.y0 + pad, panel.x0 + pad + list_width, panel.y1 - pad);
        let detail = Rect::new(list.x1 + pad, panel.y0 + pad, panel.x1 - pad, panel.y1 - pad);

        if items == 0 {
            // The empty screen is the ordinary one, so it gets a whole
            // sentence rather than a dash. Which sentence depends on
            // something the list itself cannot say -- see `Extensions`.
            let msg = empty_extensions_line(&self.extensions, this_build_loads_mods());
            let scale = layout.content();
            let width = (panel.width() - pad * 2.0).max(0.1);
            let lines = widgets::wrap(say(ctx, msg), chars_that_fit(width, scale));
            let mut y = panel.centre_y() + widgets::line_height(scale) * lines.len() as f32 / 2.0;
            for line in &lines {
                p.text_centred(line, panel.centre_x(), y, scale, MENU.ink_dim);
                y -= widgets::line_height(scale);
            }
        } else {
            let row_height = layout.at(0.10).max(layout.finger());
            let gutter = layout.at(0.022);
            let visible = (((list.height() + gap + 0.002) / (row_height + gap)) as usize).max(1);
            self.extension_visible = visible;
            self.extension_scroll = self.clamp_extension_scroll(self.extension_scroll as i32);
            let first = self.extension_scroll;
            let mut y = list.y1 - row_height;

            for index in first..items.min(first + visible) {
                let rect = Rect::new(list.x0, y, list.x1 - gutter, y + row_height);
                let selected = index == self.extension_selected;
                let hovered = self.is_hovered(rect, cursor);
                p.well(rect, if selected { MENU.row_selected } else { MENU.row });
                if selected || hovered {
                    p.border(rect, 0.003, if selected { MENU.accent } else { MENU.dark });
                }
                let item = &self.extensions.items()[index];
                let name = item.name.clone();
                // The kind and the version on the quiet line: what a
                // player scanning the list wants is the names, and
                // "which loader was this" is the question they ask
                // second.
                let second = format!("{} {}", say(ctx, kind_msg(item.kind)), item.version);
                // The one row that is not running is marked on the row
                // rather than only in the pane beside it: a list where
                // the dead entry looks exactly like the live ones is a
                // list an operator has to click through to read.
                let live = item.enabled;
                p.row_labels(
                    rect,
                    layout.at(0.025),
                    &name,
                    if !live {
                        widgets::TEXT_BAD
                    } else if selected {
                        widgets::TEXT
                    } else {
                        MENU.ink_dim
                    },
                    second.trim(),
                    MENU.ink_dim,
                    layout.content(),
                );
                self.hot.push((rect, Action::SelectExtension(index)));
                y -= row_height + gap;
            }

            p.scrollbar(
                Rect::new(list.x1 - gutter + layout.at(0.006), list.y0, list.x1, list.y1),
                first,
                visible,
                items,
            );

            self.build_extension_detail(p, ctx, detail);
        }

        // The middle third of the panel, the column BACK stands in under
        // every list in the menu. See `columns`.
        let (x0, x1) = columns((panel.x0, panel.x1), 3, layout.at(BUTTON_GAP), 1, 1);
        self.add_button(
            p,
            cursor,
            Rect::new(x0, back_y0, x1, back_y0 + button_height),
            say(ctx, Msg::Back),
            Action::Back,
            true,
        );

        p.text_centred(
            say(ctx, crate::ui::lang::by_input(Msg::ExtensionsHelp, Msg::ExtensionsHelpTouch)),
            0.0,
            help_top,
            0.8,
            widgets::TEXT_DIM,
        );
    }

    /// Everything the highlighted extension says about itself.
    ///
    /// **Every line is bounded by the pane.** The description is prose
    /// somebody else wrote and the settings are a map somebody else
    /// filled in, so both are as long as they like; a pane that drew all
    /// of it would write over the button under it. Lines are laid out
    /// top-down and stop when the room does, which is the same rule the
    /// lists above use and the only one that cannot be defeated by a mod
    /// with a chatty manifest.
    fn build_extension_detail(&mut self, p: &mut Painter, ctx: &MenuContext, pane: Rect) {
        let Some(item) = self.extensions.items().get(self.extension_selected) else {
            return;
        };
        let layout = ctx.layout;
        let body = layout.content() * 0.85;
        let heading = layout.content();
        let width = pane.width().max(0.05);

        let mut y = pane.y1 - widgets::cell_height(heading);
        let floor = pane.y0;

        // The name, and beside it whether it is running. The state is
        // the one fact on this pane that decides whether the rest of it
        // matters.
        p.text(
            &widgets::fit(&item.name, heading, width * 0.66),
            pane.x0,
            y + widgets::cell_height(heading),
            heading,
            MENU.ink,
        );
        let state = say(ctx, if item.enabled { Msg::ExtensionOn } else { Msg::ExtensionOff });
        let state_width = widgets::measure(state, body);
        p.text(
            state,
            pane.x1 - state_width,
            y + widgets::cell_height(heading),
            body,
            if item.enabled { widgets::TEXT_GOOD } else { widgets::TEXT_BAD },
        );
        y -= widgets::line_height(body);

        // One quiet line of facts: which loader, what version, and --
        // for a native mod -- the contract it was built against, which
        // is the number an operator needs when it refuses to load.
        let mut facts = format!("{} {}", say(ctx, kind_msg(item.kind)), item.version);
        if let Some((major, minor)) = item.built_for {
            facts.push_str(&format!("   {} {major}.{minor}", say(ctx, Msg::ExtensionApi)));
        }
        if !item.reason.is_empty() {
            facts.push_str("   ");
            facts.push_str(&item.reason);
        }
        p.text(
            &widgets::fit(facts.trim(), body, width),
            pane.x0,
            y + widgets::cell_height(body),
            body,
            MENU.ink_dim,
        );
        y -= widgets::line_height(body) * 1.4;

        let columns = chars_that_fit(width, body);
        let line = |p: &mut Painter, y: &mut f32, text: &str, colour: [f32; 4]| -> bool {
            if *y < floor {
                return false;
            }
            p.text(
                &widgets::fit(text, body, width),
                pane.x0,
                *y + widgets::cell_height(body),
                body,
                colour,
            );
            *y -= widgets::line_height(body);
            true
        };

        if !item.description.is_empty() {
            for text in widgets::wrap(&item.description, columns) {
                if !line(p, &mut y, &text, MENU.ink) {
                    return;
                }
            }
            y -= widgets::line_height(body) * 0.4;
        }

        if !item.authors.is_empty() {
            // Joined here rather than on the server, which cannot know
            // how wide this pane is or which language it is being read
            // in -- see `ExtensionInfo::authors`.
            let who = format!("{}: {}", say(ctx, Msg::ExtensionBy), item.authors.join(", "));
            for text in widgets::wrap(&who, columns) {
                if !line(p, &mut y, &text, MENU.ink_dim) {
                    return;
                }
            }
            y -= widgets::line_height(body) * 0.4;
        }

        if item.settings.is_empty() {
            line(p, &mut y, say(ctx, Msg::ExtensionNoSettings), MENU.ink_dim);
            return;
        }
        if !line(p, &mut y, say(ctx, Msg::ExtensionSettings), MENU.accent) {
            return;
        }
        for (key, value) in &item.settings {
            if !line(p, &mut y, &format!("{key} = {value}"), MENU.ink) {
                return;
            }
        }
        y -= widgets::line_height(body) * 0.4;
        // Last, and only if there was room: the sentence explaining why
        // none of the above can be touched. It is the least important
        // thing on the pane and the first thing to be squeezed out.
        line(p, &mut y, say(ctx, Msg::ExtensionsReadOnly), MENU.ink_dim);
    }

    #[allow(clippy::too_many_arguments)] // a screen, its cursor and its text
    fn build_confirm(
        &mut self,
        p: &mut Painter,
        cursor: Option<(f32, f32)>,
        ctx: &MenuContext,
        question: Msg,
        detail: &str,
        confirm_label: Msg,
        action: Action,
    ) {
        let layout = ctx.layout;
        self.backdrop(p, ctx);
        self.title(p, say(ctx, question), 0.42);
        // A question, a warning and two answers: the emptiest screen in
        // the game, and the one the old single cap held to five percent
        // because the settings panel could not grow.
        let content = layout.within(0.24 + 0.60, 0.60);
        if !detail.is_empty() {
            p.text_centred(detail, 0.0, 0.18, content.at(1.2), MENU.ink);
        }
        p.text_centred(
            say(ctx, Msg::CannotBeUndone),
            0.0,
            0.02,
            content.at(0.9),
            widgets::TEXT_BAD,
        );

        let height = content.at(0.10).max(content.finger());
        let (split, width) = side_by_side(layout, content.at(0.32), content.at(0.56));
        let button_y = 0.02 - content.at(0.30);
        // Cancel first and on the left, where the eye lands: the safe
        // answer should be the easy one to hit.
        self.add_button(
            p,
            cursor,
            Rect::centred(-split, button_y, width, height),
            say(ctx, Msg::Cancel),
            Action::Cancel,
            true,
        );
        self.add_button(
            p,
            cursor,
            Rect::centred(split, button_y, width, height),
            say(ctx, confirm_label),
            action,
            true,
        );
        p.text_centred(
            say(ctx, crate::ui::lang::by_input(Msg::ConfirmHelp, Msg::ConfirmHelpTouch)),
            0.0,
            button_y - height / 2.0 - content.at(0.13),
            0.8,
            widgets::TEXT_DIM,
        );
    }

    /// The backdrop every full-screen menu starts with.
    ///
    /// One quad either way, and that is worth saying: the menu does not
    /// draw the world behind it. The renderer does, before the
    /// interface is drawn at all -- this only decides how hard the
    /// interface pushes it back. With a scene there the veil is lighter
    /// than the pause menu's, because there is something behind it
    /// worth being able to see; without one it is nearly opaque,
    /// because what is behind it is either a paused world or an empty
    /// sky and neither is worth a legibility cost.
    fn backdrop(&self, p: &mut Painter, ctx: &MenuContext) {
        match ctx.background {
            Backdrop::Scene(place) => p.scrim(veil_for(place)),
            Backdrop::Bare => p.scrim(MENU_SCRIM),
        }
    }

    fn is_hovered(&self, rect: Rect, cursor: Option<(f32, f32)>) -> bool {
        cursor.is_some_and(|(x, y)| rect.contains(x, y))
    }

    fn add_button(
        &mut self,
        p: &mut Painter,
        cursor: Option<(f32, f32)>,
        rect: Rect,
        label: &str,
        action: Action,
        enabled: bool,
    ) {
        let hovered = enabled && self.is_hovered(rect, cursor);
        let press = if hovered { widgets::Press::Hovered } else { widgets::Press::Idle };
        self.add_pressed_button(p, rect, label, action, press, enabled);
    }

    /// The same, for a button the screen knows is *engaged* rather than
    /// merely pointed at.
    ///
    /// One screen has such a button: the key being rebound, which has
    /// been clicked and is now waiting for a key (`self.rebinding`).
    /// It used to be an ordinary raised button whose label had changed
    /// to PRESS A KEY -- so the only thing saying the interface was
    /// waiting for you was a line of text three words long, on a
    /// control that still looked exactly as pressable as the eleven
    /// under it. Held, its bevel turns over and it reads as a key held
    /// down, which is the thing that is actually happening.
    fn add_pressed_button(
        &mut self,
        p: &mut Painter,
        rect: Rect,
        label: &str,
        action: Action,
        press: widgets::Press,
        enabled: bool,
    ) {
        p.pressable(rect, label, press, enabled);
        if enabled {
            self.hot.push((rect, action));
        }
    }

    /// A button on one of the arrow-key-navigable screens: highlighted
    /// by the mouse or by the keyboard focus, whichever is active.
    fn add_menu_button(
        &mut self,
        p: &mut Painter,
        cursor: Option<(f32, f32)>,
        rect: Rect,
        label: &str,
        action: Action,
        index: usize,
    ) {
        let highlighted =
            self.is_hovered(rect, cursor) || self.button_focus == Some(index);
        p.button(rect, label, highlighted, true);
        self.hot.push((rect, action));
    }

    /// A choice from a short list, stepped rather than typed: `<` on the
    /// left, `>` on the right, and what is chosen written between them.
    ///
    /// ## What it was, and the bug in it
    ///
    /// A text field with one small `<` in its right-hand end, and the
    /// whole row stepping forward. Two things were wrong with that, and
    /// the second is not a matter of taste:
    ///
    /// * **It looked like a field.** A well with a word at its left edge
    ///   is exactly what NAME and SEED are, on the same form, one row up.
    ///   A choice has to show that it has a way forward and a way back,
    ///   and a word centred between two arrows is the shape for that.
    /// * **The `<` stepped forward.** A click goes to the *first* target
    ///   that contains it (`Menu::hovered`), and the row was pushed before
    ///   the button inside it -- so a mouse on `<` answered the row, and
    ///   the only control for going back went on instead. The arrows are
    ///   pushed first now and the middle no longer overlaps them; see
    ///   `a_choice_on_the_world_form_steps_the_way_its_arrow_points`.
    ///
    /// The word between the arrows still steps forward, because clicking
    /// the thing being read is what everybody tries first.
    ///
    /// Rejected: a dropdown. It is a second layer of state for lists of
    /// three and five, and on a phone it would open over the rows under
    /// it, which on this form are the rows a thumb is aiming at next.
    #[allow(clippy::too_many_arguments)] // a box, its word, and its two directions
    fn stepper(
        &mut self,
        p: &mut Painter,
        cursor: Option<(f32, f32)>,
        layout: widgets::Layout,
        rect: Rect,
        value: &str,
        back: Action,
        forward: Action,
    ) {
        // A finger wide, not a tenth of a row: the arrows are the smallest
        // things on the screen. Never more than a third of the row each,
        // or on a squeezed window the word has nowhere to go.
        let arrow = layout.at(0.10).max(layout.finger()).min(rect.width() / 3.0);
        let left = Rect::new(rect.x0, rect.y0, rect.x0 + arrow, rect.y1);
        let right = Rect::new(rect.x1 - arrow, rect.y0, rect.x1, rect.y1);
        let middle = Rect::new(left.x1, rect.y0, right.x0, rect.y1);

        p.well(middle, MENU.field);
        if self.is_hovered(middle, cursor) {
            // The same quiet frame a hovered list row gets, in the lit
            // edge rather than the dark one: this is a dark well, and a
            // dark line on it is no line.
            p.border(middle, 0.003, MENU.light);
        }
        // Written the size a field on this row would write it, and fitted
        // to the room between the arrows rather than trusted to be short.
        let wanted = widgets::field_metrics(middle, layout.content(), value, value.len()).scale;
        let scale = widgets::fitted_scale(value, wanted, (middle.width() - 0.036).max(0.0), 0.7);
        p.label_in(middle, value, scale, MENU.ink);

        // The arrows first: see above for what the other order did.
        self.add_button(p, cursor, left, "<", back, true);
        self.add_button(p, cursor, right, ">", forward.clone(), true);
        self.hot.push((middle, forward));
    }

    /// Where the pause screen's title is written.
    ///
    /// Named because two things measure from it: the title, and the key
    /// to the gauges written in the band under it. They used to share a
    /// bare `0.52` written twice, forty lines apart. It is also higher
    /// than it was, at the main menu's own height: at 0.52 that band came
    /// to a hundredth of a screen more than the legend's own letters, so
    /// the key read as a line squeezed between the title and RESUME
    /// rather than as a caption with room of its own.
    const PAUSE_TITLE_TOP: f32 = 0.62;

    /// The game's name over the wallpaper.
    ///
    /// A *light* gold, unlike the one headings on stone are printed in:
    /// this one is over the world, and the two are the same colour only
    /// in the sense that ink and paint are.
    const TITLE_GOLD: [f32; 4] = [1.0, 0.82, 0.34, 1.0];

    fn title(&self, p: &mut Painter, text: &str, y: f32) {
        p.text_centred(text, 0.0, y, 3.0, Self::TITLE_GOLD);
    }

    fn build_main(&mut self, p: &mut Painter, cursor: Option<(f32, f32)>, ctx: &MenuContext) {
        // A column of five buttons with a lot of air round it -- so
        // unlike the forms, this one has room and can take most of what
        // is asked for. The title, the subtitle and the version line
        // stay the size they are: they are read once and never pressed.
        let layout = ctx.layout;
        self.backdrop(p, ctx);
        self.title(p, "PRIMITIVE", 0.62);
        p.text_centred(say(ctx, Msg::Subtitle), 0.0, 0.44, 1.0, widgets::TEXT_DIM);

        // The band between the subtitle and the version line, and what
        // five buttons and four gaps come to inside it. See
        // `Layout::within`.
        const TOP: f32 = 0.2525;
        const STACK: f32 = 0.105 * 5.0 + 0.03 * 4.0;
        let content = layout.within(TOP + 0.75, STACK);

        let width = layout.panel_half_width(0.45) * 2.0;
        let height = content.at(0.105).max(content.finger());
        let pitch = height + content.at(0.03);
        // Measured from the top of the first button rather than from its
        // centre, so a taller column grows downward into the air it has
        // rather than up through the subtitle.
        let mut y = TOP - height / 2.0;
        for (index, (label, action)) in [
            (say(ctx, Msg::Singleplayer), Action::OpenWorlds),
            (say(ctx, Msg::Multiplayer), Action::OpenServers),
            (say(ctx, Msg::Settings), Action::OpenSettings),
            (say(ctx, Msg::Credits), Action::OpenCredits),
            (say(ctx, Msg::Quit), Action::Quit),
        ]
        .into_iter()
        .enumerate()
        {
            let rect = Rect::centred(0.0, y, width, height);
            self.add_menu_button(p, cursor, rect, label, action, index);
            y -= pitch;
        }

        p.text_centred(ctx.version, 0.0, -0.82, 0.8, widgets::TEXT_DIM);
    }

    fn build_servers(&mut self, p: &mut Painter, cursor: Option<(f32, f32)>, ctx: &MenuContext) {
        // The world list's twin -- see `build_worlds` for why it is
        // built from the bottom of the glass upward.
        let layout = ctx.layout;
        self.backdrop(p, ctx);
        self.title(p, say(ctx, Msg::Servers), 0.86);

        let help_top = -0.83;
        let button_height = layout.at(0.10).max(layout.finger());
        let back_y0 = help_top + layout.at(0.08);
        let row_y0 = back_y0 + button_height + layout.at(0.08);
        let notice_y = row_y0 + button_height + layout.at(0.11);

        let half_width = layout.panel_half_width(0.95);
        let panel = Rect::new(-half_width, notice_y + layout.at(0.06), half_width, 0.66);
        p.panel(panel);

        if self.servers.servers.is_empty() {
            p.text_centred(
                say(ctx, crate::ui::lang::by_input(Msg::NoServersYet, Msg::NoServersYetTouch)),
                panel.centre_x(),
                panel.y1 - layout.at(0.41).min(panel.height() * 0.43),
                layout.content(),
                MENU.ink_dim,
            );
        }

        let pad = layout.at(0.03);
        let gap = layout.at(0.014);
        let row_height = layout.at(0.11).max(layout.finger());
        let mut y = panel.y1 - pad - row_height;
        let visible = (((panel.height() - pad * 2.0 + gap + 0.002) / (row_height + gap)) as usize)
            .max(1);
        // **Scrolled by an offset, like the world list.** It used to
        // derive the first visible row from the selection, which meant
        // the wheel could not move it and a thumb dragged down it did
        // nothing at all -- the list simply refused to go anywhere until
        // the selection was walked with the arrow keys, which a phone
        // has not got.
        self.server_visible = visible;
        self.server_scroll = self.clamp_server_scroll(self.server_scroll as i32);
        let first = self.server_scroll;
        let gutter = layout.at(0.022);

        for index in first..self.servers.servers.len().min(first + visible) {
            let entry = &self.servers.servers[index];
            let rect = Rect::new(panel.x0 + pad, y, panel.x1 - pad - gutter, y + row_height);
            let selected = index == self.selected;
            let hovered = self.is_hovered(rect, cursor);

            p.well(rect, if selected { MENU.row_selected } else { MENU.row });
            if selected || hovered {
                p.border(rect, 0.003, if selected { MENU.accent } else { MENU.dark });
            }
            p.row_labels(
                rect,
                layout.at(0.025),
                &entry.name,
                if selected { widgets::TEXT } else { MENU.ink_dim },
                &entry.address,
                MENU.ink_dim,
                layout.content(),
            );

            // A row both selects and, on the already-selected row,
            // connects -- so a second click plays.
            let action = if selected {
                Action::Connect(index)
            } else {
                Action::Select(index)
            };
            self.hot.push((rect, action));
            y -= row_height + gap;
        }

        p.scrollbar(
            Rect::new(
                panel.x1 - pad - gutter + layout.at(0.006),
                panel.y0 + pad,
                panel.x1 - pad,
                panel.y1 - pad,
            ),
            first,
            visible,
            self.servers.servers.len(),
        );

        if let Some((text, good)) = self.notice_line(ctx) {
            let colour = if good { widgets::TEXT_GOOD } else { widgets::TEXT_BAD };
            p.text_centred(&text, 0.0, notice_y, layout.at(0.9), colour);
        }

        let any = !self.servers.servers.is_empty();
        let selected = self.selected;
        // Four across, cut from the panel -- see the world list -- and
        // BACK under the middle two, which is where the middle of a row of
        // four is.
        let span = (panel.x0, panel.x1);
        let column_gap = layout.at(BUTTON_GAP);
        for (column, label, action, enabled) in [
            (0, say(ctx, Msg::Play), Action::Connect(selected), any),
            (1, say(ctx, Msg::Add), Action::Add, true),
            (2, say(ctx, Msg::Edit), Action::Edit(selected), any),
            (3, say(ctx, Msg::Delete), Action::Delete(selected), any),
        ] {
            let (x0, x1) = columns(span, 4, column_gap, column, column);
            self.add_button(p, cursor, Rect::new(x0, row_y0, x1, row_y0 + button_height), label, action, enabled);
        }
        let (x0, x1) = columns(span, 4, column_gap, 1, 2);
        self.add_button(
            p,
            cursor,
            Rect::new(x0, back_y0, x1, back_y0 + button_height),
            say(ctx, Msg::Back),
            Action::Back,
            true,
        );

        p.text_centred(
            say(ctx, crate::ui::lang::by_input(Msg::ServersHelp, Msg::ServersHelpTouch)),
            0.0,
            help_top,
            0.8,
            widgets::TEXT_DIM,
        );
    }

    fn build_form(&mut self, p: &mut Painter, cursor: Option<(f32, f32)>, ctx: &MenuContext, editing: bool) {
        // The create-world form one row shorter -- see it, and see
        // `FormRows`, for why the width and the height get different
        // answers and why a phone gets a different arrangement.
        let layout = ctx.layout;
        self.backdrop(p, ctx);
        let rows = FormRows::plan(layout, 2, (0.44, 0.32, 0.14, 0.12));
        if layout.keyboard_top().is_none() {
            self.title(p, say(ctx, if editing { Msg::EditServer } else { Msg::AddServer }), 0.70);
        }

        let half_width = layout.panel_half_width(0.95);
        let panel = Rect::new(-half_width, rows.panel_bottom(2), half_width, rows.top);
        p.panel(panel);

        let pad = layout.at(0.05);
        for (index, msg) in [Msg::Name, Msg::Address].into_iter().enumerate() {
            let (label_x, label_y, rect) = rows.row(panel, pad, index);
            p.text(say(ctx, msg), label_x, label_y, rows.label_size, MENU.ink_dim);
            let (text, field, action, hint) = if index == 0 {
                (
                    &self.name_input,
                    Field::Name,
                    Action::Focus(Field::Name),
                    Msg::ServerNamePlaceholder,
                )
            } else {
                (
                    &self.address_input,
                    Field::Address,
                    Action::Focus(Field::Address),
                    Msg::AddressPlaceholder,
                )
            };
            p.text_field(
                rect,
                text,
                say(ctx, hint),
                self.focus == field,
                self.caret_visible(),
            );
            self.hot.push((rect, action));
            self.field_boxes.push((rect, field));
        }

        let content = layout.within(panel.y0 + 1.0 - 0.03, 0.44);
        let notice_y = panel.y0 - content.at(0.08);
        if let Some((text, good)) = self.notice_line(ctx) {
            let colour = if good { widgets::TEXT_GOOD } else { widgets::TEXT_BAD };
            p.text_centred(&text, 0.0, notice_y, content.at(0.9), colour);
        } else {
            p.text_centred(
                say(ctx, Msg::AddressHelp),
                0.0,
                notice_y,
                content.at(0.8),
                widgets::TEXT_DIM,
            );
        }

        let button_height = content.at(0.10).max(content.finger());
        let button_y = notice_y - content.at(0.13) - button_height / 2.0;
        // The two halves of the panel, as on the world form.
        let span = (panel.x0, panel.x1);
        let column_gap = layout.at(BUTTON_GAP);
        let (y0, y1) = (button_y - button_height / 2.0, button_y + button_height / 2.0);
        let (x0, x1) = columns(span, 2, column_gap, 0, 0);
        self.add_button(p, cursor, Rect::new(x0, y0, x1, y1), say(ctx, Msg::Save), Action::Save, true);
        let (x0, x1) = columns(span, 2, column_gap, 1, 1);
        self.add_button(p, cursor, Rect::new(x0, y0, x1, y1), say(ctx, Msg::Cancel), Action::Cancel, true);

        p.text_centred(
            say(ctx, crate::ui::lang::by_input(Msg::ServerFormHelp, Msg::ServerFormHelpTouch)),
            0.0,
            button_y - button_height / 2.0 - content.at(0.13),
            0.8,
            widgets::TEXT_DIM,
        );
    }

    fn build_connecting(&mut self, p: &mut Painter, cursor: Option<(f32, f32)>, ctx: &MenuContext, label: &str) {
        // Three things on an otherwise empty screen, so `within` gives
        // it very nearly everything it asks for.
        let layout = ctx.layout;
        self.backdrop(p, ctx);
        self.title(p, say(ctx, Msg::Connecting), 0.42);
        let content = layout.within(0.20 + 0.70, 0.45);
        p.text_centred(label, 0.0, 0.16, content.at(1.2), MENU.ink);
        let height = content.at(0.10).max(content.finger());
        self.add_button(
            p,
            cursor,
            Rect::centred(0.0, -0.20, content.at(0.6).min(layout.edge() * 2.0), height),
            say(ctx, Msg::Cancel),
            Action::Cancel,
            true,
        );
    }

    fn build_failed(
        &mut self,
        p: &mut Painter,
        cursor: Option<(f32, f32)>,
        ctx: &MenuContext,
        label: &str,
        reason: &str,
    ) {
        let layout = ctx.layout;
        self.backdrop(p, ctx);
        self.title(p, say(ctx, Msg::CannotConnect), 0.62);
        // The reason can run to several lines, so what this screen
        // stacks is not a constant. Measured before anything is drawn,
        // or a long error grows the text and then finds the buttons
        // already off the bottom.
        let lines = widgets::wrap(reason, 52);
        let stack = 0.18 + widgets::line_height(0.9) * lines.len() as f32 + 0.10;
        let content = layout.within(0.40 + 0.70, stack);

        p.text_centred(label, 0.0, 0.38, content.at(1.2), MENU.ink);

        // The reason is shown in full, wrapped. A truncated network
        // error tells the player nothing about what to fix.
        let mut y = 0.20;
        for line in lines {
            p.text_centred(&line, 0.0, y, content.at(0.9), widgets::TEXT_BAD);
            y -= widgets::line_height(content.at(0.9));
        }

        let height = content.at(0.10).max(content.finger());
        let (split, width) = side_by_side(layout, content.at(0.28), content.at(0.5));
        // Below whatever the reason came to, rather than at a fixed
        // -0.36: a five-line error used to be drawn straight through
        // them.
        let button_y = (y - content.at(0.10) - height / 2.0).min(-0.36);
        self.add_button(
            p,
            cursor,
            Rect::centred(-split, button_y, width, height),
            say(ctx, Msg::Retry),
            Action::Retry,
            true,
        );
        self.add_button(
            p,
            cursor,
            Rect::centred(split, button_y, width, height),
            say(ctx, Msg::Back),
            Action::Back,
            true,
        );
    }

    fn build_paused(&mut self, p: &mut Painter, cursor: Option<(f32, f32)>, ctx: &MenuContext) {
        // Dimmed, not covered: the pause screen sits over the world, and
        // seeing where you left off is half of what makes it read as a
        // pause rather than a disconnect.
        let layout = ctx.layout;
        p.scrim([0.02, 0.03, 0.05, 0.62]);
        self.title(p, say(ctx, Msg::Paused), Self::PAUSE_TITLE_TOP);
        self.gauge_legend(p, ctx);

        // Nothing under the column, so it has the rest of the screen --
        // see `build_main`, which is the same shape with a version line
        // in the way.
        const TOP: f32 = 0.2525;
        const STACK: f32 = 0.105 * 5.0 + 0.03 * 4.0;
        let content = layout.within(TOP + 0.95, STACK);

        let width = layout.panel_half_width(0.45) * 2.0;
        let height = content.at(0.105).max(content.finger());
        let pitch = height + content.at(0.03);
        let mut y = TOP - height / 2.0;
        for (index, (label, action)) in [
            (say(ctx, Msg::Resume), Action::Resume),
            (say(ctx, Msg::Settings), Action::OpenSettings),
            // Here and not on the main menu: the list is a fact about a
            // server, and the main menu is the screen with no server
            // behind it. See `Screen::Extensions`.
            (say(ctx, Msg::Extensions), Action::OpenExtensions),
            (say(ctx, Msg::LeaveWorld), Action::LeaveWorld),
            (say(ctx, Msg::Quit), Action::Quit),
        ]
        .into_iter()
        .enumerate()
        {
            let rect = Rect::centred(0.0, y, width, height);
            self.add_menu_button(p, cursor, rect, label, action, index);
            y -= pitch;
        }
    }

    /// The key to the strips over the hotbar.
    ///
    /// **The complaint: "что за полоска под температурой".** The player
    /// was looking at the water bar. The gauges now carry a mark each
    /// (see `hud::GAUGE_LEGEND`), which answers it for a heart and a
    /// drop and does not for a bolt or a thermometer -- so the six are
    /// named here, once, with the same marks beside them and **in the
    /// order they are stacked on screen**.
    ///
    /// ## Why on the pause screen and nowhere else
    ///
    /// Because it is where somebody who does not understand what they
    /// are looking at already goes, and because it is the only screen
    /// over the world that costs nothing to put this on: the HUD itself
    /// must stay silent -- it fades out entirely when nothing is wrong
    /// -- and printing four words beside four strips for the whole of a
    /// session is the thing that design exists to avoid.
    ///
    /// ## Why in the band between the title and the buttons
    ///
    /// It is the one part of this screen whose height does not move.
    /// The button column's top edge is `TOP` at every interface size
    /// (the buttons grow downward from it) and the title's baseline is
    /// fixed, so the gap between them is the same band whatever the
    /// player has asked for. Under the buttons would have been nearer
    /// the bars it explains and is not available: at a large interface
    /// size the column already reaches down into the gauges.
    fn gauge_legend(&self, p: &mut Painter, ctx: &MenuContext) {
        use crate::ui::hud;
        let layout = ctx.layout;
        let items = hud::GAUGE_LEGEND;
        let title = say(ctx, Msg::GaugesTitle);

        // Every part of one entry, in units of the scale it is drawn
        // at, so the width is linear in the scale and the fit below is
        // a division rather than a search.
        const ICON: f32 = 0.040;
        const AFTER_ICON: f32 = 0.014;
        const BETWEEN: f32 = 0.038;
        let unit_width = {
            let names: f32 = items
                .iter()
                .map(|gauge| widgets::measure(say(ctx, gauge.name), 1.0))
                .sum();
            widgets::measure(title, 1.0)
                + BETWEEN
                + names
                + (ICON + AFTER_ICON) * items.len() as f32
                + BETWEEN * items.len() as f32
        };

        // As big as the interface size asks for, and no wider than the
        // glass. The row is one line of small print: it has no second
        // line to wrap onto, so the only thing that can give is the
        // size -- and it gives all the way down rather than clipping,
        // because a key with the last two entries cut off is a key that
        // has stopped answering the question it exists for.
        const BUTTONS_TOP: f32 = 0.2525;
        let title_bottom = Self::PAUSE_TITLE_TOP - widgets::cell_height(3.0);
        let centre_y = (title_bottom + BUTTONS_TOP) / 2.0;
        let band = title_bottom - BUTTONS_TOP;

        let wanted = layout.content() * 1.25;
        let room = layout.edge() * 2.0 * 0.94;
        // Three caps, and the smallest wins: what the player asked for,
        // what the glass is wide enough for, and what the band is tall
        // enough for. The third exists because a wide window lets the
        // second one off entirely -- on a 22:9 phone there is width to
        // spare and no height at all, and a row lettered to fill the
        // width would be printed through the title above it.
        let scale = wanted
            .min(room / unit_width.max(1e-4))
            .min(band * 0.62 / widgets::cell_height(1.0).max(ICON));

        let cap = widgets::cell_height(scale);
        let mut x = -unit_width * scale / 2.0;
        p.text(title, x, centre_y + cap / 2.0, scale, MENU.ink_dim);
        x += (widgets::measure(title, 1.0) + BETWEEN) * scale;

        for gauge in items {
            let side = ICON * scale;
            // **Each mark in its own meter's colour**, which turns a
            // list of names into a key. The marks used to be drawn in
            // one pale ink, so a player who had worked out that the
            // violet strip was the one they wanted still had to count
            // strips to find it; now the violet moon in this row *is*
            // the violet strip over the hotbar. It costs nothing -- the
            // shadow underneath is what makes a mark legible over a
            // world, and it is unchanged.
            hud::draw_icon_inked(p, gauge.icon, (x + side / 2.0, centre_y), side, gauge.ink);
            x += (ICON + AFTER_ICON) * scale;
            let name = say(ctx, gauge.name);
            p.text(name, x, centre_y + cap / 2.0, scale, MENU.ink);
            x += (widgets::measure(name, 1.0) + BETWEEN) * scale;
        }
    }
}

/// Who made what, in the order the screen shows them.
///
/// A table rather than a formatted // of text: the screen lays it out
/// in two columns, and a credit whose role is not spelled out is not
/// really a credit.
pub const CREDITS: &[(Msg, &str)] = &[
    (Msg::RoleTextures, "NYukichi.I"),
    (Msg::RoleCode, "Claude (Anthropic)"),
    (Msg::RoleCode, "George Perry Floyd Jr"),
    (Msg::RoleEngine, "Rust, wgpu, tokio"),
];

/// What stands behind a full-screen menu.
///
/// **Two states rather than a texture layer**, which is what this
/// carried when the backdrop was wallpaper. The menu used to *draw* its
/// own background -- a grid of tiled quads in the interface's own
/// vertex buffer. It does not any more: the world behind it is real
/// geometry drawn by the renderer in the pass before the interface, so
/// all the interface has left to decide is how hard to push it back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backdrop {
    /// Nothing of the menu's own: a paused world, or an empty sky.
    Bare,
    /// A generated patch of world -- see `logic::menu_scene`. Which
    /// place it is decides how hard the veil pushes: a cave and a
    /// sunset are two hundredths and two thirds of a screen of light
    /// respectively, and one veil for both is one of them ruined.
    Scene(Place),
}

/// What the menus need to read in order to draw themselves.
///
/// Borrowed per frame rather than owned, because the settings and the
/// world list are owned by `main.rs` -- the menu shows them and reports
/// what the player asked for, but never mutates them behind its back.
pub struct MenuContext<'a> {
    pub version: &'a str,
    /// Where the font lives in the texture array.
    pub font: crate::engine::texture::FontAtlas,
    pub settings: &'a ClientSettings,
    pub worlds: &'a Worlds,
    /// Whether there is a generated world behind this screen.
    pub background: Backdrop,
    /// What shape of screen this is being drawn on, and how big the
    /// player asked for it.
    ///
    /// **The menu is the one part of the interface that lays itself
    /// out** rather than being drawn for a desktop and multiplied
    /// afterwards, and this is what it lays itself out against. See
    /// `widgets::Layout` for why a phone needs the difference.
    pub layout: widgets::Layout,
}

/// The subset of the keyboard the menus care about.
///
/// **The editing keys carry their modifiers rather than arriving as
/// separate variants.** Shift and control turn four keys into twelve --
/// left, word-left, select-left, select-word-left and the same going
/// right -- and twelve variants is a match arm nobody keeps in step. A
/// flag on the key is the same information in the shape the field
/// actually consumes it: see `ui::field::TextField::move_caret`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Up,
    Down,
    Enter,
    Escape,
    Tab,
    /// `word` is control held: a whole word rather than a character.
    Backspace { word: bool },
    Delete { word: bool },
    /// `extend` is shift held: the selection grows rather than the
    /// caret jumping.
    Left { word: bool, extend: bool },
    Right { word: bool, extend: bool },
    Home { extend: bool },
    End { extend: bool },
    SelectAll,
    Char(char),
}

/// One edit of whichever field has focus.
///
/// Its own vocabulary rather than [`Key`] passed straight through,
/// because a key is a thing on a keyboard and an edit is a thing that
/// happens to text -- and exactly one of those two is what a phone,
/// which has neither Home nor control, would ever be able to send.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Edit {
    Backspace { word: bool },
    Delete { word: bool },
    Move { motion: Motion, extend: bool },
    SelectAll,
}

/// What a key does to the text in a field, or nothing when it is not an
/// editing key at all.
///
/// A free function rather than a method, so the two screens that have
/// fields share one answer -- which is the whole of the fix for a
/// settings row that only knew about Backspace.
fn edit_for(key: Key) -> Option<Edit> {
    Some(match key {
        Key::Backspace { word } => Edit::Backspace { word },
        Key::Delete { word } => Edit::Delete { word },
        Key::Left { word: false, extend } => Edit::Move { motion: Motion::Left, extend },
        Key::Left { word: true, extend } => Edit::Move { motion: Motion::WordLeft, extend },
        Key::Right { word: false, extend } => Edit::Move { motion: Motion::Right, extend },
        Key::Right { word: true, extend } => Edit::Move { motion: Motion::WordRight, extend },
        Key::Home { extend } => Edit::Move { motion: Motion::Home, extend },
        Key::End { extend } => Edit::Move { motion: Motion::End, extend },
        Key::SelectAll => Edit::SelectAll,
        _ => return None,
    })
}

/// How tall one row of the key-bindings screen is, on a desktop.
///
/// **A constant now, because the list scrolls.** It used to be a
/// division: the panel's height shared out between however many
/// bindings there were, clamped to a floor. Every binding added made
/// every row shorter, so the screen got quietly worse each time the
/// game gained a key -- and once the division hit its floor the rows
/// stopped shrinking and started running off the bottom of the panel
/// instead, which is a key nobody can rebind with nothing on screen
/// saying so. Between those two failures there was a band where it
/// merely looked bad: eighteen rows a third of a finger tall, each
/// with a key button thinner still.
///
/// None of that is a row's problem. **A row's height is not the
/// variable -- how many are on screen is**, which is what a window onto
/// the list gives you and what the settings screen has always had. So
/// this is `ListRows::HEIGHT`: the one row height every list on this
/// menu is drawn at.
///
/// Only the test in `keybinds` reads it now, which is the other half of
/// the same change: the screen no longer needs to be told how tall a
/// row is, because `ListRows` decides that for every list here. What is
/// left is somewhere for that test to check the number against without
/// writing it out a second time.
#[cfg(test)]
pub fn controls_row_height() -> f32 {
    ListRows::HEIGHT
}


#[cfg(test)]
mod tests {

    /// A control dragged with a finger ends up under the finger.
    ///
    /// **The whole of what the arrangement screen has to get right.**
    /// It moves a control by writing an arrangement, and the game then
    /// draws the control from that arrangement, through three rules
    /// that run afterwards. If the two ends disagree, the button slides
    /// out from under the thumb the moment it is released -- and a
    /// player aiming a button at their thumb would be chasing it around
    /// the glass.
    #[test]
    fn a_control_dragged_across_the_glass_ends_up_where_it_was_dropped() {
        let mut menu = Menu::new(ServerList::default());
        menu.set_screen_size(2712, 1220);
        menu.begin_arranging(crate::settings::TouchLayout::default());
        menu.open(Screen::TouchControls);
        assert!(menu.is_arranging());

        // Where the jump button starts, in interface space.
        let before = menu.placed_controls(1.5);
        let slot = before
            .buttons
            .iter()
            .position(|button| button.shown)
            .expect("some button is on the glass to move");
        let from = before.buttons[slot].centre;
        let grab = widgets::cursor_to_ui((from.0 as f64, from.1 as f64), (2712, 1220), 1.0);

        menu.set_cursor(Some(grab));
        assert!(
            menu.grab_at_cursor(1.5),
            "the finger landed on a control and picked nothing up",
        );

        // Carried a long way across the glass -- far enough to change
        // which corner it is measured from, which is the seam.
        let to_px = (2712.0 * 0.25, 1220.0 * 0.30);
        let to = widgets::cursor_to_ui((to_px.0 as f64, to_px.1 as f64), (2712, 1220), 1.0);
        menu.set_cursor(Some(to));
        menu.release_control();

        let after = menu.placed_controls(1.5);
        let landed = after.buttons[slot].centre;
        assert!(
            (landed.0 - to_px.0).abs() < 2.0 && (landed.1 - to_px.1).abs() < 2.0,
            "dropped at {to_px:?} and landed at {landed:?}",
        );
    }

    /// Letting go stops the control following the finger.
    ///
    /// A control that goes on being carried after the thumb has left is
    /// a control that cannot be put down -- and on a phone there is no
    /// second button to let go with.
    #[test]
    fn a_control_that_has_been_let_go_stops_following_the_finger() {
        let mut menu = Menu::new(ServerList::default());
        menu.set_screen_size(2712, 1220);
        menu.begin_arranging(crate::settings::TouchLayout::default());
        menu.open(Screen::TouchControls);

        let placed = menu.placed_controls(1.5);
        let slot = placed.buttons.iter().position(|b| b.shown).expect("a button");
        let from = placed.buttons[slot].centre;
        let grab = widgets::cursor_to_ui((from.0 as f64, from.1 as f64), (2712, 1220), 1.0);
        menu.set_cursor(Some(grab));
        assert!(menu.grab_at_cursor(1.5));
        menu.release_control();

        let settled = menu.arrangement();
        menu.set_cursor(Some((0.0, 0.0)));
        assert!(
            menu.arrangement().same_as(&settled),
            "the control was still being carried after the finger left",
        );
    }
    use super::*;

    /// A context for `build`. The tests care about layout and hit
    /// testing, not about what is in the settings, so this is the
    /// defaults plus an empty world list unless a test says otherwise.
    struct Fixture {
        settings: ClientSettings,
        worlds: Worlds,
        /// A folder of real saves, where a test asked for some. Kept so
        /// it is removed when the fixture goes, rather than left in the
        /// machine's temp directory once per run of the suite.
        saves: Option<std::path::PathBuf>,
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            if let Some(saves) = &self.saves {
                let _ = std::fs::remove_dir_all(saves);
            }
        }
    }

    impl Fixture {
        fn new() -> Self {
            Self {
                settings: ClientSettings::default(),
                // A path that cannot exist, so nothing is read from disk
                // and nothing can be written to it.
                worlds: Worlds::load(
                    std::env::temp_dir().join("primitive-menu-tests-no-such-folder"),
                ),
                saves: None,
            }
        }

        /// The same, over `count` real worlds on disk.
        ///
        /// **Real ones, because the screen reads them.** The buttons that
        /// act on a world are dark while the list is empty, so a test that
        /// only told the menu how many rows to expect
        /// (`Menu::set_world_count`) would find nothing pressable and
        /// would be checking the empty screen it did not mean to check.
        fn with_worlds(count: usize) -> Self {
            let root = std::env::temp_dir().join(format!(
                "primitive-menu-worlds-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir_all(&root).expect("a saves folder");
            let mut worlds = Worlds::load(&root);
            for index in 0..count {
                worlds
                    .create_in(&format!("World {index}"), index as u32, Preset::Normal, Zone::Temperate)
                    .expect("a world");
            }
            Self {
                settings: ClientSettings::default(),
                worlds: Worlds::load(&root),
                saves: Some(root),
            }
        }

        fn ctx(&self) -> MenuContext<'_> {
            MenuContext {
                version: "test",
                font: crate::engine::texture::FontAtlas::for_test(),
                settings: &self.settings,
                worlds: &self.worlds,
                background: Backdrop::Bare,
                layout: widgets::Layout::desktop(),
            }
        }
    }

    /// Hashes a run of finished geometry, to a ten-thousandth of a
    /// screen.
    ///
    /// **Quantised, and it has to be.** A screen that used to write
    /// `-0.41` and now works the same number out as a sum of four
    /// smaller ones gets `-0.40999997`, because f32 addition is not
    /// associative. That is not the layout moving -- it is a hundred
    /// thousandth of a pixel -- and a check that cannot tell the two
    /// apart is a check that has to be silenced every time an
    /// expression is rearranged. A real layout change is never smaller
    /// than a thousandth.
    /// **Two hashes, not one, and that is the point.**
    ///
    /// This used to be a single number over positions *and* colours,
    /// under a test called "not one vertex moved". So a deliberate
    /// change to the palette failed a layout test, with a message
    /// saying the screen was drawn "not in the same places" -- which
    /// was untrue, and is exactly the kind of report that sends
    /// somebody hunting a layout bug that is not there.
    ///
    /// Split, each hash answers one question and the failure says which:
    /// did the screen move, or did it change colour.
    fn fingerprint_of(out: &[crate::ui::hotbar::HotbarVertex]) -> (u64, u64) {
        let grid = |v: f32| (v * 10_000.0).round() as i64 as u64;
        let fold = |words: &[u64]| {
            let mut hash = 1469598103934665603u64;
            for word in words {
                hash ^= word;
                hash = hash.wrapping_mul(1099511628211);
            }
            hash
        };

        let mut where_it_is = Vec::with_capacity(out.len() * 5);
        let mut what_colour = Vec::with_capacity(out.len() * 4);
        for v in out {
            where_it_is.extend([
                grid(v.position[0]),
                grid(v.position[1]),
                grid(v.uv[0]),
                grid(v.uv[1]),
                u64::from(v.tex_layer),
            ]);
            what_colour.extend([
                grid(v.tint[0]),
                grid(v.tint[1]),
                grid(v.tint[2]),
                grid(v.tint[3]),
            ]);
        }
        (fold(&where_it_is), fold(&what_colour))
    }

    fn fingerprint(screen: Screen) -> (usize, u64, u64) {
        let fixture = Fixture::new();
        let mut menu = Menu::new(ServerList::default());
        menu.screen = screen;
        let mut out = Vec::new();
        menu.build_into(&fixture.ctx(), &mut out);
        let (shape, colour) = fingerprint_of(&out);
        (out.len(), shape, colour)
    }

    #[test]
    #[ignore = "a tool: prints the goldens for the test below"]
    fn print_goldens() {
        for (name, screen) in golden_screens() {
            let (count, shape, colour) = fingerprint(screen);
            println!("        (\"{name}\", {count}, {shape}, {colour}),");
        }
    }

    fn build(menu: &mut Menu) -> Vec<crate::ui::hotbar::HotbarVertex> {
        let fixture = Fixture::new();
        menu.build(&fixture.ctx())
    }

    /// The same, over a list of real worlds. See `Fixture::with_worlds`.
    fn build_over_worlds(menu: &mut Menu, count: usize) -> Vec<crate::ui::hotbar::HotbarVertex> {
        let fixture = Fixture::with_worlds(count);
        menu.build(&fixture.ctx())
    }

    fn hot_rect(menu: &Menu, wanted: &Action) -> Option<Rect> {
        menu.hot
            .iter()
            .find(|(_, action)| action == wanted)
            .map(|(rect, _)| *rect)
    }

    fn settings_on(layout: widgets::Layout) -> Menu {
        let fixture = Fixture::new();
        let mut menu = Menu::new(ServerList::default());
        menu.screen = Screen::Settings;
        let ctx = MenuContext { layout, ..fixture.ctx() };
        let _ = menu.build(&ctx);
        menu
    }

    fn near(a: Rect, b: Rect) -> bool {
        (a.x0 - b.x0).abs() < 1e-5
            && (a.y0 - b.y0).abs() < 1e-5
            && (a.x1 - b.x1).abs() < 1e-5
            && (a.y1 - b.y1).abs() < 1e-5
    }

    /// Every screen the golden test pins, by the name it is reported
    /// under.
    fn golden_screens() -> Vec<(&'static str, Screen)> {
        vec![
            ("main", Screen::Main),
            ("paused", Screen::Paused),
            ("worlds", Screen::Worlds),
            ("world_form", Screen::CreatingWorld),
            ("rename_world", Screen::RenamingWorld(0)),
            ("servers", Screen::Servers),
            ("server_form", Screen::Editing(None)),
            ("settings", Screen::Settings),
            ("controls", Screen::Controls),
            ("credits", Screen::Credits),
            // Pinned in the state a fresh menu is really in -- nothing
            // asked yet, so one sentence and a BACK button. The screen
            // with a list on it is checked by
            // `an_extension_row_is_pressed_where_it_is_drawn` and drawn
            // by `ui::snapshot`; what this pins is the empty one, which
            // is the one every singleplayer world will ever see.
            ("extensions", Screen::Extensions),
            (
                "confirm",
                Screen::Confirm {
                    question: Msg::Delete,
                    detail: "a world".to_string(),
                    confirm_label: Msg::Delete,
                    action: Box::new(Action::Back),
                },
            ),
            ("connecting", Screen::Connecting { label: "host".to_string() }),
            (
                "failed",
                Screen::Failed {
                    label: "host".to_string(),
                    reason: "connection refused".to_string(),
                },
            ),
        ]
    }

    /// Everything a screen offered to be pressed, at this layout.
    /// Every pressable thing on a screen, at a given shape and size.
    ///
    /// **Over a list that has worlds in it**, and that is not a detail:
    /// five of the six buttons under the world list are dark while the
    /// list is empty, and a dark button is not in `hot` at all. Built
    /// against an empty fixture, the test that keeps controls on the
    /// glass was checking NEW and BACK and nothing else -- on the screen
    /// with the most controls on it.
    ///
    /// The fixture is the caller's so that sixteen shapes do not mean
    /// sixteen folders of saves made and removed.
    fn targets_on(
        fixture: &Fixture,
        screen: Screen,
        layout: widgets::Layout,
    ) -> Vec<(Rect, Action)> {
        let mut menu = Menu::new(ServerList::default());
        menu.screen = screen;
        let ctx = MenuContext { layout, ..fixture.ctx() };
        let mut out = Vec::new();
        menu.build_into(&ctx, &mut out);
        menu.hot.clone()
    }

    /// The phone this was all for, held sideways.
    const PHONE: f32 = 2712.0 / 1220.0;

    #[test]
    fn every_screen_grows_when_the_interface_size_does() {
        // **The report this answers, in the player's words: "the
        // interface size changes only on the settings screen and
        // in-game".** It was exactly true. The blanket scaling that used
        // to carry all twelve was removed when the settings screen
        // learned to lay itself out, and for a while the settings screen
        // was the only one that had.
        //
        // So: every screen, on both shapes of window. What must hold is
        // that the buttons a player presses actually get bigger --
        // measured on their area, because some screens grow sideways
        // more than downwards and either counts.
        // Three worlds on disk, so the five buttons that act on one are
        // in the list at all -- see `targets_on`.
        let fixture = Fixture::with_worlds(3);
        for aspect in [16.0f32 / 9.0, PHONE] {
            for (name, screen) in golden_screens() {
                let small =
                    targets_on(&fixture, screen.clone(), widgets::Layout::for_screen(aspect, 1.0));
                let large =
                    targets_on(&fixture, screen, widgets::Layout::for_screen(aspect, 2.0));
                assert!(!small.is_empty(), "{name} offers nothing to press");
                // The *biggest* target rather than the total, because a
                // list screen answers a bigger interface by showing
                // fewer rows -- which is the point of it -- so the sum
                // is allowed to go down while every row on it goes up.
                let biggest = |targets: &[(Rect, Action)]| -> f32 {
                    targets
                        .iter()
                        .map(|(r, _)| r.width() * r.height())
                        .fold(0.0f32, f32::max)
                };
                let (was, now) = (biggest(&small), biggest(&large));
                assert!(
                    now > was * 1.2,
                    "{name} at {aspect:.2}: its targets came to {was} and grew only to {now}",
                );
            }
        }
    }

    #[test]
    fn no_screen_puts_anything_pressable_off_the_glass() {
        // The other half: growing is only an improvement while it all
        // stays on the screen. Checked at the top of the range and on a
        // window far squarer than anything was designed for.
        let fixture = Fixture::with_worlds(3);
        for aspect in [16.0f32 / 9.0, PHONE, 4.0 / 3.0, 1.0] {
            // The whole range the setting is sanitised to (see
            // `ClientSettings::sanitize`), not a sample of it: the size
            // a control comes out at is a floor and a fit interacting,
            // and both ends are where that goes wrong.
            for requested in [0.5f32, 1.0, 1.5, 2.0, 4.0] {
                let layout = widgets::Layout::for_screen(aspect, requested);
                for (name, screen) in golden_screens() {
                    for (rect, action) in targets_on(&fixture, screen.clone(), layout) {
                        assert!(
                            rect.x0 >= -aspect - 1e-3 && rect.x1 <= aspect + 1e-3,
                            "{name} at {aspect:.2}x{requested}: {action:?} is off the side at {rect:?}",
                        );
                        assert!(
                            rect.y0 >= -1.0 - 1e-3 && rect.y1 <= 1.0 + 1e-3,
                            "{name} at {aspect:.2}x{requested}: {action:?} is off the top or bottom at {rect:?}",
                        );
                        assert!(
                            rect.width() > 0.0 && rect.height() > 0.0,
                            "{name} at {aspect:.2}x{requested}: {action:?} has no area at all",
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn the_buttons_under_a_list_stand_on_the_panels_own_edges() {
        // The row under the world and server lists used to stop short of
        // the panel by an amount each screen chose for itself, and BACK
        // lined up with nothing. See `columns`.
        let fixture = Fixture::new();
        for aspect in [16.0f32 / 9.0, PHONE] {
            let layout = widgets::Layout::for_screen(aspect, 1.0);
            let ctx = MenuContext { layout, ..fixture.ctx() };
            let edge = layout.panel_half_width(0.95);
            // The *last* target with an action: the selected row answers
            // `Connect(0)` too, and it is pushed before the buttons.
            let last = |menu: &Menu, wanted: &Action| {
                menu.hot.iter().rev().find(|(_, action)| action == wanted).map(|(rect, _)| *rect)
            };

            let mut servers = Menu::new(ServerList {
                servers: vec![ServerEntry { name: "home".to_string(), address: "10.0.0.1:7878".to_string() }],
            });
            servers.screen = Screen::Servers;
            let _ = servers.build(&ctx);
            let play = last(&servers, &Action::Connect(0)).expect("PLAY");
            let add = last(&servers, &Action::Add).expect("ADD");
            let edit = last(&servers, &Action::Edit(0)).expect("EDIT");
            let delete = last(&servers, &Action::Delete(0)).expect("DELETE");
            let back = last(&servers, &Action::Back).expect("BACK");
            assert!(
                (play.x0 + edge).abs() < 1e-4 && (delete.x1 - edge).abs() < 1e-4,
                "at {aspect:.2} the server buttons run {:.3}..{:.3} under a panel of +-{edge:.3}",
                play.x0,
                delete.x1,
            );
            assert!(
                (back.x0 - add.x0).abs() < 1e-4 && (back.x1 - edit.x1).abs() < 1e-4,
                "at {aspect:.2} BACK is not under ADD and EDIT: {back:?}",
            );

            let mut worlds = Menu::new(ServerList::default());
            worlds.screen = Screen::Worlds;
            let _ = worlds.build(&ctx);
            let new = last(&worlds, &Action::NewWorld).expect("NEW");
            let back = last(&worlds, &Action::Back).expect("BACK");
            assert!(
                (new.x0 - back.x0).abs() < 1e-4 && (new.x1 - back.x1).abs() < 1e-4,
                "at {aspect:.2} BACK is not under NEW",
            );
            assert!((new.x0 + new.x1).abs() < 1e-4, "at {aspect:.2} NEW is not the middle column");
        }
    }

    #[test]
    fn a_phone_can_reach_every_row_of_the_server_list() {
        // The list scrolled by *selection*, which reads as scrolling
        // only while the arrow keys are what moves it. A phone has none,
        // so the wheel -- which is what a thumb drag arrives as -- has
        // to move the view.
        let mut menu = Menu::new(ServerList {
            servers: (0..20)
                .map(|i| ServerEntry {
                    name: format!("server {i}"),
                    address: format!("10.0.0.{i}:7878"),
                })
                .collect(),
        });
        menu.screen = Screen::Servers;
        let fixture = Fixture::new();
        let ctx = MenuContext {
            layout: widgets::Layout::for_screen(PHONE, 1.5),
            ..fixture.ctx()
        };
        let mut out = Vec::new();
        menu.build_into(&ctx, &mut out);
        let visible = menu.server_visible;
        assert!(visible > 0 && visible < 20, "the whole list fits; nothing to test");

        // Every row is reachable by scrolling, and the list stops at the
        // end rather than running off into blank space.
        menu.scroll(1);
        assert_eq!(menu.server_scroll, 1, "the wheel did not move the list");
        for _ in 0..40 {
            menu.scroll(1);
        }
        assert_eq!(
            menu.server_scroll,
            20 - visible,
            "the list overscrolled past its last row",
        );
        for _ in 0..40 {
            menu.scroll(-1);
        }
        assert_eq!(menu.server_scroll, 0, "the list would not come back to the top");
    }

    /// A list of extensions, as a small server would answer.
    fn some_extensions(count: usize) -> primitive_shared::protocol::ExtensionList {
        use primitive_shared::protocol::{ExtensionInfo, ExtensionKind, ExtensionList};
        ExtensionList {
            native_api: Some((2, 1)),
            scripts_supported: true,
            items: (0..count)
                .map(|i| ExtensionInfo {
                    kind: if i % 2 == 0 { ExtensionKind::Native } else { ExtensionKind::Script },
                    name: format!("thing {i}"),
                    version: "1.0.0".to_string(),
                    description: "does a thing".to_string(),
                    authors: vec!["someone".to_string()],
                    enabled: true,
                    reason: String::new(),
                    built_for: Some((2, 1)),
                    settings: vec![("scale".to_string(), "1.4".to_string())],
                })
                .collect(),
        }
    }

    /// A row of the extensions list is pressed exactly where it is
    /// drawn, at every interface size.
    ///
    /// **The rule in CLAUDE.md, on the newest screen.** The list is laid
    /// out from a panel whose height depends on the button under it and
    /// whose row height is floored at a finger, and the hit-test table
    /// is filled in by the same pass that draws -- so the only way this
    /// can go wrong is if the two stop being one pass. It is worth a
    /// test anyway: this screen has two panes, and a row that is hit
    /// against the *panel* rather than against the list would answer a
    /// finger over the description beside it.
    #[test]
    fn an_extension_row_is_pressed_where_it_is_drawn() {
        for aspect in [4.0f32 / 3.0, 16.0 / 9.0, PHONE] {
            for step in 0..=5 {
                let scale = 0.6 + step as f32 * 0.2;
                let fixture = Fixture::new();
                let mut menu = Menu::new(ServerList::default());
                menu.screen = Screen::Extensions;
                menu.set_extensions(some_extensions(4));
                let ctx = MenuContext {
                    layout: widgets::Layout::for_screen(aspect, scale),
                    ..fixture.ctx()
                };
                let mut out = Vec::new();
                menu.build_into(&ctx, &mut out);

                let rows: Vec<(Rect, usize)> = menu
                    .hot
                    .iter()
                    .filter_map(|(rect, action)| match action {
                        Action::SelectExtension(index) => Some((*rect, *index)),
                        _ => None,
                    })
                    .collect();
                assert!(!rows.is_empty(), "no row was offered at {aspect}/{scale}");
                for (rect, index) in rows {
                    let hit = menu
                        .hot
                        .iter()
                        .find(|(r, _)| r.contains(rect.centre_x(), rect.centre_y()))
                        .map(|(_, action)| action.clone());
                    assert_eq!(
                        hit,
                        Some(Action::SelectExtension(index)),
                        "row {index} at {aspect}/{scale} is not pressed where it is drawn",
                    );
                }
            }
        }
    }

    /// A server running nothing says so, and says something different
    /// from a server that has not answered yet.
    ///
    /// **The commonest case is the empty one.** The client's own
    /// embedded server has no loaders at all -- see
    /// `primitive_client/Cargo.toml` -- so every singleplayer world in
    /// the game reaches this screen and finds nothing on it. A blank
    /// panel there reads as a broken screen; three different sentences
    /// is the whole feature.
    #[test]
    fn an_empty_extensions_screen_says_which_kind_of_empty_it_is() {
        use primitive_shared::protocol::ExtensionList;
        let fixture = Fixture::new();
        let draw = |extensions: Extensions| {
            let mut menu = Menu::new(ServerList::default());
            menu.screen = Screen::Extensions;
            menu.extensions = extensions;
            let mut out = Vec::new();
            menu.build_into(&fixture.ctx(), &mut out);
            (out.len(), fingerprint_of(&out).0)
        };

        let waiting = draw(Extensions::Waiting);
        let none = draw(Extensions::Known(ExtensionList {
            native_api: Some((2, 1)),
            scripts_supported: true,
            items: Vec::new(),
        }));
        let no_loader = draw(Extensions::Known(ExtensionList::default()));

        assert!(waiting.0 > 0, "an unanswered screen drew nothing at all");
        assert_ne!(
            waiting, none,
            "waiting for the list and being told there is none look the same",
        );
        assert_ne!(
            none, no_loader,
            "a server with no mods and a build with no loader look the same",
        );
    }

    /// Nothing a mod wrote in its own manifest can reach past the pane
    /// it is drawn in.
    ///
    /// The description, the authors and the settings are all somebody
    /// else's text, of whatever length they liked. Left unbounded, a mod
    /// with a paragraph in its manifest writes over the BACK button and
    /// then off the bottom of the screen.
    ///
    /// **Checked against the empty band under the panel**, which is the
    /// gap between the bottom of the panel and the top of the BACK
    /// button. Nothing is ever drawn there, and text that overflowed the
    /// pane would march into it before it reached anything else --
    /// lines are laid out downward one after another, so the first thing
    /// too much writing does is cross that gap. Counting vertices
    /// instead was tried and thrown away: ten times the manifest draws
    /// *almost* the same picture, differing by the few glyphs of
    /// whichever line happens to be the last one to fit, and a test that
    /// has to be told how many glyphs that is measures nothing.
    #[test]
    fn a_talkative_manifest_never_writes_under_its_panel() {
        use primitive_shared::protocol::{ExtensionInfo, ExtensionKind, ExtensionList};
        let list = ExtensionList {
            native_api: Some((2, 1)),
            scripts_supported: true,
            items: vec![ExtensionInfo {
                kind: ExtensionKind::Native,
                name: "a mod with a very long name indeed, far longer than its column".to_string(),
                version: "1.0.0".to_string(),
                description: "a description ".repeat(200),
                authors: (0..40).map(|i| format!("author number {i}")).collect(),
                enabled: true,
                reason: String::new(),
                built_for: Some((2, 1)),
                settings: (0..200)
                    .map(|i| (format!("setting_{i}"), format!("value {i}")))
                    .collect(),
            }],
        };

        for aspect in [4.0f32 / 3.0, 16.0 / 9.0, PHONE] {
            for step in 0..=4 {
                let scale = 0.6 + step as f32 * 0.2;
                let fixture = Fixture::new();
                let mut menu = Menu::new(ServerList::default());
                menu.screen = Screen::Extensions;
                menu.set_extensions(list.clone());
                let ctx = MenuContext {
                    layout: widgets::Layout::for_screen(aspect, scale),
                    ..fixture.ctx()
                };
                let mut out = Vec::new();
                menu.build_into(&ctx, &mut out);

                // The band, worked out the way the screen works it out.
                let layout = ctx.layout;
                let button_height = layout.at(0.10).max(layout.finger());
                let button_top = -0.83 + layout.at(0.08) + button_height;
                let panel_bottom = button_top + layout.at(0.08);
                for v in &out {
                    assert!(
                        v.position[1] <= button_top + 1e-4
                            || v.position[1] >= panel_bottom - 1e-4,
                        "at aspect {aspect} size {scale} the pane wrote into the gap under \
                         the panel, at {:?}",
                        v.position,
                    );
                }
            }
        }
    }

    /// A shorter list than last time never leaves the highlight on a row
    /// that is not there.
    ///
    /// A player who leaves a server with six mods for one with two would
    /// otherwise arrive with row four selected, and the detail pane
    /// would be blank with nothing on screen to explain it.
    #[test]
    fn a_shorter_list_pulls_the_highlight_back_onto_a_row_that_exists() {
        let mut menu = Menu::new(ServerList::default());
        menu.set_extensions(some_extensions(6));
        menu.move_extension_selection(5);
        assert_eq!(menu.extension_selected, 5);

        menu.set_extensions(some_extensions(2));
        assert!(
            menu.extension_selected < 2,
            "the highlight stayed on row {} of a two-row list",
            menu.extension_selected,
        );

        // ...and an empty one is not a panic, which is the case every
        // singleplayer world is in.
        menu.set_extensions(some_extensions(0));
        menu.move_extension_selection(1);
        let fixture = Fixture::new();
        menu.screen = Screen::Extensions;
        let mut out = Vec::new();
        menu.build_into(&fixture.ctx(), &mut out);
        assert!(!out.is_empty(), "an empty list drew nothing");
    }

    #[test]
    fn a_phone_with_no_mods_says_so_instead_of_blaming_the_world() {
        // On Android the client is built without the server's `mods`
        // feature -- `primitive_client/Cargo.toml` asks for it on every
        // other target -- so an in-process world there reports no
        // native loader and no scripting engine, which is exactly what
        // a desktop's singleplayer world reports when it has neither.
        // The screen said the same thing to both, and on a phone that
        // sentence ("this world runs inside the game") reads as an
        // invitation to join a server and find mods there. The half
        // that loads them is not in the package at all.
        let none = Extensions::Known(primitive_shared::protocol::ExtensionList {
            native_api: None,
            scripts_supported: false,
            items: Vec::new(),
        });
        assert_eq!(
            empty_extensions_line(&none, true),
            Msg::ExtensionsNoLoader,
            "a desktop stopped explaining its own singleplayer world",
        );
        assert_eq!(
            empty_extensions_line(&none, false),
            Msg::ExtensionsNoLoaderPhone,
            "a phone was told the world was at fault",
        );
        // ...and a server that *can* run them and does not is still a
        // different sentence, on both.
        let idle = Extensions::Known(primitive_shared::protocol::ExtensionList {
            native_api: Some((2, 1)),
            scripts_supported: true,
            items: Vec::new(),
        });
        for loads in [true, false] {
            assert_eq!(empty_extensions_line(&idle, loads), Msg::ExtensionsNone);
        }
        assert_eq!(empty_extensions_line(&Extensions::Waiting, false), Msg::ExtensionsAsking);
    }

    /// The list arriving is visible to the key that decides whether to
    /// redraw.
    ///
    /// It arrives several frames after the screen was opened and moves
    /// nothing else -- no cursor, no selection, no screen change. Left
    /// out of the key, the answer would land in the menu's state and
    /// never reach the glass.
    #[test]
    fn the_list_arriving_changes_the_key_that_decides_to_redraw() {
        let fixture = Fixture::new();
        let mut menu = Menu::new(ServerList::default());
        menu.screen = Screen::Extensions;
        let asking = menu.ui_key(&fixture.ctx());
        menu.set_extensions(some_extensions(3));
        assert_ne!(
            asking,
            menu.ui_key(&fixture.ctx()),
            "the list arrived and the screen was never rebuilt",
        );
    }

    /// Leaving a world throws the list away.
    ///
    /// Kept, it would be shown on the next server -- correct-looking and
    /// about somewhere else -- until that server's own answer happened
    /// to arrive.
    #[test]
    fn leaving_a_server_forgets_what_was_extending_it() {
        let mut menu = Menu::new(ServerList::default());
        menu.set_extensions(some_extensions(3));
        assert_eq!(menu.extensions.items().len(), 3);
        menu.forget_extensions();
        assert!(menu.extensions.items().is_empty());
        // ...and asking again is a fresh question rather than a repeat.
        menu.apply(Action::OpenExtensions);
        assert!(menu.extensions_awaited());
    }

    /// The pause menu is the way in, and Back from it returns to the
    /// paused world rather than to the main menu.
    ///
    /// Landing on the main menu after closing a screen reads exactly
    /// like having been disconnected, which is the report the settings
    /// screen's own `came_from` was added for.
    #[test]
    fn the_extensions_screen_goes_back_to_the_world_it_was_opened_from() {
        let mut menu = Menu::new(ServerList::default());
        menu.screen = Screen::Paused;
        menu.apply(Action::OpenExtensions);
        assert_eq!(menu.screen, Screen::Extensions);
        menu.apply(Action::Back);
        assert_eq!(menu.screen, Screen::Paused);
    }

    /// The caret keys reach every field on every screen that has one.
    ///
    /// **The settings screen is the one this is really about.** Its name
    /// row carried its own tiny copy of the editing rules -- a `pop()`
    /// for Backspace and nothing else -- so the caret keys worked on the
    /// two form screens and not on the row every player types their name
    /// into. Both go through `Menu::edit` now, and this is what says so.
    #[test]
    fn every_screen_with_a_field_edits_it_the_same_way() {
        for screen in [Screen::Editing(None), Screen::CreatingWorld, Screen::Settings] {
            let mut menu = Menu::new(ServerList::default());
            menu.screen = screen.clone();
            if matches!(screen, Screen::Settings) {
                menu.begin_username_edit(String::new());
            }
            menu.focus = Field::Name;
            for c in "borya".chars() {
                menu.type_char(c);
            }
            menu.key(Key::Home { extend: false });
            menu.type_char('X');
            assert_eq!(
                menu.name_input.text(),
                "Xborya",
                "{screen:?} would not type at the start of its own field",
            );
            menu.key(Key::Delete { word: false });
            assert_eq!(menu.name_input.text(), "Xorya", "{screen:?} has no Delete");
            menu.key(Key::End { extend: false });
            menu.key(Key::Backspace { word: false });
            assert_eq!(menu.name_input.text(), "Xory", "{screen:?} has no Backspace");
            menu.key(Key::SelectAll);
            menu.type_char('q');
            assert_eq!(
                menu.name_input.text(),
                "q",
                "{screen:?} would not let a field be selected and replaced",
            );
        }
    }

    /// Select-all and type works on a field that is already full.
    ///
    /// The cap is on what a field may *hold*, and typing over a
    /// selection does not add to it. Checked because the obvious
    /// implementation refuses the keystroke and leaves the player
    /// holding a full field they cannot replace.
    #[test]
    fn a_full_field_can_still_be_selected_and_rewritten() {
        let mut menu = Menu::new(ServerList::default());
        menu.screen = Screen::Editing(None);
        menu.focus = Field::Name;
        for _ in 0..MAX_NAME {
            menu.type_char('x');
        }
        assert_eq!(menu.name_input.chars(), MAX_NAME);
        menu.key(Key::SelectAll);
        menu.type_char('a');
        assert_eq!(menu.name_input.text(), "a");
    }

    /// A seed field still takes digits only, from every door into it.
    ///
    /// The filter lives in `type_char` and the caret keys go somewhere
    /// else entirely, so the two could drift apart without anything
    /// saying so.
    #[test]
    fn a_caret_does_not_let_a_letter_into_the_seed() {
        let mut menu = Menu::new(ServerList::default());
        menu.screen = Screen::CreatingWorld;
        menu.focus = Field::Seed;
        for c in "12x34".chars() {
            menu.type_char(c);
        }
        assert_eq!(menu.seed_input.text(), "1234");
        menu.key(Key::Home { extend: false });
        menu.type_char('a');
        assert_eq!(menu.seed_input.text(), "1234", "a letter got in at the start");
        menu.type_char('9');
        assert_eq!(menu.seed_input.text(), "91234");
    }

    /// An input method's line arrives with the caret at its end.
    ///
    /// The phone owns the caret while it owns the text -- see
    /// `ui::field` -- and a caret this side left in the middle of the
    /// old value would put the next committed character somewhere
    /// nobody pointed at.
    #[test]
    fn a_line_from_an_input_method_leaves_the_caret_at_the_end() {
        let mut menu = Menu::new(ServerList::default());
        menu.screen = Screen::Editing(None);
        menu.focus = Field::Name;
        for c in "old".chars() {
            menu.type_char(c);
        }
        menu.key(Key::Home { extend: false });
        menu.set_focused_text("совсем другое");
        assert_eq!(menu.focused_text(), "совсем другое");
        assert_eq!(
            menu.name_input.caret(),
            menu.name_input.text().len(),
            "the mirror left the caret in the middle of what it replaced",
        );
    }

    /// A click in a field puts the caret under the pointer.
    ///
    /// **The rule in CLAUDE.md, at the one place it has to hold to a
    /// single character.** Everywhere else a hit-test has to land on
    /// the right rectangle; here it has to land between the right two
    /// letters, and the only way that can be true is for the click to
    /// walk the same string, at the same size, through the same window
    /// as the drawing. Both sides go through `widgets::field_metrics`;
    /// this points at the middle of every character in turn and asks
    /// for it back.
    #[test]
    fn a_click_in_a_field_puts_the_caret_where_it_was_aimed() {
        for aspect in [4.0f32 / 3.0, 16.0 / 9.0, PHONE] {
            for step in 0..=4 {
                let scale = 0.6 + step as f32 * 0.2;
                let fixture = Fixture::new();
                let mut menu = Menu::new(ServerList::default());
                menu.screen = Screen::Editing(None);
                menu.focus = Field::Name;
                for c in "привет мир".chars() {
                    menu.type_char(c);
                }
                let ctx = MenuContext {
                    layout: widgets::Layout::for_screen(aspect, scale),
                    ..fixture.ctx()
                };
                let mut out = Vec::new();
                menu.build_into(&ctx, &mut out);

                let (rect, _) = *menu
                    .field_boxes
                    .iter()
                    .find(|(_, which)| *which == Field::Name)
                    .expect("the name field was drawn");
                let text = menu.name_input.text().to_string();
                let content = ctx.layout.content();
                let metrics =
                    widgets::field_metrics(rect, content, &text, menu.name_input.caret());
                let left = rect.x0 + metrics.pad;

                // The middle of each character on screen: a click there
                // has to come back as the boundary on one side of it,
                // and pressing on the left half has to give the near
                // side rather than the far one.
                let mut previous = metrics.from;
                for (index, c) in text[metrics.from..metrics.to].char_indices() {
                    let start = metrics.from + index;
                    let end = start + c.len_utf8();
                    let x0 = left + widgets::measure(&text[metrics.from..start], metrics.scale);
                    let x1 = left + widgets::measure(&text[metrics.from..end], metrics.scale);
                    menu.cursor = Some((x0 + (x1 - x0) * 0.2, rect.centre_y()));
                    menu.apply(Action::Focus(Field::Name));
                    assert_eq!(
                        menu.name_input.caret(),
                        start,
                        "at {aspect}/{scale} a click on the left of {c:?} \
                         landed at {} rather than {start}",
                        menu.name_input.caret(),
                    );
                    menu.cursor = Some((x0 + (x1 - x0) * 0.8, rect.centre_y()));
                    menu.apply(Action::Focus(Field::Name));
                    assert_eq!(
                        menu.name_input.caret(),
                        end,
                        "at {aspect}/{scale} a click on the right of {c:?} \
                         landed at {} rather than {end}",
                        menu.name_input.caret(),
                    );
                    previous = end;
                }
                assert_eq!(previous, metrics.to, "the walk missed the end of the line");
            }
        }
    }

    /// Focusing a field for any reason other than a click leaves the
    /// caret alone.
    ///
    /// A form that fails validation returns `Action::Focus` to put the
    /// player in the field that is wrong -- and the mouse is wherever
    /// it was left, which is nowhere anybody aimed.
    #[test]
    fn a_field_focused_without_a_click_keeps_the_caret_it_had() {
        let fixture = Fixture::new();
        let mut menu = Menu::new(ServerList::default());
        menu.screen = Screen::Editing(None);
        menu.focus = Field::Name;
        for c in "borya".chars() {
            menu.type_char(c);
        }
        menu.key(Key::Home { extend: false });
        let mut out = Vec::new();
        menu.build_into(&fixture.ctx(), &mut out);

        // The pointer somewhere else entirely.
        menu.cursor = Some((-3.0, 0.9));
        menu.apply(Action::Focus(Field::Name));
        assert_eq!(menu.name_input.caret(), 0, "a stray pointer moved the caret");

        // ...and with no pointer at all, which is a keyboard.
        menu.cursor = None;
        menu.apply(Action::Focus(Field::Name));
        assert_eq!(menu.name_input.caret(), 0);
    }

    /// A phone's own editor keeps its caret.
    ///
    /// While an input method holds the field it holds the caret too,
    /// and every edit arrives as a whole new line through the mirror.
    /// A caret dropped by a tap would be drawn in the middle of a value
    /// whose next character is going on the end -- a lie that looks
    /// like a feature.
    #[test]
    fn a_tap_does_not_move_a_caret_the_input_method_owns() {
        let fixture = Fixture::new();
        let mut menu = Menu::new(ServerList::default());
        menu.screen = Screen::Editing(None);
        menu.focus = Field::Name;
        for c in "borya".chars() {
            menu.type_char(c);
        }
        let mut out = Vec::new();
        menu.build_into(&fixture.ctx(), &mut out);
        let (rect, _) = *menu
            .field_boxes
            .iter()
            .find(|(_, which)| *which == Field::Name)
            .expect("the name field was drawn");

        menu.set_ime_owns_text(true);
        menu.cursor = Some((rect.x0 + 0.02, rect.centre_y()));
        menu.apply(Action::Focus(Field::Name));
        assert_eq!(
            menu.name_input.caret(),
            menu.name_input.text().len(),
            "a tap moved a caret the input method owns",
        );

        // ...and the same tap on a desktop does move it.
        menu.set_ime_owns_text(false);
        menu.apply(Action::Focus(Field::Name));
        assert!(menu.name_input.caret() < menu.name_input.text().len());
    }

    /// Both arrows on a stepped row of the world form answer the way they
    /// point, where they are drawn, on a desktop and on a phone.
    ///
    /// **The bug this pins.** The row's own "step forward" target was
    /// pushed before the `<` inside it, and a click answers the first
    /// target containing it -- so `<` stepped forward and nothing on the
    /// form could step back with a mouse. Asked of `hovered`, which is
    /// what `click` asks, rather than of the list of rectangles, which
    /// said all along that `<` was there.
    #[test]
    fn a_choice_on_the_world_form_steps_the_way_its_arrow_points() {
        for layout in [widgets::Layout::desktop(), widgets::Layout::for_screen(PHONE, 1.5)] {
            let fixture = Fixture::new();
            let mut menu = Menu::new(ServerList::default());
            menu.apply(Action::NewWorld);
            let ctx = MenuContext { layout, ..fixture.ctx() };
            let _ = menu.build(&ctx);
            for (back, forward) in [
                (Action::StepPreset(-1), Action::StepPreset(1)),
                (Action::StepZone(-1), Action::StepZone(1)),
            ] {
                let left = hot_rect(&menu, &back).expect("a way back");
                // The right-most target that steps forward is the arrow;
                // the other is the word between the two.
                let right = menu
                    .hot
                    .iter()
                    .filter(|(_, action)| *action == forward)
                    .map(|(rect, _)| *rect)
                    .max_by(|a, b| a.x0.total_cmp(&b.x0))
                    .expect("a way forward");
                assert!(left.x1 <= right.x0, "{back:?} at {left:?} is not left of {forward:?} at {right:?}");
                for (rect, wanted) in [(left, back.clone()), (right, forward.clone())] {
                    menu.set_cursor(Some((rect.centre_x(), rect.centre_y())));
                    let answered = menu.hovered().cloned();
                    assert_eq!(
                        answered,
                        Some(wanted.clone()),
                        "a click on the arrow for {wanted:?} at {rect:?} answered {answered:?}",
                    );
                }
            }
        }
    }

    /// **The roll button rolls, where it is drawn, and the seed box beside it
    /// still takes the finger.** On a desktop and on a phone: the button is a
    /// target of its own at the right of the seed row, clicking its middle
    /// answers the roll and puts a number in the box, a second roll is
    /// another number, and the middle of the box still focuses the box.
    #[test]
    fn the_seed_row_rolls_a_seed_where_its_button_is_drawn() {
        for layout in [widgets::Layout::desktop(), widgets::Layout::for_screen(PHONE, 1.5)] {
            let fixture = Fixture::new();
            let mut menu = Menu::new(ServerList::default());
            menu.apply(Action::NewWorld);
            let ctx = MenuContext { layout, ..fixture.ctx() };
            let _ = menu.build(&ctx);
            let roll = hot_rect(&menu, &Action::RollSeed).expect("a roll button on the seed row");
            let field = hot_rect(&menu, &Action::Focus(Field::Seed)).expect("the seed box");
            assert!(field.x1 <= roll.x0, "the seed box {field:?} runs under the roll button {roll:?}");
            for (rect, wanted) in [(roll, Action::RollSeed), (field, Action::Focus(Field::Seed))] {
                menu.set_cursor(Some((rect.centre_x(), rect.centre_y())));
                assert_eq!(menu.hovered().cloned(), Some(wanted.clone()), "the middle of {rect:?} is not {wanted:?}");
            }
            menu.apply(Action::RollSeed);
            let first = menu.seed_input.text().to_string();
            assert!(first.parse::<u32>().is_ok() && first.len() <= MAX_SEED_DIGITS, "rolled {first:?}");
            menu.apply(Action::RollSeed);
            assert_ne!(menu.seed_input.text(), first, "two rolls gave one seed");
        }
    }

    #[test]
    fn a_blank_seed_is_a_new_seed_each_time() {
        let rolls: std::collections::HashSet<u32> = (0..16).map(|_| random_seed()).collect();
        assert!(rolls.len() >= 15, "sixteen rolls gave {} seeds", rolls.len());
    }

    #[test]
    fn not_one_vertex_of_the_desktop_menu_moved() {
        // **The guarantee, for every screen at once.** Ten of these
        // were rewritten from boxes of literals into layouts that ask
        // `ctx.layout` for their sizes, so that a phone could be handed
        // a different one. The machine they were designed on must not
        // be able to tell.
        //
        // A fingerprint rather than the numbers written out, because
        // writing out four hundred rectangles is not a test anybody
        // maintains -- and because what has to hold here is not "the
        // buttons are roughly right", it is *nothing moved*. These were
        // taken from the build before the conversion.
        //
        // If one of these fails and the change was deliberate, run the
        // `print_goldens` tool above and read the new numbers -- but
        // read the *screen* first, because a desktop layout changing is
        // the thing this is here to stop.
        // The four *colour* fingerprints below moved together, and
        // nothing else did: every count and every shape is byte for
        // byte what it was. `Theme::DARK.row` went two per cent darker
        // so that the amber written on a row clears 4.5:1 -- see
        // `small_text_is_readable_against_everything_it_is_drawn_on`,
        // which holds `accent` to a floor now -- and these are the four
        // screens that draw a row. Nothing moved; one surface is a
        // shade deeper.
        const GOLDEN: &[(&str, usize, u64, u64)] = &[
        ("main", 552, 13211915381958015075, 9438617344697449539),
        // Five rows since the extensions screen got a way in, and a
        // key to the gauges under the title since a player asked what
        // the bar under the temperature was. See `Menu::gauge_legend`.
        // The pause screen prints the gauge legend, and the legend
        // grew a row: tiredness is the seventh meter (see
        // `hud::GAUGE_LEGEND`). Eighty-four vertices is one more
        // mark and one more line of text, which is what a row of
        // that legend costs -- the rest of the screen is untouched.
        // Same count, new shape: the title went up from 0.52 to the main
        // menu's 0.62 so the legend under it has a band of its own (see
        // `Menu::PAUSE_TITLE_TOP`).
        // ...and the colour hash alone after that, with the count and
        // every position untouched: the seven marks in the legend are
        // drawn in their own meters' inks instead of one pale grey, so
        // the key is now a colour key as well as a list of names. See
        // `gauge_legend`, and `hud::GAUGE_LEGEND` for the table both
        // halves read.
        ("paused", 1152, 13024866501460846824, 16641318740995359347),
        // Shape only: the three buttons and BACK stand on columns cut from
        // the panel (see `columns`) instead of widths of their own.
        // 744 since the list grew up: RENAME and COPY fill the two
        // columns that were empty beside BACK, every row says the day,
        // the season, the size and the date it was played on instead of
        // a seed and an age, the panel is as tall as the worlds in it
        // (rule 2 in `widgets`) with the title following it down, and an
        // unchosen world's name is written in the bright ink.
        ("worlds", 744, 7660126314126665684, 13926778572275578147),
        // Two placeholders where there were none, and the seed row no
        // longer writing its fallback into the field as if it were a
        // value: the vertex count is the writing that is now there and
        // the shape hash is the writing that moved out of the well.
        // Then CLIMATE, a fourth row, and a `>` beside every `<` (see
        // `Menu::stepper`) -- 1344 vertices -- and CREATE and CANCEL on
        // the panel's two halves (see `columns`). 1374 since that row was
        // renamed: one seed is one planet now, and the row picks where on
        // it you wake rather than what the world is made of, so it says
        // "ГДЕ ПРОСНЁТЕСЬ" and the longer word is thirty more vertices.
        // 1440 since the seed row rolls: a ROLL button at the end of the
        // box, which is the box a button's width shorter, and the
        // placeholder saying "random" where it said the settings' number.
        // 1680 since HOW BIG THE WORLD IS: a fifth row, its two arrows
        // and its word. That was a constant in `Worlds::create_in` --
        // every world the newest generator, and no way to ask for the
        // archipelago -- and it is the one choice on this form with no
        // better answer, so it is a row. `FormRows` closed the pitch to
        // hold it; nothing came off the bottom of the glass.
        ("world_form", 1680, 12592738192208125090, 12594601485374661331),
        // The rename form: the new-world form with four rows taken out,
        // laid out by the same `FormRows` so its box dodges a phone's
        // keyboard the same way. New, so there is nothing for it to
        // have moved from.
        ("rename_world", 786, 8079040803799444616, 13161572920153024867),
        // Shape only, as the world list.
        ("servers", 726, 10814275185546909310, 5468121121085479251),
        // Shape only: SAVE and CANCEL on the panel's two halves.
        ("server_form", 990, 2817167687195368495, 5300249442363723651),
        // Twenty rows since the detail-distance row was added (see
        // `Setting::LodDistance`), and once again the only thing that
        // moved is the scrollbar thumb: the same vertex count, because
        // the panel shows a fixed window of rows, the same colours,
        // and a slider one twentieth shorter.
        // One more row (the LOD quality step, `Setting::LodQuality`),
        // and the panel shows a fixed window of rows -- so the count is
        // unchanged and only the shapes move: a shorter scrollbar thumb
        // over the same list. Same reason the row before it moved these
        // numbers when it was added.
        // ...and the shadows switch (`Setting::Shadows`), twenty-two
        // rows: the count and the colour hash are exactly what they
        // were, and the shape hash is the thumb shortening again. The
        // row itself is below the first panel's worth and draws nothing
        // on this screen.
        // Same count again; the thumb shortened for the rows added since,
        // and it is drawn in the quiet ink rather than the amber every
        // reading on this screen is written in (see `Painter::scrollbar`).
        // ...and the shadow distance under the shadows switch
        // (`Setting::ShadowDistance`), twenty-three rows: the thumb again.
        // ...and the stones' thickness distance (`Setting::ReliefDistance`),
        // twenty-four: the thumb again. The see-through leaves row turning
        // from a switch into a stepped row is below the first panel's worth
        // and draws nothing here.
        // ...and which plants cast (`Setting::PlantShadows`), twenty-five:
        // the same count and colours, and the thumb shorter again. The row
        // is under the shadow distance, below the first panel's worth.
        // ...and twelve vertices fewer, because the field-of-view row
        // reads "70°" where it read "70 deg": three glyphs instead of
        // six, and a glyph is four vertices. The sign is the same in all
        // four languages, which is the point of it -- the word was a stub
        // in English, wrong after 1, 2, 3 and 4 in Polish, and also the
        // Russian for hail.
        // ...and RESOLUTION (`Setting::Resolution`), twenty-six -- the
        // first row added to this screen that is *not* below the fold.
        // It sits under vsync, tenth of twenty-six, and the panel holds
        // twelve: so it arrives inside the panel and pushes AMBIENT
        // OCCLUSION out of the bottom of it. "RESOLUTION 100%" is
        // fourteen letters where "AMBIENT OCCLUSION 45%" was nineteen,
        // and a letter is a quad, which is the whole of the thirty
        // vertices that went. Every row under vsync moved down one, so
        // the shape and the colour hashes moved with them.
        ("settings", 2430, 4497453934474193077, 1115244357459885731),
        // More actions to bind than when this was taken, so more rows.
        // The last of them is GIVE (`keybinds::Action::Give`), which is
        // ninety more vertices -- a row's well, its word and its key --
        // and it shortens every row above it by a hair, which is what
        // moves the shape and the colour hashes as well.
        // ...and HIDE INTERFACE (`keybinds::Action::ToggleHud`, Tab), one
        // more row of the same ninety-odd vertices.
        // ...and WALK / GET OFF (`keybinds::Action::Rein`, C), the horse's
        // key, one more row again.
        // ...and then the screen stopped drawing all eighteen at once.
        // It is a window onto the list now, exactly as the settings
        // screen is (`ListRows`), so what it draws is the twelve rows
        // that fit at a readable height plus a scrollbar -- eight
        // hundred vertices fewer, and every one of the twelve a
        // full-height row rather than the strip the squeeze left. The
        // shape and the colour move with them.
        ("controls", 1884, 14124537724609837711, 8649509288934718163),
        // Shape only: BACK is the middle third of the panel (`columns`).
        ("credits", 648, 4653236891393317696, 3024564837030182243),
        // Shape only, as the credits.
        ("extensions", 378, 11642442880375712349, 15804250689672981859),
        // Colour only, here and on `failed`: `widgets::TEXT_BAD` is a
        // coral now, because the old red measured 1.7:1 on a menu panel.
        ("confirm", 450, 15266355557631834072, 14706216186102587939),
        ("connecting", 156, 3188982485829401401, 4459260551965797891),
        ("failed", 324, 9000926687394646930, 126400074752713907),
        ];

        for (name, screen) in golden_screens() {
            let (count, shape, colour) = fingerprint(screen);
            let (_, want_count, want_shape, want_colour) = GOLDEN
                .iter()
                .find(|(golden, _, _, _)| *golden == name)
                .unwrap_or_else(|| panic!("{name} has no golden"));
            assert_eq!(
                count, *want_count,
                "the {name} screen draws {count} vertices where it drew {want_count}",
            );
            assert_eq!(
                shape, *want_shape,
                "the {name} screen still draws {count} vertices, but not in the same places",
            );
            assert_eq!(
                colour, *want_colour,
                "the {name} screen is laid out as it was, but drawn in different colours",
            );
        }
    }

    #[test]
    fn the_desktop_settings_screen_is_laid_out_exactly_where_it_always_was() {
        // **The hard constraint on the whole layout pass.** This screen
        // was rewritten from a box of constants into something that asks
        // a `Layout` for every number, so that a phone could be given a
        // different one. The machine it was designed on must not have
        // noticed: what follows is the old constants, written out, and
        // it fails the moment an `at(..)` is put round something that
        // should have stayed a literal.
        let menu = settings_on(widgets::Layout::desktop());

        // The two buttons under the panel: `Rect::new(-0.62, -0.83,
        // -0.02, -0.73)` and its mirror.
        let done = hot_rect(&menu, &Action::Back).expect("a DONE button");
        assert!(near(done, Rect::new(0.02, -0.83, 0.62, -0.73)), "DONE at {done:?}");
        let controls = hot_rect(&menu, &Action::OpenControls).expect("a CONTROLS button");
        assert!(
            near(controls, Rect::new(-0.62, -0.83, -0.02, -0.73)),
            "CONTROLS at {controls:?}",
        );

        // The first stepper on the screen, which pins the panel's top
        // and right edges, the row height, the gap, the gutter and the
        // button column all at once: the row runs from `panel.y1 - 0.03
        // - 0.105`, and its buttons from `row.x1 - 0.28`.
        let minus = hot_rect(&menu, &Action::Tweak(Setting::Language, -1)).expect("a minus");
        assert!(
            near(minus, Rect::new(0.818, 0.637, 0.938, 0.718)),
            "the first minus button moved to {minus:?}",
        );

        // ...and the name field, which pins the second row and the one
        // widget on this screen that is not a button.
        let field = hot_rect(&menu, &Action::EditUsername).expect("a name field");
        // Written as the sum it is rather than as `0.318`, which clippy
        // reads as somebody's misspelling of one over pi.
        assert!(
            near(field, Rect::new(1.098 - 0.78, 0.518, 1.098 - 0.02, 0.599)),
            "the name field moved to {field:?}",
        );

        // Eleven rows fit the panel, and that has to stay true: it is
        // what the scrollbar's size and the wheel's step are both
        // measured against.
        assert_eq!(menu.settings_visible, 11);
    }

    #[test]
    fn a_bigger_interface_setting_makes_the_settings_rows_bigger() {
        // The user-visible bug all of this was for: the reading climbed
        // and nothing on the screen changed. What has to hold is that
        // every step of the setting is a step of the row you press --
        // on the phone's shape, which is the one it failed on.
        let phone = 2712.0 / 1220.0;
        let mut previous = 0.0;
        for requested in [1.0f32, 1.5, 2.0, 3.0] {
            let menu = settings_on(widgets::Layout::for_screen(phone, requested));
            let minus = hot_rect(&menu, &Action::Tweak(Setting::Language, -1))
                .expect("a minus button");
            assert!(
                minus.height() > previous * 1.15,
                "at {requested} the button is {} tall, barely past {previous}",
                minus.height(),
            );
            previous = minus.height();
        }
    }

    #[test]
    fn every_settings_row_stays_inside_its_panel_at_every_size() {
        // The failure the old fixed box produced on a phone: the
        // reading and the buttons past the right-hand edge, and the
        // confirm buttons past the bottom. Nothing this screen offers
        // to be pressed may sit off the glass, at any shape or size.
        for aspect in [16.0f32 / 9.0, 2712.0 / 1220.0, 4.0 / 3.0, 1.0] {
            for requested in [1.0f32, 1.5, 2.0, 4.0] {
                let layout = widgets::Layout::for_screen(aspect, requested);
                let menu = settings_on(layout);
                for (rect, action) in &menu.hot {
                    assert!(
                        rect.x0 >= -aspect - 1e-3 && rect.x1 <= aspect + 1e-3,
                        "at {aspect:.2}x{requested}, {action:?} is off the side at {rect:?}",
                    );
                    assert!(
                        rect.y0 >= -1.0 - 1e-3 && rect.y1 <= 1.0 + 1e-3,
                        "at {aspect:.2}x{requested}, {action:?} is off the top or bottom at {rect:?}",
                    );
                }
            }
        }
    }

    #[test]
    fn the_settings_screen_shows_fewer_rows_rather_than_smaller_ones() {
        // What gives when the rows grow and the screen does not: the
        // count. The alternative -- which is what the screen did -- is
        // rows that stay put and text that grows into them.
        let phone = 2712.0 / 1220.0;
        let small = settings_on(widgets::Layout::for_screen(phone, 1.0)).settings_visible;
        let large = settings_on(widgets::Layout::for_screen(phone, 2.0)).settings_visible;
        assert!(large < small, "{large} rows at 2.0 against {small} at 1.0");
        assert!(large >= 1, "a settings screen with no settings on it");
    }

    #[test]
    fn the_key_holds_still_when_the_menu_does() {
        // The whole point of the key: two frames of an untouched menu
        // must compare equal, or the change-driven rebuild degenerates
        // into the every-frame one it replaced.
        let fixture = Fixture::new();
        let menu = menu_with(3);
        assert_eq!(menu.ui_key(&fixture.ctx()), menu.ui_key(&fixture.ctx()));
    }

    #[test]
    fn what_the_player_does_to_the_menu_changes_the_key() {
        let fixture = Fixture::new();
        let mut menu = menu_with(3);
        let untouched = menu.ui_key(&fixture.ctx());

        menu.move_selection(1);
        let moved = menu.ui_key(&fixture.ctx());
        assert_ne!(untouched, moved, "moving the selection was invisible");

        menu.open(Screen::Settings);
        assert_ne!(moved, menu.ui_key(&fixture.ctx()), "changing screens was invisible");
    }

    #[test]
    fn a_changed_setting_changes_the_key() {
        // The settings rows draw their values, so a stepped setting with
        // an equal key would be a row showing the old number until
        // something else happened to move.
        let mut fixture = Fixture::new();
        let mut menu = menu_with(1);
        menu.open(Screen::Settings);
        let before = menu.ui_key(&fixture.ctx());
        fixture.settings.fov_degrees += 5.0;
        assert_ne!(before, menu.ui_key(&fixture.ctx()));
    }

    #[test]
    fn the_cursor_is_part_of_the_key() {
        // Hover highlights follow the cursor, so a moved mouse is a
        // changed menu.
        let fixture = Fixture::new();
        let mut menu = menu_with(2);
        let nowhere = menu.ui_key(&fixture.ctx());
        menu.set_cursor(Some((0.0, 0.0)));
        assert_ne!(nowhere, menu.ui_key(&fixture.ctx()));
    }

    fn menu_with(count: usize) -> Menu {
        let mut menu = Menu::new(ServerList {
            servers: (0..count)
                .map(|i| ServerEntry {
                    name: format!("Server {i}"),
                    address: format!("10.0.0.{i}:7878"),
                })
                .collect(),
        });
        menu.screen = Screen::Servers;
        menu
    }

    /// Places the cursor over a widget by building the screen and
    /// looking up where that widget landed -- the same table clicks use.
    fn point_at(menu: &mut Menu, action: &Action) -> bool {
        build(menu);
        if let Some((rect, _)) = menu.hot.iter().find(|(_, a)| a == action) {
            menu.cursor = Some((rect.centre_x(), rect.centre_y()));
            true
        } else {
            false
        }
    }

    #[test]
    fn selection_wraps_in_both_directions() {
        let mut menu = menu_with(3);
        menu.move_selection(-1);
        assert_eq!(menu.selected, 2, "up from the top wraps to the bottom");
        menu.move_selection(1);
        assert_eq!(menu.selected, 0);
    }

    #[test]
    fn an_empty_list_does_not_panic() {
        let mut menu = menu_with(0);
        menu.move_selection(1);
        assert!(menu.selected_entry().is_none());
        assert_eq!(menu.key(Key::Enter), None, "there is nothing to connect to");
        build(&mut menu);
    }

    #[test]
    fn the_main_menu_offers_singleplayer_multiplayer_and_quit() {
        let mut menu = Menu::new(ServerList::default());
        build(&mut menu);
        let actions: Vec<Action> = menu.hot.iter().map(|(_, a)| a.clone()).collect();
        assert!(actions.contains(&Action::OpenWorlds));
        assert!(actions.contains(&Action::OpenServers));
        assert!(actions.contains(&Action::Quit));
    }

    #[test]
    fn clicking_a_button_returns_its_action() {
        let mut menu = Menu::new(ServerList::default());
        assert!(point_at(&mut menu, &Action::OpenWorlds));
        assert_eq!(menu.click(), Some(Action::OpenWorlds));
    }

    #[test]
    fn clicking_outside_every_widget_does_nothing() {
        let mut menu = Menu::new(ServerList::default());
        build(&mut menu);
        menu.cursor = Some((5.0, -0.99));
        assert_eq!(menu.click(), None);
    }

    #[test]
    fn adding_a_server_stores_it_and_returns_to_the_list() {
        let mut menu = menu_with(1);
        menu.apply(Action::Add);
        assert_eq!(menu.screen, Screen::Editing(None));

        for c in "Friends".chars() {
            menu.type_char(c);
        }
        menu.focus = Field::Address;
        for c in "play.example.com:7878".chars() {
            menu.type_char(c);
        }
        menu.apply(Action::Save);

        assert_eq!(menu.screen, Screen::Servers);
        assert_eq!(menu.servers.servers.len(), 2);
        assert_eq!(menu.servers.servers[1].name, "Friends");
        assert_eq!(menu.servers.servers[1].address, "play.example.com:7878");
        assert_eq!(menu.selected, 1, "the new server should be selected");
    }

    #[test]
    fn a_bare_host_gets_the_default_port_rather_than_being_rejected() {
        let mut menu = menu_with(0);
        menu.apply(Action::Add);
        menu.focus = Field::Address;
        for c in "example.com".chars() {
            menu.type_char(c);
        }
        menu.apply(Action::Save);
        assert_eq!(menu.servers.servers[0].address, "example.com:7878");
    }

    #[test]
    fn a_server_with_no_name_is_labelled_with_its_address() {
        let mut menu = menu_with(0);
        menu.apply(Action::Add);
        menu.focus = Field::Address;
        for c in "1.2.3.4:9999".chars() {
            menu.type_char(c);
        }
        menu.apply(Action::Save);
        assert_eq!(menu.servers.servers[0].name, "1.2.3.4:9999");
    }

    #[test]
    fn an_empty_address_is_refused_and_focuses_the_field_that_is_wrong() {
        let mut menu = menu_with(0);
        menu.apply(Action::Add);
        for c in "No address".chars() {
            menu.type_char(c);
        }
        let result = menu.apply(Action::Save);

        assert_eq!(result, Action::Focus(Field::Address));
        assert_eq!(menu.screen, Screen::Editing(None), "the form should stay open");
        assert!(menu.servers.servers.is_empty());
        assert_eq!(menu.focus, Field::Address);
        assert!(menu.notice.as_ref().is_some_and(|(_, good)| !good));
    }

    #[test]
    fn editing_replaces_an_entry_instead_of_adding_one() {
        let mut menu = menu_with(3);
        menu.apply(Action::Edit(1));
        assert_eq!(menu.screen, Screen::Editing(Some(1)));
        assert_eq!(menu.name_input.text(), "Server 1", "the form should be pre-filled");

        menu.name_input.set_text("Renamed");
        menu.apply(Action::Save);

        assert_eq!(menu.servers.servers.len(), 3, "editing must not append");
        assert_eq!(menu.servers.servers[1].name, "Renamed");
        assert_eq!(menu.servers.servers[1].address, "10.0.0.1:7878");
    }

    #[test]
    fn deleting_keeps_the_selection_on_something_that_exists() {
        // Deleting the last row used to leave `selected` past the end.
        let mut menu = menu_with(3);
        menu.selected = 2;
        menu.apply(Action::Delete(2));
        assert_eq!(menu.servers.servers.len(), 2);
        assert!(menu.selected < menu.servers.servers.len());
        assert!(menu.selected_entry().is_some());
    }

    #[test]
    fn deleting_the_only_server_leaves_a_usable_screen() {
        let mut menu = menu_with(1);
        menu.apply(Action::Delete(0));
        assert!(menu.servers.servers.is_empty());
        assert!(menu.selected_entry().is_none());
        build(&mut menu);
    }


    /// **A world's row says what happened in it, not what its file is.**
    ///
    /// The row used to be a seed and "3 d ago", which answers neither of
    /// the questions this screen is opened with: which of these was I
    /// last in, and which is the one I got through the first winter in.
    #[test]
    fn a_worlds_row_says_the_day_the_season_the_size_and_when_it_was_played() {
        let world = worlds::World {
            name: "Дом".to_string(),
            seed: Some(4242),
            preset: Preset::Normal,
            zone: Zone::Temperate,
            scale: Scale::Landforms,
            world_time: Some(93.5),
            bytes: 3_300_000,
            directory: std::path::PathBuf::from("saves/dom"),
            // Two days before the `now` below.
            last_played: 1_735_689_600,
        };
        let line = world_detail(&world, 1_735_689_600 + 2 * 86_400, Language::Russian);
        // Day 94 of a world's own calendar is not day 94 of a year:
        // the calendar starts partway in (`season::WORLD_OPENS_ON_DAY`),
        // which is why the season is asked for rather than counted.
        let season = Language::Russian.text(season_name(primitive_shared::season::Season::at(93.5)));
        for want in ["дн. назад", "01.01.2025", "день 94", season, "3.1 MB", "сид 4242"] {
            assert!(line.contains(want), "the row does not say {want:?}: {line}");
        }
        // ...and the seed is last, because it is the first thing a
        // narrow row is allowed to lose.
        assert!(
            line.rfind("сид").unwrap() > line.rfind("3.1 MB").unwrap(),
            "the seed is not the last thing in the row: {line}"
        );
    }

    #[test]
    fn a_world_nobody_has_opened_says_so_and_stops() {
        // Rather than "day 1, spring, 4 kB", which is four facts and no
        // information about a folder that has never been entered.
        let world = worlds::World {
            name: "Fresh".to_string(),
            seed: None,
            preset: Preset::Normal,
            zone: Zone::Temperate,
            scale: Scale::Landforms,
            world_time: None,
            bytes: 0,
            directory: std::path::PathBuf::new(),
            last_played: 0,
        };
        assert_eq!(world_detail(&world, 9_999_999, Language::English), "never played");
    }

    /// **RENAME and COPY are pressed where they are drawn**, and the six
    /// buttons stand in the two rows the screen already had.
    #[test]
    fn the_world_list_offers_six_buttons_in_the_two_rows_it_always_had() {
        let mut menu = Menu::new(ServerList::default());
        menu.screen = Screen::Worlds;
        build_over_worlds(&mut menu, 2);
        let mut rows: Vec<f32> = Vec::new();
        for wanted in [
            Action::PlayWorld(0),
            Action::NewWorld,
            Action::AskDeleteWorld(0),
            Action::RenameWorld(0),
            Action::CopyWorld(0),
            Action::Back,
        ] {
            // **The last target, not the first.** The selected row of
            // the list is itself a `PlayWorld` -- click to choose, click
            // again to play -- so asking for the first one finds the row
            // rather than the button under the panel.
            let rect = menu
                .hot
                .iter()
                .rev()
                .find(|(_, action)| *action == wanted)
                .map(|(rect, _)| *rect)
                .unwrap_or_else(|| panic!("{wanted:?} is not on the worlds screen"));
            if !rows.iter().any(|y| (y - rect.y0).abs() < 1e-4) {
                rows.push(rect.y0);
            }
        }
        assert_eq!(rows.len(), 2, "the six buttons are not in two rows: {rows:?}");
    }

    #[test]
    fn nothing_that_acts_on_a_world_is_offered_when_there_are_none() {
        // The screen a fresh install opens on offers exactly the one
        // thing there is to do, and BACK.
        let mut menu = Menu::new(ServerList::default());
        menu.screen = Screen::Worlds;
        build_over_worlds(&mut menu, 0);
        for wanted in [
            Action::PlayWorld(0),
            Action::AskDeleteWorld(0),
            Action::RenameWorld(0),
            Action::CopyWorld(0),
        ] {
            assert!(hot_rect(&menu, &wanted).is_none(), "{wanted:?} is pressable with no worlds");
        }
        assert!(hot_rect(&menu, &Action::NewWorld).is_some());
        assert!(hot_rect(&menu, &Action::Back).is_some());
    }

    #[test]
    fn renaming_opens_a_form_that_can_be_typed_into_and_left() {
        let mut menu = Menu::new(ServerList::default());
        menu.screen = Screen::Worlds;
        menu.apply(Action::RenameWorld(0));
        assert_eq!(menu.screen, Screen::RenamingWorld(0));
        assert!(menu.accepts_text(), "the rename form refuses letters");
        for c in "Дом".chars() {
            menu.type_char(c);
        }
        assert_eq!(menu.name_input.text(), "Дом");
        build(&mut menu);
        assert!(hot_rect(&menu, &Action::CommitRename(0)).is_some());
        // Escape is the way out of every form on this menu, and it goes
        // back to the list rather than to the main menu.
        menu.key(Key::Escape);
        assert_eq!(menu.screen, Screen::Worlds);
    }

    #[test]
    fn an_empty_rename_cannot_be_pressed() {
        // The button is dark rather than the form refusing afterwards:
        // a control that is never right to press should not be pressable.
        let mut menu = Menu::new(ServerList::default());
        menu.screen = Screen::Worlds;
        menu.apply(Action::RenameWorld(0));
        build(&mut menu);
        assert!(hot_rect(&menu, &Action::CommitRename(0)).is_none());
        menu.type_char('A');
        build(&mut menu);
        assert!(hot_rect(&menu, &Action::CommitRename(0)).is_some());
    }

    /// **The line under the new-world form explains the row the player
    /// is on**, including the two that are typed into.
    ///
    /// Before this, tapping NAME or SEED left the help explaining a
    /// world type nobody was looking at, and the sentence saying what a
    /// seed *is* could only be reached by stepping a different row.
    #[test]
    fn the_help_under_the_new_world_form_follows_the_row_being_touched() {
        let mut menu = Menu::new(ServerList::default());
        menu.apply(Action::NewWorld);
        assert_eq!(menu.explaining, FormRow::Preset);
        menu.apply(Action::Focus(Field::Seed));
        assert_eq!(menu.explaining, FormRow::Seed);
        menu.apply(Action::StepZone(1));
        assert_eq!(menu.explaining, FormRow::Zone);
        menu.apply(Action::StepScale(1));
        assert_eq!(menu.explaining, FormRow::Scale);
        menu.apply(Action::Focus(Field::Name));
        assert_eq!(menu.explaining, FormRow::Name);
    }

    #[test]
    fn the_scale_row_steps_the_way_its_arrow_points_and_comes_back() {
        // Newest first: one press forward from the default is the
        // generator before it, not two versions of the world back.
        let mut menu = Menu::new(ServerList::default());
        menu.apply(Action::NewWorld);
        assert_eq!(menu.world_scale, Scale::default());
        menu.apply(Action::StepScale(1));
        assert_eq!(menu.world_scale, Scale::Earth);
        menu.apply(Action::StepScale(1));
        assert_eq!(menu.world_scale, Scale::Regional);
        menu.apply(Action::StepScale(1));
        assert_eq!(menu.world_scale, Scale::default(), "the list does not wrap");
        menu.apply(Action::StepScale(-1));
        assert_eq!(menu.world_scale, Scale::Regional, "back is not the inverse of forward");
    }

    #[test]
    fn a_new_form_forgets_the_last_worlds_choices() {
        // A player who once made an archipelago in the tropics should
        // not make every world after it out of tropical islands without
        // having chosen to.
        let mut menu = Menu::new(ServerList::default());
        menu.apply(Action::NewWorld);
        menu.apply(Action::StepScale(1));
        menu.apply(Action::StepZone(1));
        menu.apply(Action::Cancel);
        menu.apply(Action::NewWorld);
        assert_eq!(menu.world_scale, Scale::default());
        assert_eq!(menu.world_zone, Zone::Temperate);
    }

    #[test]
    fn every_key_the_world_lists_help_line_names_actually_does_something() {
        // The line under the list is the only place these are written
        // down, so a key named there and not bound is a lie printed on
        // the screen.
        let mut menu = Menu::new(ServerList::default());
        menu.screen = Screen::Worlds;
        menu.set_world_count(2);
        for (key, wanted) in [
            (Key::Char('n'), Action::NewWorld),
            (Key::Char('r'), Action::RenameWorld(0)),
            (Key::Char('c'), Action::CopyWorld(0)),
        ] {
            menu.screen = Screen::Worlds;
            menu.world_selected = 0;
            assert_eq!(menu.key(key), Some(wanted.clone()), "{key:?} does not do {wanted:?}");
        }
        menu.screen = Screen::Worlds;
        menu.world_selected = 1;
        assert_eq!(
            menu.key(Key::Delete { word: false }),
            Some(Action::AskDeleteWorld(1)),
        );
    }

    #[test]
    fn typing_is_limited_to_characters_the_font_can_draw() {
        let mut menu = menu_with(0);
        menu.apply(Action::Add);
        menu.type_char('A');
        menu.type_char('\u{2603}'); // a snowman
        menu.type_char('\n');
        menu.type_char('B');
        assert_eq!(menu.name_input.text(), "AB");
    }

    #[test]
    fn the_fields_accept_every_alphabet_the_font_speaks() {
        // The filter is "has a glyph", not "is ASCII": an interface
        // that says РУССКИЙ must let a world be named in it.
        let mut menu = menu_with(0);
        menu.apply(Action::Add);
        for c in "Дом ćma".chars() {
            menu.type_char(c);
        }
        assert_eq!(menu.name_input.text(), "Дом ćma");
    }

    #[test]
    fn a_field_cannot_grow_without_bound() {
        let mut menu = menu_with(0);
        menu.apply(Action::Add);
        for _ in 0..500 {
            menu.type_char('x');
        }
        assert_eq!(menu.name_input.chars(), MAX_NAME);
    }

    #[test]
    fn tab_moves_between_the_two_fields() {
        let mut menu = menu_with(0);
        menu.apply(Action::Add);
        assert_eq!(menu.focus, Field::Name);
        menu.key(Key::Tab);
        assert_eq!(menu.focus, Field::Address);
        menu.key(Key::Tab);
        assert_eq!(menu.focus, Field::Name);
    }

    #[test]
    fn backspace_deletes_from_the_focused_field_only() {
        let mut menu = menu_with(0);
        menu.apply(Action::Add);
        menu.name_input.set_text("abc");
        menu.address_input.set_text("xyz");
        menu.key(Key::Backspace { word: false });
        assert_eq!(menu.name_input.text(), "ab");
        assert_eq!(menu.address_input.text(), "xyz");
    }

    #[test]
    fn text_handed_in_by_an_input_method_obeys_the_rules_typing_does() {
        // The whole point of `set_focused_text` going through
        // `type_char`. An Android input method holds its own copy of
        // the field and hands the game the lot; if that arrived as a
        // plain assignment, every rule about what a field may contain
        // would apply to a desktop and not to a phone -- and the phone
        // is where an emoji keyboard is one tap away.
        let mut menu = menu_with(0);
        menu.apply(Action::Add);
        menu.set_focused_text("миру мир 🙂");
        assert_eq!(menu.name_input.text(), "миру мир ");
    }

    #[test]
    fn what_the_input_method_offered_and_what_the_field_kept_are_comparable() {
        // How the frame loop knows to push the field back at the input
        // method: it offered something, the field kept less, and the
        // two now disagree. Without this being observable the editor
        // would keep an emoji the game had thrown away, and the next
        // backspace would delete a character the player could not see.
        let mut menu = menu_with(0);
        menu.apply(Action::Add);
        let offered = "hi 🙂";
        menu.set_focused_text(offered);
        assert_ne!(menu.focused_text(), offered);

        // ...and agree when nothing was rejected, so the loop is not
        // pushing text back sixty times a second for no reason.
        let plain = "hi";
        menu.set_focused_text(plain);
        assert_eq!(menu.focused_text(), plain);
    }

    #[test]
    fn a_seed_field_still_takes_digits_only_from_an_input_method() {
        // The seed is parsed as a number when the world is made, so a
        // field that could hold letters would be a form that looks
        // filled in and is refused on submit. `type_char` is where that
        // rule lives, which is why the input method's text goes through
        // it too.
        let mut menu = menu_with(0);
        menu.screen = Screen::CreatingWorld;
        menu.focus = Field::Seed;
        menu.set_focused_text("12a34");
        assert_eq!(menu.seed_input.text(), "1234");
    }

    #[test]
    fn a_screen_with_no_field_reports_no_text_to_an_input_method() {
        // `focused_text` is asked once a frame, on every screen, and
        // the server list has a `name_input` left over from the last
        // form it showed. Reporting that would put a stale name into
        // the input method's buffer for a screen with nothing to type
        // into.
        let mut menu = menu_with(1);
        menu.name_input.set_text("left over");
        assert!(!menu.accepts_text());
        assert_eq!(menu.focused_text(), "");
    }

    #[test]
    fn typing_outside_the_form_is_ignored() {
        // Otherwise 'a' on the server list -- which is the "add" shortcut
        // -- would also end up in a field nobody is looking at.
        let mut menu = menu_with(1);
        menu.type_char('z');
        assert!(menu.name_input.is_empty());
    }

    #[test]
    fn a_second_click_on_the_selected_row_connects() {
        let mut menu = menu_with(3);
        menu.selected = 0;
        assert!(point_at(&mut menu, &Action::Select(2)));
        assert_eq!(menu.click(), Some(Action::Select(2)));
        assert_eq!(menu.selected, 2);
        // Now that it is selected, the same row is the play button.
        assert!(point_at(&mut menu, &Action::Connect(2)));
        assert_eq!(menu.click(), Some(Action::Connect(2)));
    }

    #[test]
    fn a_failure_shows_the_reason_and_offers_a_retry() {
        let mut menu = menu_with(2);
        menu.begin_connecting("Server 0".to_string());
        menu.fail("connection refused by 10.0.0.0:7878".to_string());
        match &menu.screen {
            Screen::Failed { label, reason } => {
                assert_eq!(label, "Server 0");
                assert!(reason.contains("refused"));
            }
            other => panic!("unexpected screen {other:?}"),
        }
        build(&mut menu);
        let actions: Vec<Action> = menu.hot.iter().map(|(_, a)| a.clone()).collect();
        assert!(actions.contains(&Action::Retry));
        assert!(actions.contains(&Action::Back));
        assert_eq!(menu.key(Key::Enter), Some(Action::Retry));
    }

    #[test]
    fn the_pause_screen_does_not_hide_the_world_completely() {
        // It is a pause, not a disconnect: the scrim has to be partly
        // transparent or the player loses their bearings.
        let mut menu = menu_with(1);
        menu.screen = Screen::Paused;
        let vertices = build(&mut menu);
        assert!(vertices[0].tint[3] < 0.9, "the pause scrim is opaque");
        assert!(vertices[0].tint[3] > 0.2, "the pause scrim is invisible");
    }

    #[test]
    fn escape_resumes_from_the_pause_screen() {
        let mut menu = menu_with(1);
        menu.screen = Screen::Paused;
        assert_eq!(menu.key(Key::Escape), Some(Action::Resume));
        let actions: Vec<Action> = {
            build(&mut menu);
            menu.hot.iter().map(|(_, a)| a.clone()).collect()
        };
        assert!(actions.contains(&Action::LeaveWorld));
    }

    #[test]
    fn every_screen_produces_geometry_and_a_way_out() {
        // A screen with no exit is a hang the player can only fix with
        // the window's close button.
        let screens = [
            Screen::Main,
            Screen::Worlds,
            Screen::CreatingWorld,
            Screen::Servers,
            Screen::Editing(None),
            Screen::Settings,
            Screen::Credits,
            Screen::Confirm {
                question: Msg::DeleteThisWorld,
                detail: "Home".into(),
                confirm_label: Msg::Delete,
                action: Box::new(Action::ConfirmedDeleteWorld(0)),
            },
            Screen::Connecting { label: "x".into() },
            Screen::Failed { label: "x".into(), reason: "y".into() },
            Screen::Paused,
        ];
        for screen in screens {
            let mut menu = menu_with(2);
            menu.screen = screen.clone();
            let vertices = build(&mut menu);
            assert!(!vertices.is_empty(), "{screen:?} drew nothing");
            assert!(
                !menu.hot.is_empty(),
                "{screen:?} has no clickable way out"
            );
        }
    }

    #[test]
    fn every_screen_survives_an_empty_and_an_awkward_state() {
        // Layout code indexes into lists and slices strings; the states
        // that break it are the empty list and the over-long value, and
        // neither is reachable from the happy path a person tests by
        // hand.
        let fixture = WorldFixture::new(&[]);
        let long = "W".repeat(200);
        let screens = [
            Screen::Worlds,
            Screen::CreatingWorld,
            Screen::Servers,
            Screen::Settings,
            Screen::Confirm {
                question: Msg::DeleteThisWorld,
                detail: long.clone(),
                confirm_label: Msg::Delete,
                action: Box::new(Action::Cancel),
            },
            Screen::Failed {
                label: long.clone(),
                reason: long.clone(),
            },
        ];
        for screen in screens {
            let mut menu = Menu::new(ServerList::default());
            menu.screen = screen.clone();
            menu.name_input.set_text(long.clone());
            menu.address_input.set_text(long.clone());
            menu.seed_input.set_text("9".repeat(10));
            menu.editing_username = true;
            // Selections deliberately past the end of both lists.
            menu.selected = 99;
            menu.world_selected = 99;
            assert!(
                !menu.build(&fixture.ctx()).is_empty(),
                "{screen:?} drew nothing"
            );
        }
    }

    #[test]
    fn the_server_file_round_trips() {
        let list = ServerList {
            servers: vec![ServerEntry {
                name: "Local server".to_string(),
                address: "127.0.0.1:7878".to_string(),
            }],
        };
        let text = toml::to_string_pretty(&list).unwrap();
        let parsed: ServerList = toml::from_str(&text).unwrap();
        assert_eq!(parsed.servers.len(), 1);
        assert_eq!(parsed.servers[0].address, list.servers[0].address);
    }

    #[test]
    fn an_empty_server_file_is_valid_rather_than_an_error() {
        // Deleting every server in the UI writes exactly this, and it
        // has to be readable next time.
        let parsed: ServerList = toml::from_str("servers = []").unwrap();
        assert!(parsed.servers.is_empty());
    }

    #[test]
    fn arrow_keys_walk_the_main_menu_and_enter_presses() {
        let mut menu = Menu::new(ServerList::default());
        assert_eq!(menu.key(Key::Enter), None, "nothing is focused yet");
        menu.key(Key::Down);
        assert_eq!(menu.key(Key::Enter), Some(Action::OpenWorlds));
    }

    #[test]
    fn moving_the_mouse_takes_the_highlight_from_the_keyboard() {
        // Two highlights at once leaves the player unsure which one
        // Enter would press.
        let mut menu = Menu::new(ServerList::default());
        menu.key(Key::Down);
        assert!(menu.button_focus.is_some());
        menu.set_cursor(Some((0.0, 0.0)));
        assert!(menu.button_focus.is_none());
    }

    #[test]
    fn changing_screen_clears_the_keyboard_highlight() {
        // Regression: the pause screen used to open with its third
        // button lit because the main menu had been left on index 2.
        let mut menu = Menu::new(ServerList::default());
        // Derived, not written down: a hardcoded index here breaks
        // every time a menu entry is added, which says nothing about
        // the behaviour the test is actually for.
        let last = menu.focus_actions().len() - 1;
        menu.key(Key::Up); // wraps to the last entry
        assert_eq!(menu.button_focus, Some(last));
        menu.apply(Action::OpenServers);
        assert_eq!(menu.button_focus, None);
    }

    // --- worlds ---

    /// A fixture with real worlds in a temp folder, since the world
    /// screens are about a list that exists on disk.
    struct WorldFixture {
        settings: ClientSettings,
        worlds: Worlds,
        root: std::path::PathBuf,
    }

    impl WorldFixture {
        fn new(names: &[&str]) -> Self {
            let root = std::env::temp_dir().join(format!(
                "primitive-menu-worlds-{}-{:?}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir_all(&root).unwrap();
            let mut worlds = Worlds::load(&root);
            for (i, name) in names.iter().enumerate() {
                worlds.create(name, 100 + i as u32, Preset::Normal).unwrap();
            }
            Self {
                settings: ClientSettings::default(),
                worlds,
                root,
            }
        }

        fn ctx(&self) -> MenuContext<'_> {
            MenuContext {
                version: "test",
                font: crate::engine::texture::FontAtlas::for_test(),
                settings: &self.settings,
                worlds: &self.worlds,
                background: Backdrop::Bare,
                layout: widgets::Layout::desktop(),
            }
        }
    }

    impl Drop for WorldFixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn singleplayer_opens_the_world_list_rather_than_a_world() {
        // There is more than one world now, so the main menu cannot go
        // straight into one.
        let mut menu = Menu::new(ServerList::default());
        assert!(point_at(&mut menu, &Action::OpenWorlds));
        assert_eq!(menu.click(), Some(Action::OpenWorlds));
        assert_eq!(menu.screen, Screen::Worlds);
    }

    /// Names enough to overflow whatever the panel has room for.
    fn many_names(count: usize) -> Vec<String> {
        (0..count).map(|i| format!("World {i}")).collect()
    }

    /// Builds the world screen once and reports how many rows fit.
    fn open_worlds(menu: &mut Menu, fixture: &WorldFixture) -> usize {
        menu.screen = Screen::Worlds;
        menu.build(&fixture.ctx());
        menu.world_visible
    }

    #[test]
    fn a_list_longer_than_the_panel_scrolls_with_the_wheel() {
        // The wheel used to do nothing at all on a menu -- the handler
        // returned before it looked at what was on screen -- so the only
        // way down a long list was the arrow keys.
        let names = many_names(40);
        let fixture = WorldFixture::new(&names.iter().map(String::as_str).collect::<Vec<_>>());
        let mut menu = Menu::new(ServerList::default());
        let visible = open_worlds(&mut menu, &fixture);
        assert!(visible < 40, "the fixture has to overflow the panel");
        assert_eq!(menu.world_scroll, 0);

        menu.scroll(3);
        assert_eq!(menu.world_scroll, 3, "the wheel did not move the list");
        // ...and it did not drag the selection along with it. Scrolling
        // past the world you meant to open and then pressing Enter must
        // not load a different one.
        assert_eq!(menu.world_selected, 0);

        menu.scroll(-10);
        assert_eq!(menu.world_scroll, 0, "it scrolled off the top");
    }

    #[test]
    fn the_list_cannot_be_scrolled_past_its_own_end() {
        // A page of blank rows below the last world is how a player
        // concludes that the one they are looking for failed to load.
        let names = many_names(12);
        let fixture = WorldFixture::new(&names.iter().map(String::as_str).collect::<Vec<_>>());
        let mut menu = Menu::new(ServerList::default());
        let visible = open_worlds(&mut menu, &fixture);

        menu.scroll(1000);
        assert_eq!(menu.world_scroll, 12 - visible, "overscrolled past the end");
        menu.build(&fixture.ctx());
        assert_eq!(menu.world_scroll, 12 - visible, "the rebuild moved it");
    }

    #[test]
    fn a_short_list_does_not_scroll_at_all() {
        let fixture = WorldFixture::new(&["Alpha", "Beta"]);
        let mut menu = Menu::new(ServerList::default());
        open_worlds(&mut menu, &fixture);
        menu.scroll(5);
        assert_eq!(menu.world_scroll, 0, "two worlds should not scroll");
    }

    #[test]
    fn the_selection_stays_on_screen_without_dragging_the_list_about() {
        // The old rule was `first = selected - (visible - 1)`, which
        // pins the highlighted row to the bottom line: the list moved on
        // *every* press and the rows slid under a stationary highlight.
        // Only a selection that has left the window may move it, and
        // only by enough to bring it back.
        let names = many_names(30);
        let fixture = WorldFixture::new(&names.iter().map(String::as_str).collect::<Vec<_>>());
        let mut menu = Menu::new(ServerList::default());
        let visible = open_worlds(&mut menu, &fixture);

        // Down the window: the highlight moves, the list does not.
        for _ in 1..visible {
            menu.move_world_selection(1);
            assert_eq!(menu.world_scroll, 0, "the list moved while there was room");
        }
        // One more, and it follows by exactly one row.
        menu.move_world_selection(1);
        assert_eq!(menu.world_scroll, 1);

        // Wrapping round to the end brings the end into view.
        menu.world_selected = 0;
        menu.move_world_selection(-1);
        assert_eq!(menu.world_selected, 29);
        assert!(
            menu.world_scroll + visible > 29,
            "the last row is off screen at scroll {}",
            menu.world_scroll
        );
    }

    #[test]
    fn deleting_a_world_does_not_leave_the_list_below_its_end() {
        let names = many_names(20);
        let fixture = WorldFixture::new(&names.iter().map(String::as_str).collect::<Vec<_>>());
        let mut menu = Menu::new(ServerList::default());
        open_worlds(&mut menu, &fixture);
        menu.scroll(1000);
        let scrolled = menu.world_scroll;
        assert!(scrolled > 0);

        // The list is suddenly short. Nothing here deletes worlds from
        // disk; what matters is that the count the screen is told about
        // is the one the scroll is clamped against.
        menu.set_world_count(3);
        assert_eq!(menu.world_scroll, 0);
    }

    #[test]
    fn every_world_gets_a_row_and_the_selected_one_plays() {
        let fixture = WorldFixture::new(&["Alpha", "Beta"]);
        let mut menu = Menu::new(ServerList::default());
        menu.screen = Screen::Worlds;
        menu.build(&fixture.ctx());

        let actions: Vec<Action> = menu.hot.iter().map(|(_, a)| a.clone()).collect();
        assert!(actions.contains(&Action::NewWorld));
        // The selected row is the play button; the other is a select.
        assert!(actions.contains(&Action::PlayWorld(0)));
        assert!(actions.contains(&Action::SelectWorld(1)));
    }

    #[test]
    fn the_new_world_form_takes_a_name_and_a_numeric_seed() {
        let mut menu = Menu::new(ServerList::default());
        menu.apply(Action::NewWorld);
        assert_eq!(menu.screen, Screen::CreatingWorld);

        for c in "Home".chars() {
            menu.type_char(c);
        }
        menu.key(Key::Tab);
        assert_eq!(menu.focus, Field::Seed);
        // Letters must not reach a field that has to parse as a number.
        for c in "12a3".chars() {
            menu.type_char(c);
        }
        assert_eq!(menu.name_input.text(), "Home");
        assert_eq!(menu.seed_input.text(), "123");
    }

    #[test]
    fn the_form_offers_a_world_type_and_steps_through_it() {
        // The row has to be reachable with the mouse and it has to come
        // back to where it started: every preset once and a step of one,
        // so a player who clicks round is where they were rather than one
        // world type further on for ever. Two again: the branching trees
        // were a third stop until they became the ordinary world.
        let mut menu = Menu::new(ServerList::default());
        menu.apply(Action::NewWorld);
        assert_eq!(menu.world_preset, Preset::Normal);

        let fixture = Fixture::new();
        menu.build(&fixture.ctx());
        assert!(
            menu.hot.iter().any(|(_, a)| *a == Action::StepPreset(1)),
            "the form has no control for the world type"
        );

        menu.apply(Action::StepPreset(1));
        assert_eq!(menu.world_preset, Preset::Test);
        menu.apply(Action::StepPreset(1));
        assert_eq!(menu.world_preset, Preset::Normal);
        menu.apply(Action::StepPreset(-1));
        assert_eq!(menu.world_preset, Preset::Test);
    }

    #[test]
    fn the_form_lays_a_new_world_in_the_temperate_zone_unless_told_otherwise() {
        // The zone is the one choice a player cannot take back by walking,
        // so the default is the world the game was built in, the control
        // is on the form where the player can see it, it steps round every
        // zone and back, and opening the form again forgets the last one.
        let mut menu = Menu::new(ServerList::default());
        menu.apply(Action::NewWorld);
        assert_eq!(menu.world_zone, Zone::Temperate);

        let fixture = Fixture::new();
        menu.build(&fixture.ctx());
        assert!(
            menu.hot.iter().any(|(_, a)| *a == Action::StepZone(1)),
            "the form has no control for where the world is laid"
        );

        for _ in 0..Zone::ALL.len() {
            menu.apply(Action::StepZone(1));
        }
        assert_eq!(menu.world_zone, Zone::Temperate, "stepping round every zone did not come back");
        menu.apply(Action::StepZone(-1));
        assert_eq!(menu.world_zone, Zone::DryBelt);
        menu.apply(Action::Cancel);
        menu.apply(Action::NewWorld);
        assert_eq!(menu.world_zone, Zone::Temperate);
    }

    #[test]
    fn opening_the_form_forgets_the_last_world_type() {
        // Everything else on this form is cleared when it opens -- the
        // name and the seed -- and the type has to go with them, or a
        // player who once made a test world quietly makes another one
        // every time afterwards.
        let mut menu = Menu::new(ServerList::default());
        menu.apply(Action::NewWorld);
        menu.apply(Action::StepPreset(1));
        assert_eq!(menu.world_preset, Preset::Test);
        menu.apply(Action::Cancel);
        menu.apply(Action::NewWorld);
        assert_eq!(menu.world_preset, Preset::Normal);
    }

    #[test]
    fn a_seed_cannot_be_longer_than_a_u32() {
        let mut menu = Menu::new(ServerList::default());
        menu.apply(Action::NewWorld);
        menu.focus = Field::Seed;
        for _ in 0..40 {
            menu.type_char('9');
        }
        assert!(menu.seed_input.text().parse::<u64>().unwrap() > 0);
        assert!(menu.seed_input.chars() <= MAX_SEED_DIGITS);
    }

    #[test]
    fn deleting_a_world_goes_through_a_confirmation_first() {
        // Deleting a world removes a folder tree. One misclick must not
        // be enough.
        let mut menu = Menu::new(ServerList::default());
        menu.screen = Screen::Worlds;
        let action = menu.apply(Action::AskDeleteWorld(0));

        assert_eq!(action, Action::AskDeleteWorld(0));
        match &menu.screen {
            Screen::Confirm { action, .. } => {
                assert_eq!(**action, Action::ConfirmedDeleteWorld(0));
            }
            other => panic!("expected a confirmation, got {other:?}"),
        }
    }

    #[test]
    fn the_confirmation_screen_does_not_bind_enter_to_the_destructive_answer() {
        // Enter is what a player presses without reading. It must not
        // be what deletes their world.
        let mut menu = Menu::new(ServerList::default());
        menu.apply(Action::AskDeleteWorld(0));
        assert_eq!(menu.key(Key::Enter), None);
        assert!(matches!(menu.screen, Screen::Confirm { .. }), "it acted anyway");

        assert_eq!(menu.key(Key::Escape), Some(Action::Cancel));
        assert_eq!(menu.screen, Screen::Worlds);
    }

    #[test]
    fn the_confirmation_screen_names_what_it_will_delete() {
        let mut menu = Menu::new(ServerList::default());
        menu.apply(Action::AskDeleteWorld(3));
        menu.set_confirm_detail("Doomed".to_string());
        match &menu.screen {
            Screen::Confirm { detail, .. } => assert_eq!(detail, "Doomed"),
            other => panic!("unexpected screen {other:?}"),
        }
    }

    #[test]
    fn confirming_returns_the_action_that_was_asked_about() {
        let mut menu = Menu::new(ServerList::default());
        menu.apply(Action::AskDeleteWorld(2));
        // Not index 0: the gate must carry the index it was opened with.
        assert_eq!(menu.key(Key::Char('y')), Some(Action::ConfirmedDeleteWorld(2)));
        assert_eq!(menu.screen, Screen::Worlds);
    }

    // --- settings ---

    #[test]
    fn every_setting_is_reachable_by_scrolling() {
        // The screen is a window onto the list now, so no single frame
        // shows every row -- but every row must be shown by *some*
        // scroll position, and the scroll must be able to reach it.
        let mut menu = Menu::new(ServerList::default());
        menu.screen = Screen::Settings;
        let mut fixture = Fixture::new();
        // With the wallpaper off its block choice is deliberately
        // inert, so turn it on to see every control.
        fixture.settings.menu_background = true;

        let mut reachable = Vec::new();
        loop {
            menu.build(&fixture.ctx());
            for setting in Setting::ALL {
                let has_control = menu.hot.iter().any(|(_, a)| match a {
                    Action::Tweak(s, _) => *s == setting,
                    Action::EditUsername | Action::CommitUsername => setting.is_text(),
                    _ => false,
                });
                if has_control && !reachable.contains(&setting) {
                    reachable.push(setting);
                }
            }
            let before = menu.settings_scroll;
            menu.scroll_settings(1);
            if menu.settings_scroll == before {
                break;
            }
        }
        for setting in Setting::ALL {
            assert!(
                reachable.contains(&setting),
                "{:?} has no way to change it at any scroll position",
                setting
            );
        }
    }

    #[test]
    fn the_settings_list_scrolls_and_stops_at_both_ends() {
        let mut menu = Menu::new(ServerList::default());
        menu.screen = Screen::Settings;
        let fixture = Fixture::new();
        menu.build(&fixture.ctx());

        // Something must be off screen: the screen exists because the
        // list outgrew one panel.
        assert!(
            menu.settings_visible < Setting::ALL.len(),
            "every row fits; the scrollbar has nothing to do"
        );

        menu.scroll_settings(-5);
        assert_eq!(menu.settings_scroll, 0, "scrolled above the first row");
        menu.scroll_settings(1000);
        assert_eq!(
            menu.settings_scroll,
            Setting::ALL.len() - menu.settings_visible,
            "scrolled past the last row"
        );
    }

    #[test]
    fn opening_the_settings_starts_at_the_top_of_the_list() {
        let mut menu = Menu::new(ServerList::default());
        menu.screen = Screen::Settings;
        let fixture = Fixture::new();
        menu.build(&fixture.ctx());
        menu.scroll_settings(1000);
        assert_ne!(menu.settings_scroll, 0);
        menu.apply(Action::Back);
        menu.apply(Action::OpenSettings);
        assert_eq!(menu.settings_scroll, 0);
    }

    #[test]
    fn the_resolution_row_walks_from_automatic_up_to_every_pixel_and_back() {
        let mut settings = ClientSettings::default();
        // Wherever the platform starts it, held left it arrives at the
        // game's own choice and stays there: AUTO is the bottom of the
        // row, not a state you can only leave.
        for _ in 0..50 {
            Setting::Resolution.step(&mut settings, -1);
        }
        assert_eq!(settings.resolution_scale, None);
        assert_eq!(
            Setting::Resolution.value(&settings),
            settings.language.text(Msg::Auto),
        );

        // One step right off AUTO is the lowest share, not a jump back
        // to full size -- a player stepping away from the automatic
        // choice is looking for the neighbouring step.
        Setting::Resolution.step(&mut settings, 1);
        assert_eq!(settings.resolution_scale, Some(0.6));

        for _ in 0..50 {
            Setting::Resolution.step(&mut settings, 1);
        }
        assert_eq!(settings.resolution_scale, Some(1.0));
        assert_eq!(Setting::Resolution.value(&settings), "100%");
    }

    #[test]
    fn a_hand_edited_resolution_steps_to_the_share_beside_it_rather_than_to_an_end() {
        // A file saying 0.73 is between two steps. The row has to treat
        // it as the nearer one, or a single press throws away a value
        // the player typed on purpose.
        let hand_edited = || ClientSettings {
            resolution_scale: Some(0.73),
            ..ClientSettings::default()
        };
        let mut settings = hand_edited();
        Setting::Resolution.step(&mut settings, 1);
        assert_eq!(settings.resolution_scale, Some(0.8));
        let mut settings = hand_edited();
        Setting::Resolution.step(&mut settings, -1);
        assert_eq!(settings.resolution_scale, Some(0.7));
    }

    #[test]
    fn stepping_a_setting_changes_it_and_stays_within_its_limits() {
        let mut settings = ClientSettings::default();
        let before = settings.render_distance_chunks;
        Setting::RenderDistance.step(&mut settings, 1);
        assert_eq!(settings.render_distance_chunks, before + 1);

        // Held down at the edge, it stops rather than going nonsensical.
        for _ in 0..200 {
            Setting::RenderDistance.step(&mut settings, 1);
        }
        assert!(settings.render_distance_chunks <= 24);
        for _ in 0..500 {
            Setting::RenderDistance.step(&mut settings, -1);
        }
        assert!(settings.render_distance_chunks >= 1);
    }

    /// A name typed into the settings survives leaving the screen.
    ///
    /// **Pressing Enter used to be the only way it ever reached the
    /// settings**, and Enter is a key -- so on a phone, where the
    /// keyboard is dismissed by tapping away or by the back gesture,
    /// the name was typed, the screen was left the ordinary way, and
    /// nothing was saved. Reported from a device as "the username is
    /// not saved".
    #[test]
    fn a_name_typed_into_the_settings_survives_leaving_the_screen() {
        let mut menu = Menu::new(ServerList::default());
        menu.apply(Action::OpenSettings);
        menu.editing_username = true;
        menu.name_input.set_text("borya");

        // Left the ordinary way -- no Enter anywhere.
        menu.apply(Action::Back);
        assert_eq!(
            menu.take_typed_username().as_deref(),
            Some("borya"),
            "the name typed into the field did not survive leaving the screen",
        );
        // Drained, not merely read: a second save must not re-apply a
        // name the player has since changed by other means.
        assert_eq!(menu.take_typed_username(), None, "the name was handed over twice");
    }

    /// ...and the two ways of saying "never mind" still mean it.
    #[test]
    fn cancelling_a_name_edit_throws_the_typing_away() {
        for abandon in [Action::Cancel, Action::OpenSettings] {
            let mut menu = Menu::new(ServerList::default());
            menu.apply(Action::OpenSettings);
            menu.editing_username = true;
            menu.name_input.set_text("typed and regretted");
            menu.apply(abandon.clone());
            assert_eq!(
                menu.take_typed_username(),
                None,
                "{abandon:?} kept a name the player abandoned",
            );
        }

        // Escape, inside the field itself.
        let mut menu = Menu::new(ServerList::default());
        menu.apply(Action::OpenSettings);
        menu.editing_username = true;
        menu.name_input.set_text("typed and regretted");
        menu.key(Key::Escape);
        assert!(!menu.editing_username, "escape did not leave the field");
        assert_eq!(menu.take_typed_username(), None, "escape kept the typing");
    }

    #[test]
    fn toggles_flip_regardless_of_which_button_was_pressed() {
        // A switch has no "less" and "more", so both directions have to
        // mean the same thing -- otherwise one of them looks broken.
        let mut settings = ClientSettings::default();
        let before = settings.vsync;
        Setting::Vsync.step(&mut settings, 1);
        assert_ne!(settings.vsync, before);
        Setting::Vsync.step(&mut settings, -1);
        assert_eq!(settings.vsync, before);
    }

    #[test]
    fn the_leaf_and_stone_distance_rows_are_pressed_where_they_are_drawn_and_walk_their_stops() {
        // Both rows were added below the first panel's worth, so each is
        // scrolled to, and pressed through `click` -- the hit test a
        // player's finger goes through -- rather than by calling `step`,
        // which would pass however far the button had drifted from its
        // picture.
        let fixture = Fixture::new();
        for (row, plus) in [(Setting::TransparentLeaves, true), (Setting::ReliefDistance, true), (Setting::TransparentLeaves, false)] {
            let mut menu = Menu::new(ServerList::default());
            menu.screen = Screen::Settings;
            let wanted = Action::Tweak(row, if plus { 1 } else { -1 });
            let rect = loop {
                menu.build(&fixture.ctx());
                if let Some(rect) = hot_rect(&menu, &wanted) {
                    break rect;
                }
                let before = menu.settings_scroll;
                menu.scroll_settings(1);
                assert_ne!(menu.settings_scroll, before, "no scroll position shows the {row:?} row");
            };
            menu.cursor = Some((rect.centre_x(), rect.centre_y()));
            assert_eq!(menu.click(), Some(wanted), "pressing the middle of {row:?}'s button pressed something else");
        }

        // The leaves walk from solid everywhere to see-through everywhere
        // and stop at both ends, showing words at the ends and chunks
        // between.
        let mut settings = fixture.settings.clone();
        settings.transparent_leaves_chunks = 0;
        assert_eq!(Setting::TransparentLeaves.value(&settings), settings.language.text(Msg::Off));
        Setting::TransparentLeaves.step(&mut settings, -1);
        assert_eq!(settings.transparent_leaves_chunks, 0, "stepped below solid everywhere");
        let mut shown = Vec::new();
        for _ in 0..crate::settings::TRANSPARENT_LEAVES_STOPS.len() + 2 {
            Setting::TransparentLeaves.step(&mut settings, 1);
            shown.push(settings.transparent_leaves_chunks);
        }
        assert_eq!(settings.transparent_leaves_chunks, crate::engine::lod::LEAVES_SEE_THROUGH_EVERYWHERE);
        assert_eq!(Setting::TransparentLeaves.value(&settings), settings.language.text(Msg::Everywhere));
        assert!(shown.windows(2).all(|w| w[0] <= w[1]), "the row went backwards: {shown:?}");
        settings.transparent_leaves_chunks = 6;
        assert_eq!(Setting::TransparentLeaves.value(&settings), format!("6 {}", settings.language.text(Msg::Chunks)));

        // The stones reach "off", which is flat everywhere.
        settings.relief_chunks = crate::engine::lod::RELIEF_CHUNKS;
        for _ in 0..crate::settings::RELIEF_STOPS.len() {
            Setting::ReliefDistance.step(&mut settings, -1);
        }
        assert_eq!(settings.relief_chunks, 0);
        assert_eq!(Setting::ReliefDistance.value(&settings), settings.language.text(Msg::Off));
    }

    #[test]
    fn the_shadows_row_is_pressed_where_it_is_drawn_and_turns_them_on() {
        // The row is below the first panel's worth, so it has to be
        // scrolled to -- and a control that is recorded somewhere other
        // than where its row is drawn is exactly the fault a click at
        // its own centre would miss, so the click goes through `click`,
        // the same hit test a player's finger does.
        let mut menu = Menu::new(ServerList::default());
        menu.screen = Screen::Settings;
        let fixture = Fixture::new();
        let mut settings = fixture.settings.clone();
        assert!(!settings.shadows.is_on(), "the fixture starts with shadows on");
        let rect = loop {
            menu.build(&fixture.ctx());
            let found = menu
                .hot
                .iter()
                .find(|(_, action)| matches!(action, Action::Tweak(Setting::Shadows, _)))
                .map(|(rect, _)| *rect);
            if let Some(rect) = found {
                break rect;
            }
            let before = menu.settings_scroll;
            menu.scroll_settings(1);
            assert_ne!(menu.settings_scroll, before, "no scroll position shows the shadows row");
        };
        menu.cursor = Some((rect.centre_x(), rect.centre_y()));
        let Some(Action::Tweak(Setting::Shadows, delta)) = menu.click() else {
            panic!("pressing the middle of the shadows switch did not press it");
        };
        Setting::Shadows.step(&mut settings, delta);
        assert_eq!(settings.shadows, crate::engine::shadow::Mode::Hard, "pressing the switch did not turn hard shadows on");
        assert_eq!(Setting::Shadows.value(&settings), settings.language.text(Msg::ShadowsHard));
        // Round the three steps and back, not stuck at the last one.
        Setting::Shadows.step(&mut settings, delta);
        assert_eq!(settings.shadows, crate::engine::shadow::Mode::Soft);
        Setting::Shadows.step(&mut settings, delta);
        assert!(!settings.shadows.is_on(), "the third press did not turn shadows off again");
    }

    #[test]
    fn every_setting_shows_a_value_a_person_can_read() {
        // **Every language, and the font's own list.** This asked
        // `is_ascii_graphic`, which was the right question while the font
        // was ASCII and has been the wrong one ever since it learned
        // Cyrillic, Polish and the punctuation a Russian keyboard types
        // (`font::ORDER`) -- it would have called `ДАЛЬНОСТЬ` undrawable
        // and it did call the degree sign undrawable, which the font has
        // had for as long as the chat has been able to print `№`. And it
        // only ever looked at the default settings, which are English:
        // the rows whose reading is a word are exactly the rows a
        // translation can break.
        let mut settings = ClientSettings::default();
        for language in Language::ALL.iter().copied() {
            settings.language = language;
            for setting in Setting::ALL {
                let value = setting.value(&settings);
                assert!(!value.is_empty(), "{setting:?} shows nothing in {language:?}");
                assert!(
                    value.chars().all(|c| crate::engine::texture::GLYPHS.contains(c)),
                    "{setting:?} shows {value:?} in {language:?}, which the font cannot draw"
                );
            }
        }
    }

    #[test]
    fn sensitivity_is_shown_at_a_scale_where_a_step_is_visible() {
        // Stored around 0.0025; a row reading "0.003" would not change
        // when stepped.
        let mut settings = ClientSettings::default();
        let before = Setting::Sensitivity.value(&settings);
        Setting::Sensitivity.step(&mut settings, 1);
        assert_ne!(before, Setting::Sensitivity.value(&settings));
    }

    #[test]
    fn typing_a_name_only_reaches_the_field_while_it_is_being_edited() {
        let mut menu = Menu::new(ServerList::default());
        menu.screen = Screen::Settings;
        menu.type_char('x');
        assert!(menu.name_input.is_empty(), "typed into a field nobody opened");

        menu.begin_username_edit("player".to_string());
        menu.type_char('!');
        assert_eq!(menu.name_input.text(), "player!");
        assert_eq!(menu.key(Key::Enter), Some(Action::CommitUsername));
        assert!(!menu.editing_username);
    }

    #[test]
    fn abandoning_a_name_edit_does_not_commit_it() {
        let mut menu = Menu::new(ServerList::default());
        menu.screen = Screen::Settings;
        menu.begin_username_edit("player".to_string());
        menu.type_char('z');
        assert_eq!(menu.key(Key::Escape), None, "escape must not commit");
        assert!(!menu.editing_username);
    }

    #[test]
    fn settings_are_reachable_from_the_main_menu_and_from_the_pause_screen() {
        for screen in [Screen::Main, Screen::Paused] {
            let mut menu = Menu::new(ServerList::default());
            menu.screen = screen.clone();
            build(&mut menu);
            let actions: Vec<Action> = menu.hot.iter().map(|(_, a)| a.clone()).collect();
            assert!(
                actions.contains(&Action::OpenSettings),
                "{screen:?} has no way into the settings"
            );
        }
    }

    #[test]
    fn leaving_the_settings_returns_to_where_they_were_opened_from() {
        // Opened from the pause screen, DONE has to go back to the pause
        // screen -- dropping the player on the main menu would look like
        // they had been disconnected.
        let mut menu = Menu::new(ServerList::default());
        menu.screen = Screen::Paused;
        menu.apply(Action::OpenSettings);
        assert_eq!(menu.screen, Screen::Settings);
        menu.apply(Action::Back);
        assert_eq!(menu.screen, Screen::Paused);
    }

    #[test]
    fn up_on_a_fresh_menu_selects_the_last_entry_and_down_the_first() {
        // Regression: "nothing selected" was encoded as index -1 and run
        // through the wrapping arithmetic, which put Up one short of the
        // end.
        let mut menu = Menu::new(ServerList::default());
        let last = menu.focus_actions().len() - 1;
        menu.key(Key::Up);
        assert_eq!(menu.button_focus, Some(last), "Up should reach the last entry");

        let mut menu = Menu::new(ServerList::default());
        menu.key(Key::Down);
        assert_eq!(menu.button_focus, Some(0));
    }

    #[test]
    fn the_main_menu_has_a_way_into_the_credits() {
        let mut menu = Menu::new(ServerList::default());
        assert!(point_at(&mut menu, &Action::OpenCredits));
        assert_eq!(menu.click(), Some(Action::OpenCredits));
        assert_eq!(menu.screen, Screen::Credits);
    }

    #[test]
    fn the_credits_screen_names_everyone_and_says_what_they_did() {
        let mut menu = Menu::new(ServerList::default());
        menu.screen = Screen::Credits;
        let vertices = build(&mut menu);
        assert!(!vertices.is_empty());
        // A role with no name, or a name with no role, is not a credit.
        for (role, who) in CREDITS {
            for language in Language::ALL {
                assert!(!language.text(*role).is_empty());
            }
            assert!(!who.is_empty());
            assert!(
                who.chars().all(|c| c.is_ascii_graphic() || c == ' '),
                "{who:?} contains characters the font cannot draw"
            );
        }
    }

    #[test]
    fn the_credits_screen_can_be_left() {
        for key in [Key::Escape, Key::Enter] {
            let mut menu = Menu::new(ServerList::default());
            menu.screen = Screen::Credits;
            assert_eq!(menu.key(key), Some(Action::Back));
            assert_eq!(menu.screen, Screen::Main);
        }
    }

    #[test]
    fn opening_a_screen_directly_drops_the_previous_hit_targets() {
        // Regression: the table is rebuilt by `build`, so between
        // changing screen and the next frame it described the screen
        // just left. Opening the pause menu and immediately clicking
        // could fire whatever used to be under the cursor.
        let mut menu = menu_with(3);
        menu.screen = Screen::Servers;
        assert!(point_at(&mut menu, &Action::Add));
        menu.open(Screen::Paused);
        assert_eq!(menu.click(), None, "a stale target survived the switch");
    }

    /// Everything the menus draw straight onto the backdrop, rather
    /// than onto a panel or a button of their own.
    ///
    /// A panel is 96% opaque and a button is solid, so writing on
    /// either is measured against the surface it sits on and is already
    /// covered by `small_text_is_readable_against_everything_it_is_drawn_on`.
    /// These are the ones with nothing under them but the veil, and
    /// they are why the veil has a floor.
    const OVER_THE_BACKDROP: [(&str, [f32; 4], f32); 4] = [
        // The title: three times the body size, so it takes the
        // large-text threshold. Everything else here is small.
        ("the title", Menu::TITLE_GOLD, 3.0),
        ("the subtitle", widgets::TEXT_DIM, 4.5),
        ("the version line", widgets::TEXT_DIM, 4.5),
        ("a screen's help line", widgets::TEXT_DIM, 4.5),
    ];

    /// What the two veils are answering, and what they are answering it
    /// about. Measured -- see `SCENE_HIGHLIGHT_OUTDOORS`.
    const VEILED_SCENES: [(&str, f32, [f32; 4]); 2] = [
        ("a shore, a wood or a meadow", SCENE_HIGHLIGHT_OUTDOORS, VEIL_OUTDOORS),
        ("a cave", SCENE_HIGHLIGHT_UNDERGROUND, VEIL_UNDERGROUND),
    ];

    #[test]
    fn every_word_over_the_menu_scene_is_readable_at_its_worst() {
        // **The worst case is a measurement, not an assumption.** What
        // is under any one letter of a rendered world is unknowable;
        // how bright that world gets is not, and that is what
        // `SCENE_HIGHLIGHT_OUTDOORS` holds. This is the arithmetic that
        // turns it into a veil.
        for (scene, highlight, veil) in VEILED_SCENES {
            let brightest = [highlight, highlight, highlight, 1.0];
            let veiled = widgets::over(veil, brightest);
            for (what, ink, floor) in OVER_THE_BACKDROP {
                let ratio = widgets::contrast(ink, veiled);
                assert!(
                    ratio >= floor,
                    "over {scene} at its brightest, {what} is {ratio:.2}:1, below {floor}:1"
                );
            }
        }
    }

    #[test]
    fn the_veil_over_a_shore_is_no_heavier_than_the_measurement_asks_for() {
        // The other half, and the half that stops this being solved by
        // painting the screen black. A scene nobody can see is the
        // tiled wallpaper this replaced with extra steps, so the
        // outdoor alpha is held to within a hundredth of the lightest
        // one that still carries small text.
        let brightest = [
            SCENE_HIGHLIGHT_OUTDOORS,
            SCENE_HIGHLIGHT_OUTDOORS,
            SCENE_HIGHLIGHT_OUTDOORS,
            1.0,
        ];
        let lighter = [
            VEIL_OUTDOORS[0],
            VEIL_OUTDOORS[1],
            VEIL_OUTDOORS[2],
            VEIL_OUTDOORS[3] - 0.02,
        ];
        assert!(
            widgets::contrast(widgets::TEXT_DIM, widgets::over(lighter, brightest)) < 4.5,
            "two hundredths lighter would still read: the veil is heavier than it has to be"
        );
    }

    #[test]
    fn a_cave_is_veiled_more_lightly_than_a_sunset() {
        // Not a rounding detail: the whole reason there are two veils.
        // A cave measures at a hundredth of a shore's brightness, and
        // veiling the two the same is choosing between a menu nobody
        // can read and a backdrop nobody can see.
        const { assert!(VEIL_UNDERGROUND[3] < VEIL_OUTDOORS[3]) };
        assert_eq!(veil_for(Place::Cave), VEIL_UNDERGROUND);
        for place in [Place::Shore, Place::Forest, Place::Plains] {
            assert_eq!(veil_for(place), VEIL_OUTDOORS);
        }
    }

    #[test]
    fn the_scene_choice_is_inert_while_the_backdrop_is_off() {
        // Shown, so the option is discoverable; not clickable, so it
        // cannot be changed to no effect.
        let mut fixture = Fixture::new();
        fixture.settings.menu_background = false;
        let mut menu = Menu::new(ServerList::default());
        menu.screen = Screen::Settings;

        // The scene row lives at the bottom of the list, below the
        // window; scroll it into view first.
        menu.build(&fixture.ctx());
        menu.scroll_settings(Setting::ALL.len() as i32);
        menu.build(&fixture.ctx());
        let has_control = |m: &Menu| {
            m.hot.iter().any(|(_, a)| {
                matches!(a, Action::Tweak(Setting::MenuBackgroundScene, _))
            })
        };
        assert!(!has_control(&menu), "changing it would do nothing");

        fixture.settings.menu_background = true;
        menu.build(&fixture.ctx());
        assert!(has_control(&menu));
    }

    #[test]
    fn the_background_scene_cycles_through_every_place_and_back_to_random() {
        let mut settings = ClientSettings::default();
        assert_eq!(settings.menu_background_place(), None, "random comes first");
        let mut seen = Vec::new();
        for _ in 0..Place::ALL.len() {
            Setting::MenuBackgroundScene.step(&mut settings, 1);
            seen.push(settings.menu_background_place());
        }
        assert_eq!(
            seen,
            Place::ALL.map(Some).to_vec(),
            "stepping should reach every place, in order"
        );

        // And it wraps rather than dead-ending -- back to "any of them".
        Setting::MenuBackgroundScene.step(&mut settings, 1);
        assert_eq!(settings.menu_background_place(), None);

        // Backwards from the head is the tail, which is what makes the
        // row usable with the left arrow rather than only the right.
        Setting::MenuBackgroundScene.step(&mut settings, -1);
        assert_eq!(settings.menu_background_place(), Some(Place::Cave));
    }

    #[test]
    fn the_caret_blinks() {
        let mut menu = menu_with(0);
        menu.apply(Action::Add);
        let first = menu.caret_visible();
        menu.tick(0.7);
        assert_ne!(first, menu.caret_visible(), "the caret never blinked");
    }
}

/// The settings screen has to hold however many settings there are.
#[cfg(test)]
mod settings_layout_tests {
    use super::*;

    /// The panel the rows are laid out in; must match `build_settings`.
    const PANEL: (f32, f32) = (-0.62, 0.76);

    #[test]
    fn the_window_shows_rows_a_person_can_read() {
        // The list scrolls now, so the rows never shrink -- what has to
        // hold instead is that the window is worth scrolling: full-size
        // rows, and more than a couple of them at a time.
        const GAP: f32 = 0.014;
        const ROW_HEIGHT: f32 = 0.105;
        let height = PANEL.1 - PANEL.0;
        let visible = (((height - 0.06) / (ROW_HEIGHT + GAP)) as usize).max(1);
        assert!(
            visible >= 8,
            "only {visible} rows fit; the window is a slot, not a list"
        );
        // And the last visible row stays inside the panel.
        let bottom = PANEL.1 - 0.03 - visible as f32 * (ROW_HEIGHT + GAP);
        assert!(
            bottom >= PANEL.0 - ROW_HEIGHT,
            "the window admits a row the panel cannot hold"
        );
    }

    #[test]
    fn the_list_and_the_buttons_under_it_do_not_overlap() {
        // The buttons sit at y = -0.83..-0.73, and the notice above them
        // at -0.70. Nothing in the list may reach either.
        assert!(PANEL.0 > -0.70, "the panel reaches the notice line");
    }

    #[test]
    fn every_setting_is_on_the_screen_exactly_once() {
        // `ALL` is written out by hand next to the enum; a setting
        // missing from it is one no player can reach.
        let mut seen = Setting::ALL.to_vec();
        let before = seen.len();
        seen.dedup();
        assert_eq!(seen.len(), before, "a setting is listed twice");
        let settings = ClientSettings::default();
        for setting in Setting::ALL {
            assert!(!setting.label_in(&settings).is_empty());
        }
    }
}

/// The key-bindings screen is a scrolling list, and has to behave like
/// one.
#[cfg(test)]
mod controls_list_tests {
    use super::*;
    use crate::ui::keybinds::Action as Bind;

    /// Every shape this screen is drawn on that has ever been argued
    /// about, and both ends of the interface size a player is likely to
    /// pick.
    ///
    /// The square and the tall window are in here because a desktop
    /// window can be dragged to any shape at all, and this list used to
    /// answer a short panel by making its rows shorter -- so the shapes
    /// with the least vertical room are exactly the ones it got wrong.
    fn every_shape() -> Vec<(f32, f32, widgets::Layout)> {
        let mut out = Vec::new();
        for aspect in [16.0f32 / 9.0, 4.0 / 3.0, 2712.0 / 1220.0, 1.0, 0.75] {
            for scale in [1.0f32, 1.5] {
                out.push((aspect, scale, widgets::Layout::for_screen(aspect, scale)));
            }
        }
        out
    }

    fn controls_on(layout: widgets::Layout, scroll: usize) -> Menu {
        let settings = ClientSettings::default();
        let worlds = Worlds::load(
            std::env::temp_dir().join("primitive-controls-tests-no-such-folder"),
        );
        let mut menu = Menu::new(ServerList::default());
        menu.screen = Screen::Controls;
        menu.controls_scroll = scroll;
        let ctx = MenuContext {
            version: "test",
            font: crate::engine::texture::FontAtlas::for_test(),
            settings: &settings,
            worlds: &worlds,
            background: Backdrop::Bare,
            layout,
        };
        let _ = menu.build(&ctx);
        menu
    }

    /// Which binding the hit-test table answers for a point -- the
    /// table a real click goes through, read the way `hovered` reads it.
    fn pressed_at(menu: &Menu, at: (f32, f32)) -> Option<Action> {
        menu.hot
            .iter()
            .find(|(rect, _)| rect.contains(at.0, at.1))
            .map(|(_, action)| action.clone())
    }

    #[test]
    fn every_binding_row_is_pressed_where_it_is_drawn_at_every_scroll_position() {
        // **The failure this catches is silent.** A list that scrolls
        // its drawing and not its hit-test table looks perfectly
        // ordinary and rebinds the wrong action -- and the player finds
        // out later, in the middle of something, with their jump key
        // gone. So: at every shape, at every scroll position the list
        // can reach, the button for a binding answers for that binding
        // at its own middle and at all four of its corners.
        for (aspect, scale, layout) in every_shape() {
            let mut scroll = 0;
            loop {
                let menu = controls_on(layout, scroll);
                let visible = menu.controls_visible;
                let on_screen: Vec<Bind> =
                    Bind::ALL.iter().copied().skip(scroll).take(visible).collect();
                assert!(!on_screen.is_empty(), "{aspect}:1 at {scale} shows no bindings");

                // Exactly the window, and nothing else. A hot rect left
                // behind for a row that has been scrolled away is a
                // click that lands on a binding nobody can see.
                let offered: Vec<Bind> = menu
                    .hot
                    .iter()
                    .filter_map(|(_, action)| match action {
                        Action::RebindKey(bind) => Some(*bind),
                        _ => None,
                    })
                    .collect();
                assert_eq!(
                    offered, on_screen,
                    "{aspect}:1 at {scale}, scrolled to {scroll}: the rows offered to be \
                     pressed are not the rows on screen",
                );

                for bind in &on_screen {
                    let wanted = Action::RebindKey(*bind);
                    let (rect, _) = menu
                        .hot
                        .iter()
                        .find(|(_, action)| *action == wanted)
                        .unwrap_or_else(|| panic!("{bind:?} has no button"));
                    // A hair inside each corner: a rectangle's own edge
                    // is a boundary case, not the thing being checked.
                    let in_by = (rect.width().min(rect.height()) * 0.1).min(0.002);
                    for at in [
                        (rect.centre_x(), rect.centre_y()),
                        (rect.x0 + in_by, rect.y0 + in_by),
                        (rect.x1 - in_by, rect.y0 + in_by),
                        (rect.x0 + in_by, rect.y1 - in_by),
                        (rect.x1 - in_by, rect.y1 - in_by),
                    ] {
                        assert_eq!(
                            pressed_at(&menu, at),
                            Some(wanted.clone()),
                            "{aspect}:1 at {scale}, scrolled to {scroll}: {bind:?} is drawn \
                             at {rect:?} and {at:?} presses something else",
                        );
                    }
                }

                let mut moved = controls_on(layout, scroll);
                moved.scroll_controls(1);
                if moved.controls_scroll == scroll {
                    break;
                }
                scroll = moved.controls_scroll;
            }
            // ...and the walk above really did reach the end of the
            // list, rather than stopping at the first screenful.
            assert_eq!(
                scroll,
                Bind::ALL.len() - controls_on(layout, 0).controls_visible,
                "{aspect}:1 at {scale}: the list stopped short of its last row",
            );
        }
    }

    #[test]
    fn no_binding_row_is_drawn_outside_the_panel_at_any_interface_size() {
        // The squeeze this replaced had the opposite failure mode: the
        // rows always fitted, by being however thin they had to be. Now
        // that they are a fixed height the thing that has to hold is
        // that the window onto them is honest -- nothing drawn under
        // the footer buttons, nothing off the side of the glass, and no
        // two rows on top of each other.
        for (aspect, scale, layout) in every_shape() {
            let menu = controls_on(layout, 0);
            let footer = menu
                .hot
                .iter()
                .find(|(_, action)| matches!(action, Action::Back))
                .map(|(rect, _)| *rect)
                .expect("a DONE button");
            let reset = menu
                .hot
                .iter()
                .find(|(_, action)| matches!(action, Action::ResetKeys))
                .map(|(rect, _)| *rect)
                .expect("a RESET TO DEFAULTS button");
            assert!(
                (footer.y1 - reset.y1).abs() < 1e-5,
                "{aspect}:1 at {scale}: the two footer buttons are at different heights",
            );

            let mut rows: Vec<Rect> = menu
                .hot
                .iter()
                .filter(|(_, action)| matches!(action, Action::RebindKey(_)))
                .map(|(rect, _)| *rect)
                .collect();
            assert_eq!(
                rows.len(),
                menu.controls_visible,
                "{aspect}:1 at {scale}: more rows are offered than fit",
            );
            for rect in &rows {
                assert!(
                    rect.y0 > footer.y1,
                    "{aspect}:1 at {scale}: a binding at {rect:?} reaches the footer at \
                     {}",
                    footer.y1,
                );
                assert!(rect.y1 < 1.0, "{aspect}:1 at {scale}: {rect:?} is off the top");
                assert!(
                    rect.x0 > -layout.edge() && rect.x1 < layout.edge(),
                    "{aspect}:1 at {scale}: {rect:?} runs off the side",
                );
                // Tall enough to read, and -- where there is a finger
                // rather than a pointer -- tall enough to hit.
                assert!(
                    rect.height() >= layout.finger() - 1e-5,
                    "{aspect}:1 at {scale}: a row {} tall is under the finger",
                    rect.height(),
                );
            }
            rows.sort_by(|a, b| b.y0.partial_cmp(&a.y0).expect("no NaN rows"));
            for pair in rows.windows(2) {
                assert!(
                    pair[0].y0 >= pair[1].y1,
                    "{aspect}:1 at {scale}: {:?} and {:?} overlap",
                    pair[0],
                    pair[1],
                );
            }
        }
    }

    #[test]
    fn the_bindings_list_scrolls_with_the_wheel_and_stops_at_both_ends() {
        let mut menu = controls_on(widgets::Layout::desktop(), 0);
        assert!(
            menu.controls_visible < Bind::ALL.len(),
            "every binding fits; this screen has nothing to scroll",
        );
        // Through `Menu::scroll`, which is where the wheel arrives --
        // and where a thumb drag arrives too, the backend having turned
        // it into a wheel. The screen used to be missing from that
        // match, so the list could not be moved at all.
        menu.scroll(2);
        assert_eq!(menu.controls_scroll, 2, "the wheel did not move the list");
        menu.scroll(-10);
        assert_eq!(menu.controls_scroll, 0, "it scrolled off the top");
        menu.scroll(1000);
        assert_eq!(
            menu.controls_scroll,
            Bind::ALL.len() - menu.controls_visible,
            "it scrolled past the last binding",
        );
    }

    #[test]
    fn an_arrow_key_moves_the_list_and_never_steals_a_binding() {
        // Up and down scroll here, as they do on the settings screen.
        // What must not happen is this arm taking a key the screen was
        // listening for: `lib.rs` asks `awaiting_key` first, so while a
        // rebind is in flight `key` is never called at all -- and the
        // day that changes, this is the test that says what broke.
        let mut menu = controls_on(widgets::Layout::desktop(), 0);
        assert_eq!(menu.key(Key::Down), None);
        assert_eq!(menu.controls_scroll, 1, "Down did not move the list");
        assert_eq!(menu.key(Key::Up), None);
        assert_eq!(menu.controls_scroll, 0, "Up did not move it back");

        menu.apply(Action::RebindKey(Bind::Jump));
        assert_eq!(
            menu.awaiting_key(),
            Some(Bind::Jump),
            "the screen stopped listening for the new key",
        );
    }

    #[test]
    fn opening_the_bindings_starts_at_the_top_of_the_list() {
        let mut menu = controls_on(widgets::Layout::desktop(), 0);
        menu.scroll_controls(1000);
        assert_ne!(menu.controls_scroll, 0);
        menu.apply(Action::Back);
        menu.apply(Action::OpenControls);
        assert_eq!(menu.controls_scroll, 0);
    }

    #[test]
    fn every_binding_can_be_reached_by_scrolling() {
        // The whole point of the window: a binding below the fold is
        // still a binding a player can get to. Under the old squeeze
        // the eighteenth row was on screen and too thin to aim at;
        // under a window that never scrolled it would be unreachable.
        let layout = widgets::Layout::desktop();
        let mut reachable: Vec<Bind> = Vec::new();
        let mut scroll = 0;
        loop {
            let menu = controls_on(layout, scroll);
            for (_, action) in &menu.hot {
                if let Action::RebindKey(bind) = action {
                    if !reachable.contains(bind) {
                        reachable.push(*bind);
                    }
                }
            }
            let mut moved = controls_on(layout, scroll);
            moved.scroll_controls(1);
            if moved.controls_scroll == scroll {
                break;
            }
            scroll = moved.controls_scroll;
        }
        for bind in Bind::ALL {
            assert!(
                reachable.contains(&bind),
                "{bind:?} cannot be rebound at any scroll position",
            );
        }
    }
}

/// Pictures of the key-bindings screen, for a person to look at.
///
/// **Its own twenty-line rasteriser, and that is deliberate.**
/// `ui::snapshot` already turns interface quads into a PNG, but it
/// renders a fixed cast of screens and every one of its helpers --
/// `write`, `fill`, `draw_glyph` -- is private to that module. Making
/// them public to take four pictures of one screen would widen a test
/// harness's surface for the sake of a tool; the twenty lines that fill
/// a box and stamp a glyph cost nothing here and leave that file alone.
///
/// What it draws is only what this screen has in it: boxes and text.
/// Block icons and triangles, which the pack and the map need, are not
/// here because the bindings list has neither.
///
/// ```text
/// KEYBIND_SHOT_DIR=shots/ui3/after cargo test -p primitive_client --lib \
///     keybind_shots -- --ignored --nocapture
/// PRIMITIVE_TOUCH_UI=1 KEYBIND_SHOT_DIR=shots/ui3/after \
///     cargo test -p primitive_client --lib keybind_shots -- --ignored --nocapture
/// ```
#[cfg(test)]
mod controls_shots {
    use super::*;
    use crate::engine::font::{glyph, GLYPH_HEIGHT, GLYPH_WIDTH};
    use crate::engine::texture::{FontAtlas, GLYPHS};
    use crate::ui::hotbar::{HotbarVertex, UNTEXTURED};

    /// The window a picture is taken through.
    #[derive(Clone, Copy)]
    struct Shot {
        width: u32,
        height: u32,
        scale: f32,
    }

    impl Shot {
        fn aspect(&self) -> f32 {
            self.width as f32 / self.height as f32
        }
    }

    /// UVs come back through a float; a rounded key compares them safely.
    fn uv_key(v: f32) -> i32 {
        (v * 10_000.0).round() as i32
    }

    /// Fills one rectangle in interface coordinates, blended by its alpha.
    fn fill(pixels: &mut [[u8; 4]], shot: Shot, r: (f32, f32, f32, f32), tint: [f32; 4]) {
        let (w, h) = (shot.width as i32, shot.height as i32);
        let to_x = |x: f32| ((x / shot.aspect() + 1.0) * 0.5 * shot.width as f32).round() as i32;
        // Y is up in interface coordinates and down in an image.
        let to_y = |y: f32| ((1.0 - (y + 1.0) * 0.5) * shot.height as f32).round() as i32;
        let (px0, px1) = (to_x(r.0).max(0), to_x(r.2).min(w));
        let (py0, py1) = (to_y(r.3).max(0), to_y(r.1).min(h));
        let alpha = tint[3].clamp(0.0, 1.0);
        for y in py0..py1 {
            for x in px0..px1 {
                let at = (y * w + x) as usize;
                let under = pixels[at];
                for channel in 0..3 {
                    let over = tint[channel].clamp(0.0, 1.0) * 255.0;
                    pixels[at][channel] =
                        (under[channel] as f32 * (1.0 - alpha) + over * alpha).round() as u8;
                }
            }
        }
    }

    /// Stamps one letter off the same bitmap font the game draws with,
    /// because most layout mistakes are text mistakes.
    fn draw_glyph(
        pixels: &mut [[u8; 4]],
        shot: Shot,
        c: char,
        r: (f32, f32, f32, f32),
        tint: [f32; 4],
    ) {
        let (w, h) = (r.2 - r.0, r.3 - r.1);
        for (row, bits) in glyph(c).iter().enumerate() {
            for column in 0..GLYPH_WIDTH {
                if bits & (1 << (GLYPH_WIDTH - 1 - column)) == 0 {
                    continue;
                }
                let px0 = r.0 + w * column as f32 / GLYPH_WIDTH as f32;
                let py1 = r.3 - h * row as f32 / GLYPH_HEIGHT as f32;
                fill(
                    pixels,
                    shot,
                    (px0, py1 - h / GLYPH_HEIGHT as f32, px0 + w / GLYPH_WIDTH as f32, py1),
                    tint,
                );
            }
        }
    }

    fn write(path: &str, shot: Shot, vertices: &[HotbarVertex], font: FontAtlas) {
        let glyphs: std::collections::HashMap<(u32, i32, i32), char> = GLYPHS
            .chars()
            .map(|c| {
                let (layer, u, v) = font.place(c);
                ((layer, uv_key(u), uv_key(v)), c)
            })
            .collect();
        // A mid-grey ground, so a panel nearly the colour of the
        // background shows up as the problem it is.
        let mut pixels = vec![[70u8, 78, 92, 255]; (shot.width * shot.height) as usize];
        for quad in vertices.chunks_exact(6) {
            let xs = quad.iter().map(|v| v.position[0]);
            let ys = quad.iter().map(|v| v.position[1]);
            let r = (
                xs.clone().fold(f32::MAX, f32::min),
                ys.clone().fold(f32::MAX, f32::min),
                xs.fold(f32::MIN, f32::max),
                ys.fold(f32::MIN, f32::max),
            );
            let tint = quad[0].tint;
            match quad[0].tex_layer {
                UNTEXTURED => fill(&mut pixels, shot, r, tint),
                // A glyph quad's corner is its *smallest* uv, whichever
                // corner happens to come first in the run.
                layer => {
                    let at = (
                        layer,
                        uv_key(quad.iter().map(|v| v.uv[0]).fold(f32::MAX, f32::min)),
                        uv_key(quad.iter().map(|v| v.uv[1]).fold(f32::MAX, f32::min)),
                    );
                    if let Some(&c) = glyphs.get(&at) {
                        draw_glyph(&mut pixels, shot, c, r, tint);
                    }
                }
            }
        }
        let mut flat = Vec::with_capacity(pixels.len() * 4);
        for pixel in &pixels {
            flat.extend_from_slice(pixel);
        }
        let image: image::RgbaImage = image::ImageBuffer::from_raw(shot.width, shot.height, flat)
            .expect("the buffer is the right size");
        image.save(path).expect("write png");
        println!("wrote {path}");
    }

    #[test]
    #[ignore = "a tool: writes PNGs of the key-bindings screen for a person to look at"]
    fn keybind_shots() {
        let out = std::env::var("KEYBIND_SHOT_DIR").unwrap_or_else(|_| ".".to_string());
        std::fs::create_dir_all(&out).expect("output directory");
        // The same resolution the game packs the font at, so the text in
        // the picture is the text the game would draw.
        let font = FontAtlas::for_size(32, 1_000);
        let worlds =
            Worlds::load(std::env::temp_dir().join("primitive-keybind-shots-no-such-folder"));
        let mut settings = ClientSettings::default();
        // A phone is a different layout, not a different size, so it is
        // named in the file rather than left to be guessed at.
        let touch = if crate::ui::lang::touch_primary() { "_touch" } else { "" };

        for shot in [
            Shot { width: 1280, height: 720, scale: 1.0 },
            Shot { width: 1920, height: 1080, scale: 1.0 },
            // The phone this pass was for, and the same interface size
            // on a window with half the pixels: the shape that runs out
            // of room first.
            Shot { width: 2712, height: 1220, scale: 1.5 },
            Shot { width: 1280, height: 720, scale: 1.5 },
        ] {
            for (language, tag) in [(Language::English, "en"), (Language::Russian, "ru")] {
                settings.language = language;
                let mut menu = Menu::new(ServerList::default());
                menu.screen = Screen::Controls;
                let ctx = MenuContext {
                    version: "test",
                    font,
                    settings: &settings,
                    worlds: &worlds,
                    background: Backdrop::Bare,
                    layout: widgets::Layout::for_screen(shot.aspect(), shot.scale),
                };
                let mut vertices = Vec::new();
                menu.build_into(&ctx, &mut vertices);
                let name = format!(
                    "{out}/keybinds_{}x{}_s{}{touch}_{tag}.png",
                    shot.width, shot.height, shot.scale
                );
                write(&name, shot, &vertices, font);
            }
        }
    }
}
