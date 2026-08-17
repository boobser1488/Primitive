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

use crate::settings::ClientSettings;
use crate::ui::widgets::{self, Painter, Rect};
use crate::logic::worlds::{self, Worlds};
use crate::ui::lang::{Language, Msg};

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
    /// Reads `servers.toml`, seeding it from the configured address on
    /// first run.
    ///
    /// An unreadable or invalid file is *not* silently replaced: it is
    /// left alone and the configured address is offered instead, so a
    /// hand-edited file with a typo in it can still be recovered.
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

/// Which screen the player is on.
#[derive(Debug, Clone, PartialEq)]
pub enum Screen {
    Main,
    /// Singleplayer worlds.
    Worlds,
    /// The new-world form: name and seed.
    CreatingWorld,
    Servers,
    /// The add/edit server form. `Some(index)` edits an existing entry.
    Editing(Option<usize>),
    /// Everything the game can change without a text editor.
    Settings,
    /// Key bindings. Its own screen rather than more rows on Settings:
    /// eleven actions is already as long as that list, and rebinding
    /// swallows the next keypress, which the rest of the screen must
    /// not do.
    Controls,
    /// Who made what.
    Credits,
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
    Vsync,
    Fog,
    AmbientOcclusion,
    Anisotropy,
    TransparentLeaves,
    DetailDistance,
    Cloudiness,
    LocalViewDistance,
    MenuBackground,
    MenuBackgroundBlock,
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
    pub const ALL: [Setting; 15] = [
        Setting::Language,
        Setting::Username,
        Setting::RenderDistance,
        Setting::Fov,
        Setting::Sensitivity,
        Setting::Vsync,
        Setting::Fog,
        Setting::AmbientOcclusion,
        Setting::Anisotropy,
        Setting::TransparentLeaves,
        Setting::DetailDistance,
        Setting::Cloudiness,
        Setting::LocalViewDistance,
        Setting::MenuBackground,
        Setting::MenuBackgroundBlock,
    ];

    /// What this row is called, as a message the language table knows.
    pub fn msg(&self) -> Msg {
        match self {
            Setting::Username => Msg::Name,
            Setting::RenderDistance => Msg::RenderDistance,
            Setting::Fov => Msg::FieldOfView,
            Setting::Sensitivity => Msg::MouseSensitivity,
            Setting::Vsync => Msg::Vsync,
            Setting::Fog => Msg::Fog,
            Setting::AmbientOcclusion => Msg::AmbientOcclusion,
            Setting::Anisotropy => Msg::Anisotropy,
            Setting::TransparentLeaves => Msg::TransparentLeaves,
            Setting::DetailDistance => Msg::DetailDistance,
            Setting::Cloudiness => Msg::Cloudiness,
            Setting::LocalViewDistance => Msg::LocalViewDistance,
            Setting::MenuBackground => Msg::MenuBackground,
            Setting::MenuBackgroundBlock => Msg::MenuBackgroundBlock,
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
                language.text(Msg::Chunks)
            ),
            Setting::Fov => {
                format!("{:.0} {}", settings.fov_degrees, language.text(Msg::Degrees))
            }
            // Shown scaled up: the stored value is around 0.0025, and a
            // row reading "0.003" tells the player nothing about whether
            // a step made a difference.
            Setting::Sensitivity => format!("{:.0}", settings.mouse_sensitivity * 10_000.0),
            Setting::Vsync => on_off(settings.vsync, language),
            Setting::Fog => on_off(settings.fog_enabled, language),
            Setting::AmbientOcclusion => format!("{:.0}%", settings.ambient_occlusion * 100.0),
            Setting::Anisotropy => {
                if settings.anisotropy <= 1 {
                    language.text(Msg::Off).to_string()
                } else {
                    format!("{}x", settings.anisotropy)
                }
            }
            Setting::TransparentLeaves => on_off(settings.transparent_leaves, language),
            Setting::DetailDistance => format!("{:.0}%", settings.detail_distance * 100.0),
            Setting::Cloudiness => format!("{:.0}%", settings.cloudiness * 100.0),
            Setting::LocalViewDistance => format!(
                "{} {}",
                settings.singleplayer_view_distance_chunks,
                language.text(Msg::Chunks)
            ),
            Setting::MenuBackground => on_off(settings.menu_background, language),
            Setting::MenuBackgroundBlock => settings.menu_background_block.to_uppercase(),
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
                | Setting::TransparentLeaves
        )
    }

    /// True for the setting that is greyed out until another one is on.
    pub fn depends_on_menu_background(&self) -> bool {
        matches!(self, Setting::MenuBackgroundBlock)
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
            Setting::Vsync => settings.vsync = !settings.vsync,
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
            Setting::TransparentLeaves => {
                settings.transparent_leaves = !settings.transparent_leaves
            }
            Setting::DetailDistance => settings.detail_distance += 0.1 * d,
            Setting::Cloudiness => settings.cloudiness += 0.1 * d,
            Setting::LocalViewDistance => settings.singleplayer_view_distance_chunks += delta,
            Setting::MenuBackground => settings.menu_background = !settings.menu_background,
            Setting::MenuBackgroundBlock => {
                let blocks = crate::settings::MENU_BACKGROUND_BLOCKS;
                let current = blocks
                    .iter()
                    .position(|id| *id == settings.menu_background_block())
                    .unwrap_or(0) as i32;
                let count = blocks.len() as i32;
                let next = ((current + delta) % count + count) % count;
                settings.menu_background_block =
                    primitive_shared::types::block_name(blocks[next as usize]).to_string();
            }
        }
        settings.sanitize();
    }
}

fn on_off(value: bool, language: Language) -> String {
    language
        .text(if value { Msg::On } else { Msg::Off })
        .to_string()
}

/// Something the player asked for. `main.rs` carries these out; nothing
/// in this module performs them.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    OpenWorlds,
    OpenServers,
    OpenSettings,
    OpenControls,
    OpenCredits,
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
    CreateWorld,

    // ---- settings ----
    Tweak(Setting, i32),
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
    pub screen: Screen,
    pub name_input: String,
    pub address_input: String,
    pub seed_input: String,
    pub focus: Field,
    /// True while the name row of the settings screen is being typed
    /// into. The row turns into a text field and swallows keys.
    pub editing_username: bool,
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
    caret_phase: f32,
}

/// Longest name and address the form will accept. Both are generous;
/// the point is only that neither can grow without bound.
const MAX_NAME: usize = 32;
const MAX_ADDRESS: usize = 64;
/// A `u32` is ten digits at most; anything longer cannot be a seed.
const MAX_SEED_DIGITS: usize = 10;

/// What to say, in the language the player has chosen.
///
/// A free function taking the context rather than a method, so a screen
/// that already borrows `self` mutably to build itself can still ask.
fn say(ctx: &MenuContext, msg: Msg) -> &'static str {
    ctx.settings.language.text(msg)
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
            world_visible: 1,
            screen: Screen::Main,
            name_input: String::new(),
            address_input: String::new(),
            seed_input: String::new(),
            focus: Field::Name,
            editing_username: false,
            rebinding: None,
            notice: None,
            cursor: None,
            button_focus: None,
            came_from: Box::new(Screen::Main),
            hot: Vec::new(),
            caret_phase: 0.0,
        }
    }

    /// Moves the mouse pointer, taking the highlight away from the
    /// keyboard.
    pub fn set_cursor(&mut self, position: Option<(f32, f32)>) {
        self.cursor = position;
        if position.is_some() {
            self.button_focus = None;
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
        if matches!(self.screen, Screen::Worlds) {
            self.scroll_worlds(rows);
        }
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
            Screen::Controls => match key {
                Key::Escape => Some(self.apply(Action::Back)),
                _ => None,
            },
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
                Key::Delete if !self.servers.servers.is_empty() => {
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
                Key::Delete if self.world_count > 0 => {
                    Some(self.apply(Action::AskDeleteWorld(self.world_selected)))
                }
                _ => None,
            },

            Screen::Editing(_) | Screen::CreatingWorld => {
                let save = if matches!(self.screen, Screen::CreatingWorld) {
                    Action::CreateWorld
                } else {
                    Action::Save
                };
                match key {
                    Key::Tab => self.cycle_field(),
                    Key::Enter => return Some(self.apply(save)),
                    Key::Escape => return Some(self.apply(Action::Cancel)),
                    Key::Backspace => {
                        self.field_mut().pop();
                    }
                    Key::Char(c) => self.type_char(c),
                    _ => {}
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
                            self.editing_username = false;
                            return None;
                        }
                        Key::Backspace => {
                            self.name_input.pop();
                        }
                        Key::Char(c) => self.type_char(c),
                        _ => {}
                    }
                    return None;
                }
                match key {
                    Key::Escape | Key::Enter => Some(self.apply(Action::Back)),
                    _ => None,
                }
            }

            Screen::Credits => match key {
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
        if field.chars().count() < limit {
            field.push(c);
        }
    }

    /// True when keystrokes should go into a field rather than be read
    /// as shortcuts.
    pub fn accepts_text(&self) -> bool {
        matches!(self.screen, Screen::Editing(_) | Screen::CreatingWorld)
            || (matches!(self.screen, Screen::Settings) && self.editing_username)
    }

    fn field_mut(&mut self) -> &mut String {
        match self.focus {
            Field::Name => &mut self.name_input,
            Field::Address => &mut self.address_input,
            Field::Seed => &mut self.seed_input,
        }
    }

    /// Moves between the fields of whichever form is open.
    fn cycle_field(&mut self) {
        self.focus = match (&self.screen, self.focus) {
            (Screen::CreatingWorld, Field::Name) => Field::Seed,
            (Screen::CreatingWorld, _) => Field::Name,
            (_, Field::Name) => Field::Address,
            _ => Field::Name,
        };
    }

    /// Starts typing into the name row of the settings screen.
    pub fn begin_username_edit(&mut self, current: String) {
        self.name_input = current;
        self.focus = Field::Name;
        self.editing_username = true;
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

    fn apply_inner(&mut self, action: Action) -> Action {
        match &action {
            Action::OpenServers => {
                self.notice = None;
                self.screen = Screen::Servers;
            }
            Action::Back => {
                self.notice = None;
                self.editing_username = false;
                self.screen = match self.screen {
                    // Back out of a connection attempt, or out of the
                    // settings, to wherever it was opened from rather
                    // than to a fixed screen. Leaving the settings for
                    // the main menu when they were opened from a paused
                    // world looks exactly like being disconnected.
                    Screen::Failed { .. } | Screen::Connecting { .. } | Screen::Settings => {
                        (*self.came_from).clone()
                    }
                    Screen::Controls => Screen::Settings,
                    Screen::CreatingWorld | Screen::Confirm { .. } => Screen::Worlds,
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
                    self.name_input = entry.name.clone();
                    self.address_input = entry.address.clone();
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
            Action::Focus(field) => self.focus = *field,
            Action::Save => return self.save_form(),
            Action::Cancel => {
                self.notice = None;
                self.editing_username = false;
                self.screen = match self.screen {
                    Screen::Connecting { .. } | Screen::Failed { .. } => (*self.came_from).clone(),
                    Screen::CreatingWorld | Screen::Confirm { .. } => Screen::Worlds,
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
                self.focus = Field::Name;
                self.notice = None;
                self.screen = Screen::CreatingWorld;
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

            // ---- settings ----
            Action::OpenSettings => {
                self.notice = None;
                self.editing_username = false;
                *self.came_from = self.screen.clone();
                self.screen = Screen::Settings;
            }
            Action::OpenControls => {
                self.notice = None;
                self.rebinding = None;
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

    /// Validates and stores the form. Returns `Action::Save` on success
    /// and `Action::Focus` on failure -- so a rejected form puts the
    /// cursor in the field that needs fixing rather than just refusing.
    fn save_form(&mut self) -> Action {
        let address = self.address_input.trim().to_string();
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
            let trimmed = self.name_input.trim();
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
        self.editing_username = false;
        self.notice = None;
    }

    /// Fills in the detail line of a confirmation gate -- the name of
    /// the thing about to be destroyed.
    pub fn set_confirm_detail(&mut self, text: String) {
        if let Screen::Confirm { detail, .. } = &mut self.screen {
            *detail = text;
        }
    }

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
        for entry in &self.servers.servers {
            entry.name.hash(&mut h);
            entry.address.hash(&mut h);
        }
        self.name_input.hash(&mut h);
        self.address_input.hash(&mut h);
        self.seed_input.hash(&mut h);
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
            || matches!(self.screen, Screen::Editing(_) | Screen::CreatingWorld);
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
        }
        // The world rows show a rough age ("3 min ago") measured from
        // the wall clock, so the clock's minute is part of the picture.
        (worlds::unix_now() / 60).hash(&mut h);
        ctx.background
            .map(|(layer, aspect)| (layer, aspect.to_bits()))
            .hash(&mut h);
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
        self.set_world_count(ctx.worlds.list().len());
        let mut p = Painter::onto(ctx.font, std::mem::take(out));
        let hover = self.cursor;

        match self.screen.clone() {
            Screen::Main => self.build_main(&mut p, hover, ctx),
            Screen::Worlds => self.build_worlds(&mut p, hover, ctx),
            Screen::CreatingWorld => self.build_world_form(&mut p, hover, ctx),
            Screen::Servers => self.build_servers(&mut p, hover, ctx),
            Screen::Editing(existing) => self.build_form(&mut p, hover, ctx, existing.is_some()),
            Screen::Settings => self.build_settings(&mut p, hover, ctx),
            Screen::Controls => self.build_controls(&mut p, hover, ctx),
            Screen::Credits => self.build_credits(&mut p, hover, ctx),
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
        }

        *out = p.into_vertices();
    }

    fn build_worlds(&mut self, p: &mut Painter, cursor: Option<(f32, f32)>, ctx: &MenuContext) {
        self.backdrop(p, ctx);
        self.title(p, say(ctx, Msg::Worlds), 0.86);

        let panel = Rect::new(-0.95, -0.30, 0.95, 0.66);
        p.panel(panel);

        let worlds = ctx.worlds.list();
        if worlds.is_empty() {
            p.text_centred(
                say(ctx, Msg::NoWorldsYet),
                panel.centre_x(),
                0.25,
                1.0,
                widgets::TEXT_DIM,
            );
        }

        let row_height = 0.11;
        let mut y = panel.y1 - 0.03 - row_height;
        let visible = (((panel.height() - 0.06) / (row_height + 0.014)) as usize).max(1);
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
        let gutter = 0.022;

        // By index, not by iterator: the index *is* the row's identity
        // -- it goes into `Action::PlayWorld` and is compared against
        // the selection -- so enumerating a slice would only put it back.
        #[allow(clippy::needless_range_loop)]
        for index in first..worlds.len().min(first + visible) {
            let world = &worlds[index];
            let rect = Rect::new(panel.x0 + 0.03, y, panel.x1 - 0.03 - gutter, y + row_height);
            let selected = index == self.world_selected;

            p.quad(rect, if selected { widgets::ROW_SELECTED } else { widgets::ROW });
            if selected || self.is_hovered(rect, cursor) {
                p.border(rect, 0.003, if selected { widgets::ACCENT } else { widgets::BUTTON_EDGE });
            }
            let detail = format!(
                "seed {}   {}",
                world.seed.unwrap_or(ctx.settings.singleplayer_seed),
                world.played_description(now, ctx.settings.language)
            );
            p.row_labels(
                rect,
                0.025,
                &world.name,
                if selected { widgets::TEXT } else { widgets::TEXT_DIM },
                &detail,
                widgets::TEXT_DIM,
            );

            // Same rule as the server list: click to select, click the
            // selected row again to play.
            let action = if selected {
                Action::PlayWorld(index)
            } else {
                Action::SelectWorld(index)
            };
            self.hot.push((rect, action));
            y -= row_height + 0.014;
        }

        // The scrollbar: a track down the gutter with a thumb on it as
        // long a share of the track as the window is of the list.
        //
        // Only when there is something to scroll. A full-length thumb
        // that can never move is not information, it is furniture --
        // and, worse, it is furniture that says "there is more" to
        // anybody glancing at it.
        if worlds.len() > visible {
            let track = Rect::new(
                panel.x1 - 0.03 - gutter + 0.006,
                panel.y0 + 0.03,
                panel.x1 - 0.03,
                panel.y1 - 0.03,
            );
            p.quad(track, widgets::ROW);
            let span = visible as f32 / worlds.len() as f32;
            let offset = first as f32 / worlds.len() as f32;
            // Measured from the top down, because the list runs down the
            // screen and y runs up it.
            let top = track.y1 - track.height() * offset;
            p.quad(
                Rect::new(track.x0, top - track.height() * span, track.x1, top),
                widgets::ACCENT,
            );
        }

        if let Some((text, good)) = self.notice_line(ctx) {
            let colour = if good { widgets::TEXT_GOOD } else { widgets::TEXT_BAD };
            p.text_centred(&text, 0.0, -0.36, 0.9, colour);
        }

        let any = !worlds.is_empty();
        let selected = self.world_selected;
        let height = 0.10;
        self.add_button(
            p,
            cursor,
            Rect::centred(-0.62, -0.52, 0.56, height),
            say(ctx, Msg::Play),
            Action::PlayWorld(selected),
            any,
        );
        self.add_button(
            p,
            cursor,
            Rect::centred(0.0, -0.52, 0.56, height),
            say(ctx, Msg::New),
            Action::NewWorld,
            true,
        );
        self.add_button(
            p,
            cursor,
            Rect::centred(0.62, -0.52, 0.56, height),
            say(ctx, Msg::Delete),
            Action::AskDeleteWorld(selected),
            any,
        );
        self.add_button(
            p,
            cursor,
            Rect::centred(0.0, -0.70, 0.5, height),
            say(ctx, Msg::Back),
            Action::Back,
            true,
        );

        p.text_centred(
            say(ctx, Msg::WorldsHelp),
            0.0,
            -0.83,
            0.8,
            widgets::TEXT_DIM,
        );
    }

    fn build_world_form(&mut self, p: &mut Painter, cursor: Option<(f32, f32)>, ctx: &MenuContext) {
        self.backdrop(p, ctx);
        self.title(p, say(ctx, Msg::NewWorld), 0.70);

        let panel = Rect::new(-0.95, -0.30, 0.95, 0.44);
        p.panel(panel);

        let label_x = panel.x0 + 0.05;
        let field = |y: f32| Rect::new(panel.x0 + 0.05, y, panel.x1 - 0.05, y + 0.11);

        p.text(say(ctx, Msg::Name), label_x, 0.30, 0.9, widgets::TEXT_DIM);
        let name_rect = field(0.14);
        p.field(
            name_rect,
            &self.name_input,
            self.focus == Field::Name,
            self.caret_visible(),
        );
        self.hot.push((name_rect, Action::Focus(Field::Name)));

        p.text(say(ctx, Msg::Seed), label_x, -0.02, 0.9, widgets::TEXT_DIM);
        let seed_rect = field(-0.18);
        let shown = if self.seed_input.is_empty() {
            ctx.settings.singleplayer_seed.to_string()
        } else {
            self.seed_input.clone()
        };
        p.field(
            seed_rect,
            &shown,
            self.focus == Field::Seed,
            self.caret_visible(),
        );
        self.hot.push((seed_rect, Action::Focus(Field::Seed)));

        if let Some((text, good)) = self.notice_line(ctx) {
            let colour = if good { widgets::TEXT_GOOD } else { widgets::TEXT_BAD };
            p.text_centred(&text, 0.0, -0.38, 0.9, colour);
        } else {
            p.text_centred(
                say(ctx, Msg::SeedHelp),
                0.0,
                -0.38,
                0.8,
                widgets::TEXT_DIM,
            );
        }

        self.add_button(
            p,
            cursor,
            Rect::centred(-0.28, -0.56, 0.5, 0.10),
            say(ctx, Msg::Create),
            Action::CreateWorld,
            true,
        );
        self.add_button(
            p,
            cursor,
            Rect::centred(0.28, -0.56, 0.5, 0.10),
            say(ctx, Msg::Cancel),
            Action::Cancel,
            true,
        );

        p.text_centred(
            say(ctx, Msg::WorldFormHelp),
            0.0,
            -0.74,
            0.8,
            widgets::TEXT_DIM,
        );
    }

    fn build_settings(&mut self, p: &mut Painter, cursor: Option<(f32, f32)>, ctx: &MenuContext) {
        self.backdrop(p, ctx);
        self.title(p, say(ctx, Msg::Settings), 0.90);

        let panel = Rect::new(-1.15, -0.60, 1.15, 0.76);
        p.panel(panel);

        // The rows are sized to fit the panel rather than the panel to
        // fit the rows.
        //
        // A fixed height was right for the nine settings this started
        // with and wrong for every one added since: two more rows ran
        // the list out of the bottom of its own panel and over the
        // buttons under it. Dividing the space up means the screen is
        // correct for any number of settings, which is the number that
        // keeps changing.
        //
        // Capped, so that a short list does not stretch into a few
        // enormous bars.
        const GAP: f32 = 0.012;
        const MARGIN: f32 = 0.035;
        const MAX_ROW: f32 = 0.105;
        let count = Setting::ALL.len() as f32;
        let room = panel.height() - 2.0 * MARGIN - (count - 1.0) * GAP;
        let row_height = (room / count).min(MAX_ROW);
        // How much of a row the buttons inside it leave as margin, and
        // how large its text is, so both shrink with the row instead of
        // spilling out of it.
        let inset = (row_height * 0.115).min(0.012);
        let label_scale = (row_height / MAX_ROW).min(1.0);
        let mut y = panel.y1 - MARGIN - row_height;

        for setting in Setting::ALL {
            let row = Rect::new(panel.x0 + 0.03, y, panel.x1 - 0.03, y + row_height);

            if setting.is_text() {
                p.quad(row, widgets::ROW);
                p.label_left(row, setting.label_in(ctx.settings), 0.025, label_scale, widgets::TEXT);
                let field_rect =
                    Rect::new(row.x1 - 0.78, row.y0 + inset, row.x1 - 0.02, row.y1 - inset);
                if self.editing_username {
                    p.field(field_rect, &self.name_input, true, self.caret_visible());
                    self.hot.push((field_rect, Action::CommitUsername));
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
                p.setting_row(
                    row,
                    setting.label_in(ctx.settings),
                    &setting.value(ctx.settings),
                    enabled,
                );
                if setting.is_toggle() {
                    // One wide button: a switch has no "less" and "more".
                    let toggle =
                        Rect::new(row.x1 - 0.26, row.y0 + inset, row.x1 - 0.02, row.y1 - inset);
                    self.add_button(p, cursor, toggle, say(ctx, Msg::Toggle), Action::Tweak(setting, 1), enabled);
                } else {
                    let minus =
                        Rect::new(row.x1 - 0.26, row.y0 + inset, row.x1 - 0.15, row.y1 - inset);
                    let plus =
                        Rect::new(row.x1 - 0.13, row.y0 + inset, row.x1 - 0.02, row.y1 - inset);
                    self.add_button(p, cursor, minus, "-", Action::Tweak(setting, -1), enabled);
                    self.add_button(p, cursor, plus, "+", Action::Tweak(setting, 1), enabled);
                }
            }

            y -= row_height + GAP;
        }

        if let Some((text, good)) = self.notice_line(ctx) {
            let colour = if good { widgets::TEXT_GOOD } else { widgets::TEXT_BAD };
            p.text_centred(&text, 0.0, -0.70, 0.9, colour);
        }

        self.add_button(
            p,
            cursor,
            Rect::new(-0.62, -0.83, -0.02, -0.73),
            say(ctx, Msg::Controls),
            Action::OpenControls,
            true,
        );
        self.add_button(
            p,
            cursor,
            Rect::new(0.02, -0.83, 0.62, -0.73),
            say(ctx, Msg::Done),
            Action::Back,
            true,
        );
        p.text_centred(
            say(ctx, Msg::SettingsHelp),
            0.0,
            -0.90,
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

    fn build_controls(&mut self, p: &mut Painter, cursor: Option<(f32, f32)>, ctx: &MenuContext) {
        // The rows are measured rather than fixed -- see
        // `controls_row_height`.
        use crate::ui::keybinds::Action as Bind;

        self.backdrop(p, ctx);
        self.title(p, say(ctx, Msg::Controls), 0.90);

        let panel = Rect::new(-1.05, -0.62, 1.05, 0.76);
        p.panel(panel);

        let row_height = controls_row_height();
        let mut y = panel.y1 - 0.030 - row_height;

        for action in Bind::ALL {
            let row = Rect::new(panel.x0 + 0.03, y, panel.x1 - 0.03, y + row_height);
            p.quad(row, widgets::ROW);
            p.label_left(row, action.label(ctx.settings.language), 0.025, 0.95, widgets::TEXT);

            let button = Rect::new(row.x1 - 0.44, row.y0 + 0.011, row.x1 - 0.02, row.y1 - 0.011);
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
                p.quad(button, widgets::FIELD);
                p.border(button, 0.003, widgets::TEXT_BAD);
                p.label_in(button, "--", 0.95, widgets::TEXT_BAD);
                self.hot.push((button, Action::RebindKey(action)));
            } else {
                self.add_button(p, cursor, button, &label, Action::RebindKey(action), true);
            }

            y -= row_height + 0.010;
        }

        if let Some((text, good)) = self.notice_line(ctx) {
            let colour = if good { widgets::TEXT_GOOD } else { widgets::TEXT_BAD };
            p.text_centred(&text, 0.0, -0.68, 0.85, colour);
        }

        self.add_button(
            p,
            cursor,
            Rect::new(-0.62, -0.83, -0.02, -0.73),
            say(ctx, Msg::ResetToDefaults),
            Action::ResetKeys,
            true,
        );
        self.add_button(
            p,
            cursor,
            Rect::new(0.02, -0.83, 0.62, -0.73),
            say(ctx, Msg::Done),
            Action::Back,
            true,
        );
        p.text_centred(
            say(ctx, Msg::ControlsHelp),
            0.0,
            -0.90,
            0.75,
            widgets::TEXT_DIM,
        );
    }

    fn build_credits(&mut self, p: &mut Painter, cursor: Option<(f32, f32)>, ctx: &MenuContext) {
        self.backdrop(p, ctx);
        self.title(p, say(ctx, Msg::Credits), 0.74);

        let panel = Rect::new(-1.0, -0.34, 1.0, 0.52);
        p.panel(panel);

        // Role on the left, who did it on the right. Two columns rather
        // than one line each, because the question a credits screen
        // answers is "who did the art", and a list of names does not
        // answer it.
        let row_height = 0.115;
        let mut y = panel.y1 - 0.05 - row_height;
        for (role, who) in CREDITS {
            let row = Rect::new(panel.x0 + 0.04, y, panel.x1 - 0.04, y + row_height);
            p.label_left(row, say(ctx, *role), 0.02, 0.9, widgets::TEXT_DIM);
            let width = widgets::measure(who, 1.1);
            p.label_left(
                Rect::new(row.x1 - width, row.y0, row.x1, row.y1),
                who,
                0.0,
                1.1,
                widgets::TEXT,
            );
            y -= row_height;
        }

        p.text_centred(ctx.version, 0.0, panel.y0 + 0.09, 0.8, widgets::TEXT_DIM);

        self.add_button(
            p,
            cursor,
            Rect::centred(0.0, -0.52, 0.6, 0.105),
            say(ctx, Msg::Back),
            Action::Back,
            true,
        );
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
        self.backdrop(p, ctx);
        self.title(p, say(ctx, question), 0.42);
        if !detail.is_empty() {
            p.text_centred(detail, 0.0, 0.18, 1.2, widgets::TEXT);
        }
        p.text_centred(say(ctx, Msg::CannotBeUndone), 0.0, 0.02, 0.9, widgets::TEXT_BAD);

        // Cancel first and on the left, where the eye lands: the safe
        // answer should be the easy one to hit.
        self.add_button(
            p,
            cursor,
            Rect::centred(-0.32, -0.28, 0.56, 0.10),
            say(ctx, Msg::Cancel),
            Action::Cancel,
            true,
        );
        self.add_button(
            p,
            cursor,
            Rect::centred(0.32, -0.28, 0.56, 0.10),
            say(ctx, confirm_label),
            action,
            true,
        );
        p.text_centred(say(ctx, Msg::ConfirmHelp), 0.0, -0.46, 0.8, widgets::TEXT_DIM);
    }

    /// The backdrop every full-screen menu starts with.
    ///
    /// With wallpaper on, the scrim over it is only partly opaque -- but
    /// still substantial, because the text has to stay readable against
    /// a texture that was drawn to look like rock.
    fn backdrop(&self, p: &mut Painter, ctx: &MenuContext) {
        match ctx.background {
            Some((layer, aspect)) => {
                p.block_background(layer, aspect, [0.55, 0.58, 0.66, 1.0]);
                p.scrim([0.04, 0.05, 0.07, 0.70]);
            }
            None => p.scrim(widgets::SCRIM),
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
        p.button(rect, label, hovered, enabled);
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

    fn title(&self, p: &mut Painter, text: &str, y: f32) {
        p.text_centred(text, 0.0, y, 3.0, widgets::ACCENT);
    }

    fn build_main(&mut self, p: &mut Painter, cursor: Option<(f32, f32)>, ctx: &MenuContext) {
        self.backdrop(p, ctx);
        self.title(p, "PRIMITIVE", 0.62);
        p.text_centred(say(ctx, Msg::Subtitle), 0.0, 0.44, 1.0, widgets::TEXT_DIM);

        let width = 0.9;
        let height = 0.105;
        let gap = 0.03;
        let mut y = 0.20;
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
            y -= height + gap;
        }

        p.text_centred(ctx.version, 0.0, -0.82, 0.8, widgets::TEXT_DIM);
    }

    fn build_servers(&mut self, p: &mut Painter, cursor: Option<(f32, f32)>, ctx: &MenuContext) {
        self.backdrop(p, ctx);
        self.title(p, say(ctx, Msg::Servers), 0.86);

        let panel = Rect::new(-0.95, -0.30, 0.95, 0.66);
        p.panel(panel);

        if self.servers.servers.is_empty() {
            p.text_centred(
                say(ctx, Msg::NoServersYet),
                panel.centre_x(),
                0.25,
                1.0,
                widgets::TEXT_DIM,
            );
        }

        let row_height = 0.11;
        let mut y = panel.y1 - 0.03 - row_height;
        // Only as many rows as fit; the rest are reachable with the
        // arrow keys, which scroll the selection rather than the view.
        let visible = ((panel.height() - 0.06) / (row_height + 0.014)) as usize;
        let first = self.selected.saturating_sub(visible.saturating_sub(1));

        for index in first..self.servers.servers.len().min(first + visible) {
            let entry = &self.servers.servers[index];
            let rect = Rect::new(panel.x0 + 0.03, y, panel.x1 - 0.03, y + row_height);
            let selected = index == self.selected;
            let hovered = self.is_hovered(rect, cursor);

            p.quad(rect, if selected { widgets::ROW_SELECTED } else { widgets::ROW });
            if selected || hovered {
                p.border(rect, 0.003, if selected { widgets::ACCENT } else { widgets::BUTTON_EDGE });
            }
            p.row_labels(
                rect,
                0.025,
                &entry.name,
                if selected { widgets::TEXT } else { widgets::TEXT_DIM },
                &entry.address,
                widgets::TEXT_DIM,
            );

            // A row both selects and, on the already-selected row,
            // connects -- so a second click plays.
            let action = if selected {
                Action::Connect(index)
            } else {
                Action::Select(index)
            };
            self.hot.push((rect, action));
            y -= row_height + 0.014;
        }

        if let Some((text, good)) = self.notice_line(ctx) {
            let colour = if good { widgets::TEXT_GOOD } else { widgets::TEXT_BAD };
            p.text_centred(&text, 0.0, -0.36, 0.9, colour);
        }

        let any = !self.servers.servers.is_empty();
        let selected = self.selected;
        let width = 0.42;
        let height = 0.10;
        let row_y = -0.52;
        self.add_button(
            p,
            cursor,
            Rect::centred(-0.72, row_y, width, height),
            say(ctx, Msg::Play),
            Action::Connect(selected),
            any,
        );
        self.add_button(
            p,
            cursor,
            Rect::centred(-0.24, row_y, width, height),
            say(ctx, Msg::Add),
            Action::Add,
            true,
        );
        self.add_button(
            p,
            cursor,
            Rect::centred(0.24, row_y, width, height),
            say(ctx, Msg::Edit),
            Action::Edit(selected),
            any,
        );
        self.add_button(
            p,
            cursor,
            Rect::centred(0.72, row_y, width, height),
            say(ctx, Msg::Delete),
            Action::Delete(selected),
            any,
        );
        self.add_button(
            p,
            cursor,
            Rect::centred(0.0, -0.70, 0.5, height),
            say(ctx, Msg::Back),
            Action::Back,
            true,
        );

        p.text_centred(
            say(ctx, Msg::ServersHelp),
            0.0,
            -0.83,
            0.8,
            widgets::TEXT_DIM,
        );
    }

    fn build_form(&mut self, p: &mut Painter, cursor: Option<(f32, f32)>, ctx: &MenuContext, editing: bool) {
        self.backdrop(p, ctx);
        self.title(p, say(ctx, if editing { Msg::EditServer } else { Msg::AddServer }), 0.70);

        let panel = Rect::new(-0.95, -0.30, 0.95, 0.44);
        p.panel(panel);

        let label_x = panel.x0 + 0.05;
        let field = |y: f32| Rect::new(panel.x0 + 0.05, y, panel.x1 - 0.05, y + 0.11);

        p.text(say(ctx, Msg::Name), label_x, 0.30, 0.9, widgets::TEXT_DIM);
        let name_rect = field(0.14);
        p.field(
            name_rect,
            &self.name_input,
            self.focus == Field::Name,
            self.caret_visible(),
        );
        self.hot.push((name_rect, Action::Focus(Field::Name)));

        p.text(say(ctx, Msg::Address), label_x, -0.02, 0.9, widgets::TEXT_DIM);
        let address_rect = field(-0.18);
        p.field(
            address_rect,
            &self.address_input,
            self.focus == Field::Address,
            self.caret_visible(),
        );
        self.hot.push((address_rect, Action::Focus(Field::Address)));

        if let Some((text, good)) = self.notice_line(ctx) {
            let colour = if good { widgets::TEXT_GOOD } else { widgets::TEXT_BAD };
            p.text_centred(&text, 0.0, -0.38, 0.9, colour);
        } else {
            p.text_centred(
                say(ctx, Msg::AddressHelp),
                0.0,
                -0.38,
                0.8,
                widgets::TEXT_DIM,
            );
        }

        self.add_button(
            p,
            cursor,
            Rect::centred(-0.28, -0.56, 0.5, 0.10),
            say(ctx, Msg::Save),
            Action::Save,
            true,
        );
        self.add_button(
            p,
            cursor,
            Rect::centred(0.28, -0.56, 0.5, 0.10),
            say(ctx, Msg::Cancel),
            Action::Cancel,
            true,
        );

        p.text_centred(say(ctx, Msg::ServerFormHelp), 0.0, -0.74, 0.8, widgets::TEXT_DIM);
    }

    fn build_connecting(&mut self, p: &mut Painter, cursor: Option<(f32, f32)>, ctx: &MenuContext, label: &str) {
        self.backdrop(p, ctx);
        self.title(p, say(ctx, Msg::Connecting), 0.42);
        p.text_centred(label, 0.0, 0.16, 1.2, widgets::TEXT);
        self.add_button(
            p,
            cursor,
            Rect::centred(0.0, -0.20, 0.6, 0.10),
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
        self.backdrop(p, ctx);
        self.title(p, say(ctx, Msg::CannotConnect), 0.62);
        p.text_centred(label, 0.0, 0.38, 1.2, widgets::TEXT);

        // The reason is shown in full, wrapped. A truncated network error
        // tells the player nothing about what to fix.
        let mut y = 0.20;
        for line in widgets::wrap(reason, 52) {
            p.text_centred(&line, 0.0, y, 0.9, widgets::TEXT_BAD);
            y -= widgets::line_height(0.9);
        }

        self.add_button(
            p,
            cursor,
            Rect::centred(-0.28, -0.36, 0.5, 0.10),
            say(ctx, Msg::Retry),
            Action::Retry,
            true,
        );
        self.add_button(
            p,
            cursor,
            Rect::centred(0.28, -0.36, 0.5, 0.10),
            say(ctx, Msg::Back),
            Action::Back,
            true,
        );
    }

    fn build_paused(&mut self, p: &mut Painter, cursor: Option<(f32, f32)>, ctx: &MenuContext) {
        // Dimmed, not covered: the pause screen sits over the world, and
        // seeing where you left off is half of what makes it read as a
        // pause rather than a disconnect.
        p.scrim([0.02, 0.03, 0.05, 0.62]);
        self.title(p, say(ctx, Msg::Paused), 0.52);

        let width = 0.9;
        let height = 0.105;
        let mut y = 0.20;
        for (index, (label, action)) in [
            (say(ctx, Msg::Resume), Action::Resume),
            (say(ctx, Msg::Settings), Action::OpenSettings),
            (say(ctx, Msg::LeaveWorld), Action::LeaveWorld),
            (say(ctx, Msg::Quit), Action::Quit),
        ]
        .into_iter()
        .enumerate()
        {
            let rect = Rect::centred(0.0, y, width, height);
            self.add_menu_button(p, cursor, rect, label, action, index);
            y -= height + 0.03;
        }
    }
}

/// Who made what, in the order the screen shows them.
///
/// A table rather than a formatted block of text: the screen lays it out
/// in two columns, and a credit whose role is not spelled out is not
/// really a credit.
pub const CREDITS: &[(Msg, &str)] = &[
    (Msg::RoleTextures, "NYukichi.I"),
    (Msg::RoleCode, "Claude (Anthropic)"),
    (Msg::RoleCode, "George Perry Floyd Jr"),
    (Msg::RoleEngine, "Rust, wgpu, tokio"),
];

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
    /// Texture layer to tile behind the menus, and the window's aspect
    /// so the tiles reach both edges.
    ///
    /// `None` means "draw a plain backdrop": either the player has the
    /// wallpaper switched off, or there is a world behind the screen
    /// already, which is a better backdrop than any wallpaper.
    pub background: Option<(u32, f32)>,
}

/// The subset of the keyboard the menus care about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Up,
    Down,
    Enter,
    Escape,
    Tab,
    Backspace,
    Delete,
    Char(char),
}

/// How tall one row of the controls screen is.
///
/// Derived from how many actions there are rather than fixed, because
/// the list grows: the thirteenth binding was what pushed the last row
/// off the bottom of the panel, and the list does not scroll -- a row
/// that falls off is simply a key nobody can rebind, with nothing on
/// screen saying so.
///
/// The floor is where the text stops being comfortably readable. Past
/// that this screen needs scrolling rather than smaller print, and the
/// test in `keybinds` is what will say so.
pub fn controls_row_height() -> f32 {
    const PANEL_HEIGHT: f32 = 0.76 - -0.62;
    const TOP_PAD: f32 = 0.030;
    const GAP: f32 = 0.010;
    let available = PANEL_HEIGHT - TOP_PAD * 2.0;
    let rows = crate::ui::keybinds::Action::ALL.len() as f32;
    (available / rows - GAP).clamp(0.055, 0.098)
}


#[cfg(test)]
mod tests {
    use super::*;

    /// A context for `build`. The tests care about layout and hit
    /// testing, not about what is in the settings, so this is the
    /// defaults plus an empty world list unless a test says otherwise.
    struct Fixture {
        settings: ClientSettings,
        worlds: Worlds,
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
            }
        }

        fn ctx(&self) -> MenuContext<'_> {
            MenuContext {
                version: "test",
                font: crate::engine::texture::FontAtlas::for_test(),
                settings: &self.settings,
                worlds: &self.worlds,
                background: None,
            }
        }
    }

    fn build(menu: &mut Menu) -> Vec<crate::ui::hotbar::HotbarVertex> {
        let fixture = Fixture::new();
        menu.build(&fixture.ctx())
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
        assert_eq!(menu.name_input, "Server 1", "the form should be pre-filled");

        menu.name_input = "Renamed".to_string();
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

    #[test]
    fn typing_is_limited_to_characters_the_font_can_draw() {
        let mut menu = menu_with(0);
        menu.apply(Action::Add);
        menu.type_char('A');
        menu.type_char('\u{2603}'); // a snowman
        menu.type_char('\n');
        menu.type_char('B');
        assert_eq!(menu.name_input, "AB");
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
        assert_eq!(menu.name_input, "Дом ćma");
    }

    #[test]
    fn a_field_cannot_grow_without_bound() {
        let mut menu = menu_with(0);
        menu.apply(Action::Add);
        for _ in 0..500 {
            menu.type_char('x');
        }
        assert_eq!(menu.name_input.chars().count(), MAX_NAME);
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
        menu.name_input = "abc".to_string();
        menu.address_input = "xyz".to_string();
        menu.key(Key::Backspace);
        assert_eq!(menu.name_input, "ab");
        assert_eq!(menu.address_input, "xyz");
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
            menu.name_input = long.clone();
            menu.address_input = long.clone();
            menu.seed_input = "9".repeat(10);
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
                worlds.create(name, 100 + i as u32).unwrap();
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
                background: None,
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
        assert_eq!(menu.name_input, "Home");
        assert_eq!(menu.seed_input, "123");
    }

    #[test]
    fn a_seed_cannot_be_longer_than_a_u32() {
        let mut menu = Menu::new(ServerList::default());
        menu.apply(Action::NewWorld);
        menu.focus = Field::Seed;
        for _ in 0..40 {
            menu.type_char('9');
        }
        assert!(menu.seed_input.parse::<u64>().unwrap() > 0);
        assert!(menu.seed_input.chars().count() <= MAX_SEED_DIGITS);
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
    fn the_settings_screen_draws_a_row_for_every_setting() {
        let mut menu = Menu::new(ServerList::default());
        menu.screen = Screen::Settings;
        let mut fixture = Fixture::new();
        // With the wallpaper off its block choice is deliberately
        // inert, so turn it on to see every control at once.
        fixture.settings.menu_background = true;
        menu.build(&fixture.ctx());
        for setting in Setting::ALL {
            let has_control = menu.hot.iter().any(|(_, a)| match a {
                Action::Tweak(s, _) => *s == setting,
                Action::EditUsername | Action::CommitUsername => setting.is_text(),
                _ => false,
            });
            assert!(has_control, "{:?} has no way to change it", setting);
        }
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
    fn every_setting_shows_a_value_a_person_can_read() {
        let settings = ClientSettings::default();
        for setting in Setting::ALL {
            let value = setting.value(&settings);
            assert!(!value.is_empty(), "{setting:?} shows nothing");
            assert!(
                value.chars().all(|c| c.is_ascii_graphic() || c == ' '),
                "{setting:?} shows {value:?}, which the font cannot draw"
            );
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
        assert_eq!(menu.name_input, "player!");
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

    #[test]
    fn the_block_choice_is_inert_while_the_wallpaper_is_off() {
        // Shown, so the option is discoverable; not clickable, so it
        // cannot be changed to no effect.
        let mut fixture = Fixture::new();
        let mut menu = Menu::new(ServerList::default());
        menu.screen = Screen::Settings;

        menu.build(&fixture.ctx());
        let has_control = |m: &Menu| {
            m.hot.iter().any(|(_, a)| {
                matches!(a, Action::Tweak(Setting::MenuBackgroundBlock, _))
            })
        };
        assert!(!has_control(&menu), "changing it would do nothing");

        fixture.settings.menu_background = true;
        menu.build(&fixture.ctx());
        assert!(has_control(&menu));
    }

    #[test]
    fn the_background_block_cycles_through_the_offered_blocks() {
        let mut settings = ClientSettings::default();
        let mut seen = vec![settings.menu_background_block()];
        for _ in 1..crate::settings::MENU_BACKGROUND_BLOCKS.len() {
            Setting::MenuBackgroundBlock.step(&mut settings, 1);
            seen.push(settings.menu_background_block());
        }
        seen.sort();
        seen.dedup();
        assert_eq!(
            seen.len(),
            crate::settings::MENU_BACKGROUND_BLOCKS.len(),
            "stepping should reach every offered block"
        );

        // And it wraps rather than dead-ending.
        Setting::MenuBackgroundBlock.step(&mut settings, 1);
        assert_eq!(
            settings.menu_background_block(),
            crate::settings::MENU_BACKGROUND_BLOCKS[0]
        );
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
    const PANEL: (f32, f32) = (-0.60, 0.76);

    #[test]
    fn every_row_fits_inside_the_panel() {
        // Twice now a setting has been added and the list has run out of
        // the bottom of its own panel and over the buttons underneath.
        // The rows are sized from the panel rather than the other way
        // round, so this holds for any number of them -- which is the
        // number that keeps changing.
        const GAP: f32 = 0.012;
        const MARGIN: f32 = 0.035;
        const MAX_ROW: f32 = 0.105;
        let height = PANEL.1 - PANEL.0;
        let count = Setting::ALL.len() as f32;
        let room = height - 2.0 * MARGIN - (count - 1.0) * GAP;
        let row_height = (room / count).min(MAX_ROW);

        assert!(row_height > 0.03, "rows squeezed to {row_height}, too thin to read");
        let bottom = PANEL.1 - MARGIN - count * row_height - (count - 1.0) * GAP;
        assert!(
            bottom >= PANEL.0 - 1e-4,
            "{} settings run {:.3} past the bottom of the panel",
            Setting::ALL.len(),
            PANEL.0 - bottom
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
