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
use crate::ui::{self, Painter, Rect};
use crate::worlds::{self, Worlds};

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
    /// Who made what.
    Credits,
    /// A yes/no gate in front of something irreversible.
    ///
    /// The confirmed action is carried in the screen rather than
    /// remembered in a field, so it is impossible to arrive here and
    /// confirm something other than what was asked about.
    Confirm {
        question: String,
        detail: String,
        confirm_label: String,
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
    LocalViewDistance,
    MenuBackground,
    MenuBackgroundBlock,
}

impl Setting {
    /// Every setting on the screen, top to bottom.
    pub const ALL: [Setting; 10] = [
        Setting::Username,
        Setting::RenderDistance,
        Setting::Fov,
        Setting::Sensitivity,
        Setting::Vsync,
        Setting::Fog,
        Setting::AmbientOcclusion,
        Setting::LocalViewDistance,
        Setting::MenuBackground,
        Setting::MenuBackgroundBlock,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            Setting::Username => "NAME",
            Setting::RenderDistance => "RENDER DISTANCE",
            Setting::Fov => "FIELD OF VIEW",
            Setting::Sensitivity => "MOUSE SENSITIVITY",
            Setting::Vsync => "VSYNC",
            Setting::Fog => "FOG",
            Setting::AmbientOcclusion => "AMBIENT OCCLUSION",
            Setting::LocalViewDistance => "LOCAL WORLD DISTANCE",
            Setting::MenuBackground => "MENU BACKGROUND",
            Setting::MenuBackgroundBlock => "BACKGROUND BLOCK",
        }
    }

    /// What the row shows on the right.
    pub fn value(&self, settings: &ClientSettings) -> String {
        match self {
            Setting::Username => settings.username.clone(),
            Setting::RenderDistance => format!("{} chunks", settings.render_distance_chunks),
            Setting::Fov => format!("{:.0} deg", settings.fov_degrees),
            // Shown scaled up: the stored value is around 0.0025, and a
            // row reading "0.003" tells the player nothing about whether
            // a step made a difference.
            Setting::Sensitivity => format!("{:.0}", settings.mouse_sensitivity * 10_000.0),
            Setting::Vsync => on_off(settings.vsync),
            Setting::Fog => on_off(settings.fog_enabled),
            Setting::AmbientOcclusion => format!("{:.0}%", settings.ambient_occlusion * 100.0),
            Setting::LocalViewDistance => {
                format!("{} chunks", settings.singleplayer_view_distance_chunks)
            }
            Setting::MenuBackground => on_off(settings.menu_background),
            Setting::MenuBackgroundBlock => settings.menu_background_block.to_uppercase(),
        }
    }

    /// True for settings that are a switch rather than a range, so the
    /// screen can draw one wide button instead of a `-`/`+` pair.
    pub fn is_toggle(&self) -> bool {
        matches!(self, Setting::Vsync | Setting::Fog | Setting::MenuBackground)
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
        let d = delta as f32;
        match self {
            Setting::Username => {}
            Setting::RenderDistance => settings.render_distance_chunks += delta,
            Setting::Fov => settings.fov_degrees += 5.0 * d,
            Setting::Sensitivity => settings.mouse_sensitivity += 0.0002 * d,
            Setting::Vsync => settings.vsync = !settings.vsync,
            Setting::Fog => settings.fog_enabled = !settings.fog_enabled,
            Setting::AmbientOcclusion => settings.ambient_occlusion += 0.05 * d,
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

fn on_off(value: bool) -> String {
    if value { "ON".to_string() } else { "OFF".to_string() }
}

/// Something the player asked for. `main.rs` carries these out; nothing
/// in this module performs them.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    OpenWorlds,
    OpenServers,
    OpenSettings,
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
    pub screen: Screen,
    pub name_input: String,
    pub address_input: String,
    pub seed_input: String,
    pub focus: Field,
    /// True while the name row of the settings screen is being typed
    /// into. The row turns into a text field and swallows keys.
    pub editing_username: bool,
    /// A one-line note under the list ("server added", "address is
    /// required"), cleared when the player navigates away.
    pub notice: Option<(String, bool)>,
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

impl Menu {
    pub fn new(servers: ServerList) -> Self {
        Self {
            servers,
            selected: 0,
            world_selected: 0,
            world_count: 0,
            screen: Screen::Main,
            name_input: String::new(),
            address_input: String::new(),
            seed_input: String::new(),
            focus: Field::Name,
            editing_username: false,
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
    }

    pub fn move_world_selection(&mut self, delta: i32) {
        let count = self.world_count as i32;
        if count == 0 {
            return;
        }
        self.world_selected = (((self.world_selected as i32 + delta) % count + count) % count) as usize;
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
        if !(c.is_ascii_graphic() || c == ' ') {
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
                    self.notice = Some((format!("removed {}", removed.name), true));
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
                    question: "DELETE THIS WORLD?".to_string(),
                    detail: String::new(),
                    confirm_label: "DELETE".to_string(),
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
                self.came_from = Box::new(self.screen.clone());
                self.screen = Screen::Settings;
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
            self.notice = Some(("an address is required".to_string(), false));
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
                self.notice = Some((format!("saved {name}"), true));
            }
            _ => {
                self.servers.servers.push(entry);
                self.selected = self.servers.servers.len() - 1;
                self.notice = Some((format!("added {name}"), true));
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
    pub fn build(&mut self, ctx: &MenuContext) -> Vec<crate::hotbar::HotbarVertex> {
        self.hot.clear();
        self.set_world_count(ctx.worlds.list().len());
        let mut p = Painter::new(ctx.font);
        let hover = self.cursor;

        match self.screen.clone() {
            Screen::Main => self.build_main(&mut p, hover, ctx),
            Screen::Worlds => self.build_worlds(&mut p, hover, ctx),
            Screen::CreatingWorld => self.build_world_form(&mut p, hover, ctx),
            Screen::Servers => self.build_servers(&mut p, hover, ctx),
            Screen::Editing(existing) => self.build_form(&mut p, hover, ctx, existing.is_some()),
            Screen::Settings => self.build_settings(&mut p, hover, ctx),
            Screen::Credits => self.build_credits(&mut p, hover, ctx),
            Screen::Confirm {
                question,
                detail,
                confirm_label,
                action,
            } => {
                self.build_confirm(&mut p, hover, ctx, &question, &detail, &confirm_label, *action)
            }
            Screen::Connecting { label } => self.build_connecting(&mut p, hover, ctx, &label),
            Screen::Failed { label, reason } => {
                self.build_failed(&mut p, hover, ctx, &label, &reason)
            }
            Screen::Paused => self.build_paused(&mut p, hover),
        }

        p.into_vertices()
    }

    fn build_worlds(&mut self, p: &mut Painter, cursor: Option<(f32, f32)>, ctx: &MenuContext) {
        self.backdrop(p, ctx);
        self.title(p, "WORLDS", 0.86);

        let panel = Rect::new(-0.95, -0.30, 0.95, 0.66);
        p.panel(panel);

        let worlds = ctx.worlds.list();
        if worlds.is_empty() {
            p.text_centred(
                "no worlds yet -- press NEW",
                panel.centre_x(),
                0.25,
                1.0,
                ui::TEXT_DIM,
            );
        }

        let row_height = 0.11;
        let mut y = panel.y1 - 0.03 - row_height;
        let visible = ((panel.height() - 0.06) / (row_height + 0.014)) as usize;
        let first = self.world_selected.saturating_sub(visible.saturating_sub(1));
        let now = worlds::unix_now();

        for index in first..worlds.len().min(first + visible) {
            let world = &worlds[index];
            let rect = Rect::new(panel.x0 + 0.03, y, panel.x1 - 0.03, y + row_height);
            let selected = index == self.world_selected;

            p.quad(rect, if selected { ui::ROW_SELECTED } else { ui::ROW });
            if selected || self.is_hovered(rect, cursor) {
                p.border(rect, 0.003, if selected { ui::ACCENT } else { ui::BUTTON_EDGE });
            }
            let detail = format!(
                "seed {}   {}",
                world.seed.unwrap_or(ctx.settings.singleplayer_seed),
                world.played_description(now)
            );
            p.row_labels(
                rect,
                0.025,
                &world.name,
                if selected { ui::TEXT } else { ui::TEXT_DIM },
                &detail,
                ui::TEXT_DIM,
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

        if let Some((text, good)) = self.notice.clone() {
            let colour = if good { ui::TEXT_GOOD } else { ui::TEXT_BAD };
            p.text_centred(&text, 0.0, -0.36, 0.9, colour);
        }

        let any = !worlds.is_empty();
        let selected = self.world_selected;
        let height = 0.10;
        self.add_button(
            p,
            cursor,
            Rect::centred(-0.62, -0.52, 0.56, height),
            "PLAY",
            Action::PlayWorld(selected),
            any,
        );
        self.add_button(
            p,
            cursor,
            Rect::centred(0.0, -0.52, 0.56, height),
            "NEW",
            Action::NewWorld,
            true,
        );
        self.add_button(
            p,
            cursor,
            Rect::centred(0.62, -0.52, 0.56, height),
            "DELETE",
            Action::AskDeleteWorld(selected),
            any,
        );
        self.add_button(
            p,
            cursor,
            Rect::centred(0.0, -0.70, 0.5, height),
            "BACK",
            Action::Back,
            true,
        );

        p.text_centred(
            "up/down select   enter play   N new   del remove",
            0.0,
            -0.83,
            0.8,
            ui::TEXT_DIM,
        );
    }

    fn build_world_form(&mut self, p: &mut Painter, cursor: Option<(f32, f32)>, ctx: &MenuContext) {
        self.backdrop(p, ctx);
        self.title(p, "NEW WORLD", 0.70);

        let panel = Rect::new(-0.95, -0.30, 0.95, 0.44);
        p.panel(panel);

        let label_x = panel.x0 + 0.05;
        let field = |y: f32| Rect::new(panel.x0 + 0.05, y, panel.x1 - 0.05, y + 0.11);

        p.text("NAME", label_x, 0.30, 0.9, ui::TEXT_DIM);
        let name_rect = field(0.14);
        p.field(
            name_rect,
            &self.name_input,
            self.focus == Field::Name,
            self.caret_visible(),
        );
        self.hot.push((name_rect, Action::Focus(Field::Name)));

        p.text("SEED", label_x, -0.02, 0.9, ui::TEXT_DIM);
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

        if let Some((text, good)) = self.notice.clone() {
            let colour = if good { ui::TEXT_GOOD } else { ui::TEXT_BAD };
            p.text_centred(&text, 0.0, -0.38, 0.9, colour);
        } else {
            p.text_centred(
                "the seed decides the terrain -- leave it for the default",
                0.0,
                -0.38,
                0.8,
                ui::TEXT_DIM,
            );
        }

        self.add_button(
            p,
            cursor,
            Rect::centred(-0.28, -0.56, 0.5, 0.10),
            "CREATE",
            Action::CreateWorld,
            true,
        );
        self.add_button(
            p,
            cursor,
            Rect::centred(0.28, -0.56, 0.5, 0.10),
            "CANCEL",
            Action::Cancel,
            true,
        );

        p.text_centred(
            "tab switches field   enter creates   esc cancels",
            0.0,
            -0.74,
            0.8,
            ui::TEXT_DIM,
        );
    }

    fn build_settings(&mut self, p: &mut Painter, cursor: Option<(f32, f32)>, ctx: &MenuContext) {
        self.backdrop(p, ctx);
        self.title(p, "SETTINGS", 0.90);

        let panel = Rect::new(-1.15, -0.60, 1.15, 0.76);
        p.panel(panel);

        let row_height = 0.105;
        let mut y = panel.y1 - 0.035 - row_height;

        for setting in Setting::ALL {
            let row = Rect::new(panel.x0 + 0.03, y, panel.x1 - 0.03, y + row_height);

            if setting.is_text() {
                p.quad(row, ui::ROW);
                p.label_left(row, setting.label(), 0.025, 1.0, ui::TEXT);
                let field_rect = Rect::new(row.x1 - 0.78, row.y0 + 0.012, row.x1 - 0.02, row.y1 - 0.012);
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
                p.setting_row(row, setting.label(), &setting.value(ctx.settings), enabled);
                if setting.is_toggle() {
                    // One wide button: a switch has no "less" and "more".
                    let toggle = Rect::new(row.x1 - 0.26, row.y0 + 0.012, row.x1 - 0.02, row.y1 - 0.012);
                    self.add_button(p, cursor, toggle, "TOGGLE", Action::Tweak(setting, 1), enabled);
                } else {
                    let minus = Rect::new(row.x1 - 0.26, row.y0 + 0.012, row.x1 - 0.15, row.y1 - 0.012);
                    let plus = Rect::new(row.x1 - 0.13, row.y0 + 0.012, row.x1 - 0.02, row.y1 - 0.012);
                    self.add_button(p, cursor, minus, "-", Action::Tweak(setting, -1), enabled);
                    self.add_button(p, cursor, plus, "+", Action::Tweak(setting, 1), enabled);
                }
            }

            y -= row_height + 0.012;
        }

        if let Some((text, good)) = self.notice.clone() {
            let colour = if good { ui::TEXT_GOOD } else { ui::TEXT_BAD };
            p.text_centred(&text, 0.0, -0.66, 0.9, colour);
        }

        self.add_button(
            p,
            cursor,
            Rect::centred(0.0, -0.78, 0.6, 0.10),
            "DONE",
            Action::Back,
            true,
        );
        p.text_centred(
            "changes apply at once and are saved when you leave",
            0.0,
            -0.90,
            0.8,
            ui::TEXT_DIM,
        );
    }

    fn build_credits(&mut self, p: &mut Painter, cursor: Option<(f32, f32)>, ctx: &MenuContext) {
        self.backdrop(p, ctx);
        self.title(p, "CREDITS", 0.74);

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
            p.label_left(row, role, 0.02, 0.9, ui::TEXT_DIM);
            let width = ui::measure(who, 1.1);
            p.label_left(
                Rect::new(row.x1 - width, row.y0, row.x1, row.y1),
                who,
                0.0,
                1.1,
                ui::TEXT,
            );
            y -= row_height;
        }

        p.text_centred(ctx.version, 0.0, panel.y0 + 0.09, 0.8, ui::TEXT_DIM);

        self.add_button(
            p,
            cursor,
            Rect::centred(0.0, -0.52, 0.6, 0.105),
            "BACK",
            Action::Back,
            true,
        );
    }

    fn build_confirm(
        &mut self,
        p: &mut Painter,
        cursor: Option<(f32, f32)>,
        ctx: &MenuContext,
        question: &str,
        detail: &str,
        confirm_label: &str,
        action: Action,
    ) {
        self.backdrop(p, ctx);
        self.title(p, question, 0.42);
        if !detail.is_empty() {
            p.text_centred(detail, 0.0, 0.18, 1.2, ui::TEXT);
        }
        p.text_centred("this cannot be undone", 0.0, 0.02, 0.9, ui::TEXT_BAD);

        // Cancel first and on the left, where the eye lands: the safe
        // answer should be the easy one to hit.
        self.add_button(
            p,
            cursor,
            Rect::centred(-0.32, -0.28, 0.56, 0.10),
            "CANCEL",
            Action::Cancel,
            true,
        );
        self.add_button(
            p,
            cursor,
            Rect::centred(0.32, -0.28, 0.56, 0.10),
            confirm_label,
            action,
            true,
        );
        p.text_centred("Y confirms   N or esc cancels", 0.0, -0.46, 0.8, ui::TEXT_DIM);
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
            None => p.scrim(ui::SCRIM),
        }
    }

    fn is_hovered(&self, rect: Rect, cursor: Option<(f32, f32)>) -> bool {
        cursor.map_or(false, |(x, y)| rect.contains(x, y))
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
        p.text_centred(text, 0.0, y, 3.0, ui::ACCENT);
    }

    fn build_main(&mut self, p: &mut Painter, cursor: Option<(f32, f32)>, ctx: &MenuContext) {
        self.backdrop(p, ctx);
        self.title(p, "PRIMITIVE", 0.62);
        p.text_centred("a voxel world", 0.0, 0.44, 1.0, ui::TEXT_DIM);

        let width = 0.9;
        let height = 0.105;
        let gap = 0.03;
        let mut y = 0.20;
        for (index, (label, action)) in [
            ("SINGLEPLAYER", Action::OpenWorlds),
            ("MULTIPLAYER", Action::OpenServers),
            ("SETTINGS", Action::OpenSettings),
            ("CREDITS", Action::OpenCredits),
            ("QUIT", Action::Quit),
        ]
        .into_iter()
        .enumerate()
        {
            let rect = Rect::centred(0.0, y, width, height);
            self.add_menu_button(p, cursor, rect, label, action, index);
            y -= height + gap;
        }

        p.text_centred(ctx.version, 0.0, -0.82, 0.8, ui::TEXT_DIM);
    }

    fn build_servers(&mut self, p: &mut Painter, cursor: Option<(f32, f32)>, ctx: &MenuContext) {
        self.backdrop(p, ctx);
        self.title(p, "SERVERS", 0.86);

        let panel = Rect::new(-0.95, -0.30, 0.95, 0.66);
        p.panel(panel);

        if self.servers.servers.is_empty() {
            p.text_centred(
                "no servers yet -- press ADD",
                panel.centre_x(),
                0.25,
                1.0,
                ui::TEXT_DIM,
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

            p.quad(rect, if selected { ui::ROW_SELECTED } else { ui::ROW });
            if selected || hovered {
                p.border(rect, 0.003, if selected { ui::ACCENT } else { ui::BUTTON_EDGE });
            }
            p.row_labels(
                rect,
                0.025,
                &entry.name,
                if selected { ui::TEXT } else { ui::TEXT_DIM },
                &entry.address,
                ui::TEXT_DIM,
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

        if let Some((text, good)) = self.notice.clone() {
            let colour = if good { ui::TEXT_GOOD } else { ui::TEXT_BAD };
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
            "PLAY",
            Action::Connect(selected),
            any,
        );
        self.add_button(
            p,
            cursor,
            Rect::centred(-0.24, row_y, width, height),
            "ADD",
            Action::Add,
            true,
        );
        self.add_button(
            p,
            cursor,
            Rect::centred(0.24, row_y, width, height),
            "EDIT",
            Action::Edit(selected),
            any,
        );
        self.add_button(
            p,
            cursor,
            Rect::centred(0.72, row_y, width, height),
            "DELETE",
            Action::Delete(selected),
            any,
        );
        self.add_button(
            p,
            cursor,
            Rect::centred(0.0, -0.70, 0.5, height),
            "BACK",
            Action::Back,
            true,
        );

        p.text_centred(
            "up/down select   enter play   A add   E edit   del remove",
            0.0,
            -0.83,
            0.8,
            ui::TEXT_DIM,
        );
    }

    fn build_form(&mut self, p: &mut Painter, cursor: Option<(f32, f32)>, ctx: &MenuContext, editing: bool) {
        self.backdrop(p, ctx);
        self.title(p, if editing { "EDIT SERVER" } else { "ADD SERVER" }, 0.70);

        let panel = Rect::new(-0.95, -0.30, 0.95, 0.44);
        p.panel(panel);

        let label_x = panel.x0 + 0.05;
        let field = |y: f32| Rect::new(panel.x0 + 0.05, y, panel.x1 - 0.05, y + 0.11);

        p.text("NAME", label_x, 0.30, 0.9, ui::TEXT_DIM);
        let name_rect = field(0.14);
        p.field(
            name_rect,
            &self.name_input,
            self.focus == Field::Name,
            self.caret_visible(),
        );
        self.hot.push((name_rect, Action::Focus(Field::Name)));

        p.text("ADDRESS", label_x, -0.02, 0.9, ui::TEXT_DIM);
        let address_rect = field(-0.18);
        p.field(
            address_rect,
            &self.address_input,
            self.focus == Field::Address,
            self.caret_visible(),
        );
        self.hot.push((address_rect, Action::Focus(Field::Address)));

        if let Some((text, good)) = self.notice.clone() {
            let colour = if good { ui::TEXT_GOOD } else { ui::TEXT_BAD };
            p.text_centred(&text, 0.0, -0.38, 0.9, colour);
        } else {
            p.text_centred(
                "host:port  --  the port defaults to 7878",
                0.0,
                -0.38,
                0.8,
                ui::TEXT_DIM,
            );
        }

        self.add_button(
            p,
            cursor,
            Rect::centred(-0.28, -0.56, 0.5, 0.10),
            "SAVE",
            Action::Save,
            true,
        );
        self.add_button(
            p,
            cursor,
            Rect::centred(0.28, -0.56, 0.5, 0.10),
            "CANCEL",
            Action::Cancel,
            true,
        );

        p.text_centred("tab switches field   enter saves   esc cancels", 0.0, -0.74, 0.8, ui::TEXT_DIM);
    }

    fn build_connecting(&mut self, p: &mut Painter, cursor: Option<(f32, f32)>, ctx: &MenuContext, label: &str) {
        self.backdrop(p, ctx);
        self.title(p, "CONNECTING", 0.42);
        p.text_centred(label, 0.0, 0.16, 1.2, ui::TEXT);
        self.add_button(
            p,
            cursor,
            Rect::centred(0.0, -0.20, 0.6, 0.10),
            "CANCEL",
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
        self.title(p, "CANNOT CONNECT", 0.62);
        p.text_centred(label, 0.0, 0.38, 1.2, ui::TEXT);

        // The reason is shown in full, wrapped. A truncated network error
        // tells the player nothing about what to fix.
        let mut y = 0.20;
        for line in ui::wrap(reason, 52) {
            p.text_centred(&line, 0.0, y, 0.9, ui::TEXT_BAD);
            y -= ui::line_height(0.9);
        }

        self.add_button(
            p,
            cursor,
            Rect::centred(-0.28, -0.36, 0.5, 0.10),
            "RETRY",
            Action::Retry,
            true,
        );
        self.add_button(
            p,
            cursor,
            Rect::centred(0.28, -0.36, 0.5, 0.10),
            "BACK",
            Action::Back,
            true,
        );
    }

    fn build_paused(&mut self, p: &mut Painter, cursor: Option<(f32, f32)>) {
        // Dimmed, not covered: the pause screen sits over the world, and
        // seeing where you left off is half of what makes it read as a
        // pause rather than a disconnect.
        p.scrim([0.02, 0.03, 0.05, 0.62]);
        self.title(p, "PAUSED", 0.52);

        let width = 0.9;
        let height = 0.105;
        let mut y = 0.20;
        for (index, (label, action)) in [
            ("RESUME", Action::Resume),
            ("SETTINGS", Action::OpenSettings),
            ("LEAVE WORLD", Action::LeaveWorld),
            ("QUIT", Action::Quit),
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
pub const CREDITS: &[(&str, &str)] = &[
    ("TEXTURES", "NYukichi.I"),
    ("CODE", "Claude (Anthropic)"),
    ("ENGINE", "Rust, wgpu, tokio"),
];

/// What the menus need to read in order to draw themselves.
///
/// Borrowed per frame rather than owned, because the settings and the
/// world list are owned by `main.rs` -- the menu shows them and reports
/// what the player asked for, but never mutates them behind its back.
pub struct MenuContext<'a> {
    pub version: &'a str,
    /// Where the font lives in the texture array.
    pub font: crate::texture::FontAtlas,
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
                font: crate::texture::FontAtlas::for_test(),
                settings: &self.settings,
                worlds: &self.worlds,
                background: None,
            }
        }
    }

    fn build(menu: &mut Menu) -> Vec<crate::hotbar::HotbarVertex> {
        let fixture = Fixture::new();
        menu.build(&fixture.ctx())
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
                question: "DELETE THIS WORLD?".into(),
                detail: "Home".into(),
                confirm_label: "DELETE".into(),
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
                question: long.clone(),
                detail: long.clone(),
                confirm_label: long.clone(),
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
                font: crate::texture::FontAtlas::for_test(),
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
            assert!(!role.is_empty() && !who.is_empty());
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
