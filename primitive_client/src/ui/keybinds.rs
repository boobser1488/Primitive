//! What each key does, and how that survives a restart.
//!
//! ## Why the indirection
//!
//! Every action used to name its key inline -- `is_down(KeyCode::KeyW)`
//! scattered through the frame loop, `KeyCode::KeyI` in the event match.
//! That is fine right up until someone wants a different layout, at
//! which point the answer is "edit the source", and it is silently wrong
//! for anyone not on QWERTY: `KeyCode` is a *physical* position, so a
//! player on AZERTY walks forward with the key labelled Z.
//!
//! ## Why names rather than codes on disk
//!
//! `KeyCode` has no stable numeric form and no serde support, and a
//! settings file full of integers is a settings file nobody can edit by
//! hand. The table below maps both ways; anything not in it cannot be
//! bound, which is the point -- an unbindable key is better than a
//! binding that silently does nothing.

use serde::{Deserialize, Serialize};
// The game's own key type, not the window library's. Aliased to the
// name the table below already used, because every entry means exactly
// what it did before -- a physical position -- and renaming sixty rows
// to say so would be churn. See `crate::platform::Key`.
use crate::platform::Key as KeyCode;

use crate::ui::lang::{Language, Msg};

/// Something the player can bind a key to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Forward,
    Back,
    Left,
    Right,
    Jump,
    Sprint,
    /// **On a horse: walk, and get down.** Held with forward, the horse
    /// walks rather than trots (`horse::Gait`); pressed with the horse
    /// standing, the rider gets off. A key of its own because the two it
    /// might have borrowed are the horse's: the jump key jumps it and the
    /// sprint key gallops it. On foot it does nothing -- see `Keybinds`'s
    /// note on keys that mean something only somewhere.
    Rein,
    /// Held to lay a *single layer* of loose material instead of a
    /// whole block. See `types::layer_placement`.
    Inventory,
    Drop,
    /// Eat whatever is in the selected hotbar slot.
    ///
    /// A key rather than a gesture on the food itself, and that is the
    /// decision worth writing down. The alternatives were right-click
    /// while holding it -- which collides with placing a block, and
    /// every food here is unplaceable but *some future one might not
    /// be* -- and a button on the inventory screen, which means opening
    /// a screen to do the most ordinary thing in the game. A key you
    /// press while walking is what eating actually is.
    Eat,
    Respawn,
    ToggleFog,
    ToggleStats,
    /// Borderless fullscreen, on and off.
    ToggleFullscreen,
    /// The journal, open at the map.
    Map,
    /// The journal, open at the recipe book.
    ///
    /// Two keys for one screen, because the two pages are asked for by
    /// two different questions -- "where am I" and "how do I make this"
    /// -- and a player should not have to open the map to reach the book.
    /// Tab turns between them once either is open.
    Recipes,
    /// The journal, open at the give menu.
    ///
    /// Its own key for the same reason the book has one: a screen only
    /// reachable by opening another and turning the page is a screen
    /// nobody opens. See `ui::give_screen` for why the menu is a page of
    /// the journal at all.
    Give,
    /// The whole heads-up display off and on: hotbar, gauges, notices.
    ///
    /// Tab, and outside the journal only -- inside it Tab turns the page, as
    /// it always has. A player taking a picture, or just looking at the
    /// world, wants the world with nothing over it; a key that does the same
    /// thing the pause menu would need three clicks for is what the request
    /// ("скрытие и открытие hud на tab") was for.
    ToggleHud,
}

impl Action {
    /// Every action, in the order the controls screen lists them.
    pub const ALL: [Action; 18] = [
        Action::Forward,
        Action::Back,
        Action::Left,
        Action::Right,
        Action::Jump,
        Action::Sprint,
        Action::Rein,
        Action::Inventory,
        Action::Drop,
        Action::Eat,
        Action::Respawn,
        Action::ToggleFog,
        Action::ToggleStats,
        Action::ToggleFullscreen,
        Action::Map,
        Action::Recipes,
        Action::Give,
        Action::ToggleHud,
    ];

    /// What the controls screen calls this action, in the language the
    /// player has chosen. The key *names* (SPACE, L SHIFT) stay as they
    /// are printed on the keyboard.
    pub fn label(self, language: Language) -> &'static str {
        language.text(self.msg())
    }

    /// The action's row in the language table.
    fn msg(self) -> Msg {
        match self {
            Action::Forward => Msg::WalkForward,
            Action::Back => Msg::WalkBack,
            Action::Left => Msg::StrafeLeft,
            Action::Right => Msg::StrafeRight,
            Action::Jump => Msg::Jump,
            Action::Sprint => Msg::Sprint,
            Action::Rein => Msg::Rein,
            Action::Inventory => Msg::Inventory,
            Action::Drop => Msg::DropItem,
            Action::Eat => Msg::Eat,
            Action::Respawn => Msg::Respawn,
            Action::ToggleFog => Msg::ToggleFog,
            Action::ToggleStats => Msg::ToggleStats,
            Action::ToggleFullscreen => Msg::Fullscreen,
            Action::Map => Msg::MapTab,
            Action::Recipes => Msg::RecipesTab,
            Action::Give => Msg::GiveTab,
            Action::ToggleHud => Msg::ToggleHud,
        }
    }

    /// The field name used in the settings file.
    fn key(self) -> &'static str {
        match self {
            Action::Forward => "forward",
            Action::Back => "back",
            Action::Left => "left",
            Action::Right => "right",
            Action::Jump => "jump",
            Action::Sprint => "sprint",
            Action::Rein => "rein",
            Action::Inventory => "inventory",
            Action::Drop => "drop",
            Action::Eat => "eat",
            Action::Respawn => "respawn",
            Action::ToggleFog => "toggle_fog",
            Action::ToggleStats => "toggle_stats",
            Action::ToggleFullscreen => "toggle_fullscreen",
            Action::Map => "map",
            Action::Recipes => "recipes",
            Action::Give => "give",
            Action::ToggleHud => "toggle_hud",
        }
    }

    fn default_key(self) -> KeyCode {
        match self {
            Action::Forward => KeyCode::KeyW,
            Action::Back => KeyCode::KeyS,
            Action::Left => KeyCode::KeyA,
            Action::Right => KeyCode::KeyD,
            Action::Jump => KeyCode::Space,
            Action::Sprint => KeyCode::ShiftLeft,
            // C, where a crouch lives in every game that has one: the
            // gesture a hand makes toward the reins.
            Action::Rein => KeyCode::KeyC,
            Action::Inventory => KeyCode::KeyI,
            Action::Drop => KeyCode::KeyQ,
            // Next to the drop key, because the two are the same
            // gesture aimed at opposite ends: get rid of this, or use
            // it up.
            Action::Eat => KeyCode::KeyE,
            Action::Respawn => KeyCode::KeyR,
            Action::ToggleFog => KeyCode::KeyF,
            Action::ToggleStats => KeyCode::F3,
            Action::ToggleFullscreen => KeyCode::F11,
            // M for the map because every game with one uses it, and B
            // for the book beside it on the bottom row.
            Action::Map => KeyCode::KeyM,
            Action::Recipes => KeyCode::KeyB,
            // G for give, which is the word the command has always used
            // and the letter nothing else on the board wanted.
            Action::Give => KeyCode::KeyG,
            Action::ToggleHud => KeyCode::Tab,
        }
    }
}

/// The bindings, as they are stored and used.
///
/// A plain map keyed by the settings-file name, so an unknown entry in
/// the file is ignored rather than fatal and a missing one falls back to
/// the default.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Keybinds {
    bound: std::collections::BTreeMap<String, String>,
}

impl Default for Keybinds {
    fn default() -> Self {
        let mut bound = std::collections::BTreeMap::new();
        for action in Action::ALL {
            bound.insert(
                action.key().to_string(),
                key_name(action.default_key()).to_string(),
            );
        }
        Self { bound }
    }
}

impl Keybinds {
    /// The key bound to an action, if it has one.
    ///
    /// `None` is a real state, not an error: binding a key that another
    /// action already held leaves that other action with nothing, and
    /// the controls screen says so. Pretending it fell back to its
    /// default would put two actions on one key again, which is the bug
    /// this returns an `Option` to avoid.
    ///
    /// A *missing* entry is different from an unbound one -- it means a
    /// settings file written before this action existed, and it gets the
    /// default.
    pub fn key(&self, action: Action) -> Option<KeyCode> {
        match self.bound.get(action.key()) {
            Some(name) if name == NONE => None,
            Some(name) => key_from_name(name),
            None => Some(action.default_key()),
        }
    }

    /// What to show on the controls screen.
    pub fn label(&self, action: Action) -> &'static str {
        match self.key(action) {
            Some(key) => key_name(key),
            None => NONE,
        }
    }

    /// Binds a key, taking it off whatever else had it.
    ///
    /// Stealing rather than refusing: a player rebinding forward to `E`
    /// when `E` is already something else means they want it on forward,
    /// and a refusal leaves them hunting for what is holding it. The
    /// action that lost its key falls back to its default, which is
    /// visible on the same screen.
    pub fn bind(&mut self, action: Action, key: KeyCode) {
        let name = key_name(key);
        if name == UNKNOWN {
            return;
        }
        for other in Action::ALL {
            if other != action && self.key(other) == Some(key) {
                // Explicitly unbound, not merely forgotten: a forgotten
                // entry falls back to its default, which is usually the
                // very key that was just taken.
                self.bound.insert(other.key().to_string(), NONE.to_string());
            }
        }
        self.bound.insert(action.key().to_string(), name.to_string());
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Drops entries that no longer name a real action or a real key, so
    /// a file edited by hand cannot leave a binding that does nothing.
    pub fn sanitize(&mut self) {
        self.bound.retain(|action, key| {
            Action::ALL.iter().any(|a| a.key() == action)
                && (key == NONE || key_from_name(key).is_some())
        });
    }
}

const UNKNOWN: &str = "?";
/// What an action with no key at all is written as.
const NONE: &str = "--";

/// Keys a player may bind, and what to call them.
///
/// Deliberately not exhaustive. Modifiers that the window manager eats,
/// and keys with no printed label, are worse than useless as bindings --
/// they look bound and do nothing.
const KEYS: &[(KeyCode, &str)] = &[
    (KeyCode::KeyA, "A"), (KeyCode::KeyB, "B"), (KeyCode::KeyC, "C"),
    (KeyCode::KeyD, "D"), (KeyCode::KeyE, "E"), (KeyCode::KeyF, "F"),
    (KeyCode::KeyG, "G"), (KeyCode::KeyH, "H"), (KeyCode::KeyI, "I"),
    (KeyCode::KeyJ, "J"), (KeyCode::KeyK, "K"), (KeyCode::KeyL, "L"),
    (KeyCode::KeyM, "M"), (KeyCode::KeyN, "N"), (KeyCode::KeyO, "O"),
    (KeyCode::KeyP, "P"), (KeyCode::KeyQ, "Q"), (KeyCode::KeyR, "R"),
    (KeyCode::KeyS, "S"), (KeyCode::KeyT, "T"), (KeyCode::KeyU, "U"),
    (KeyCode::KeyV, "V"), (KeyCode::KeyW, "W"), (KeyCode::KeyX, "X"),
    (KeyCode::KeyY, "Y"), (KeyCode::KeyZ, "Z"),
    (KeyCode::Space, "SPACE"),
    (KeyCode::ShiftLeft, "L SHIFT"),
    (KeyCode::ShiftRight, "R SHIFT"),
    (KeyCode::ControlLeft, "L CTRL"),
    (KeyCode::ControlRight, "R CTRL"),
    (KeyCode::AltLeft, "L ALT"),
    (KeyCode::Tab, "TAB"),
    (KeyCode::CapsLock, "CAPS"),
    (KeyCode::ArrowUp, "UP"),
    (KeyCode::ArrowDown, "DOWN"),
    (KeyCode::ArrowLeft, "LEFT"),
    (KeyCode::ArrowRight, "RIGHT"),
    (KeyCode::F1, "F1"), (KeyCode::F2, "F2"), (KeyCode::F3, "F3"),
    (KeyCode::F4, "F4"), (KeyCode::F5, "F5"), (KeyCode::F6, "F6"),
    (KeyCode::F7, "F7"), (KeyCode::F8, "F8"),
    (KeyCode::F9, "F9"), (KeyCode::F10, "F10"), (KeyCode::F11, "F11"),
    (KeyCode::F12, "F12"),
];

pub fn key_name(key: KeyCode) -> &'static str {
    KEYS.iter()
        .find(|(code, _)| *code == key)
        .map(|(_, name)| *name)
        .unwrap_or(UNKNOWN)
}

pub fn key_from_name(name: &str) -> Option<KeyCode> {
    KEYS.iter()
        .find(|(_, label)| label.eq_ignore_ascii_case(name))
        .map(|(code, _)| *code)
}

/// Whether a key may be bound at all.
pub fn is_bindable(key: KeyCode) -> bool {
    key_name(key) != UNKNOWN
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_bindable_key_is_one_a_backend_can_actually_produce() {
        // The failure this guards against: a row on the controls screen
        // offering a key that no platform backend ever emits, so the
        // binding takes and then never fires. The table below is the
        // game's promise; the backend has to be able to keep it.
        for (key, label) in KEYS {
            assert!(
                crate::platform::winit_backend::can_produce(*key),
                "{label} ({key:?}) is offered as a binding but winit never reports it",
            );
        }
    }

    #[test]
    fn the_defaults_are_the_layout_the_game_shipped_with() {
        let binds = Keybinds::default();
        assert_eq!(binds.key(Action::Forward), Some(KeyCode::KeyW));
        assert_eq!(binds.key(Action::Jump), Some(KeyCode::Space));
        assert_eq!(binds.key(Action::Sprint), Some(KeyCode::ShiftLeft));
        assert_eq!(binds.key(Action::Inventory), Some(KeyCode::KeyI));
    }

    #[test]
    fn every_action_has_a_key_and_every_key_has_a_name() {
        let binds = Keybinds::default();
        for action in Action::ALL {
            let key = binds.key(action).expect("every action starts bound");
            assert!(is_bindable(key), "{:?} has an unnameable key", action);
            assert_ne!(binds.label(action), UNKNOWN);
            for language in Language::ALL {
                assert!(!action.label(*language).is_empty());
            }
        }
    }

    #[test]
    fn no_two_actions_start_out_sharing_a_key() {
        let binds = Keybinds::default();
        for (index, a) in Action::ALL.iter().enumerate() {
            for b in Action::ALL.iter().skip(index + 1) {
                assert_ne!(
                    binds.key(*a),
                    binds.key(*b),
                    "{:?} and {:?} share a default key",
                    a,
                    b
                );
            }
        }
    }

    #[test]
    fn names_survive_a_round_trip() {
        for (code, name) in KEYS {
            assert_eq!(key_from_name(name), Some(*code), "{name} did not come back");
            assert_eq!(key_name(*code), *name);
        }
    }

    #[test]
    fn binding_a_key_takes_it_off_whatever_had_it() {
        // Refusing instead would leave the player hunting for what is
        // holding the key they want.
        let mut binds = Keybinds::default();
        binds.bind(Action::Forward, KeyCode::KeyI);
        assert_eq!(binds.key(Action::Forward), Some(KeyCode::KeyI));
        assert_eq!(
            binds.key(Action::Inventory),
            None,
            "two actions ended up on one key"
        );
        assert_eq!(binds.label(Action::Inventory), NONE, "and the screen must say so");
    }

    #[test]
    fn an_unbindable_key_is_refused_rather_than_stored() {
        let mut binds = Keybinds::default();
        let before = binds.key(Action::Forward);
        binds.bind(Action::Forward, KeyCode::F24);
        assert_eq!(binds.key(Action::Forward), before, "an unnameable key was stored");
    }

    #[test]
    fn rebinding_something_to_its_own_key_is_harmless() {
        let mut binds = Keybinds::default();
        binds.bind(Action::Jump, KeyCode::Space);
        assert_eq!(binds.key(Action::Jump), Some(KeyCode::Space));
    }

    #[test]
    fn a_hand_edited_file_cannot_leave_a_dead_binding() {
        let mut binds = Keybinds::default();
        binds.bound.insert("forward".to_string(), "NOT A KEY".to_string());
        binds.bound.insert("fly".to_string(), "F".to_string());
        binds.sanitize();
        assert_eq!(
            binds.key(Action::Forward),
            Some(KeyCode::KeyW),
            "a nonsense key should fall back to the default"
        );
        assert!(!binds.bound.contains_key("fly"), "an unknown action survived");
    }

    #[test]
    fn every_action_is_bound_to_something_of_its_own() {
        // Two actions sharing a default key is a binding one of them
        // silently never gets, and the controls screen shows both of
        // them holding it.
        let mut seen = std::collections::HashSet::new();
        for action in Action::ALL {
            assert!(
                seen.insert(action.default_key()),
                "{} shares its default key",
                action.label(Language::English)
            );
            assert!(
                !action.key().is_empty(),
                "{} has no settings name",
                action.label(Language::English)
            );
        }
    }

    #[test]
    fn another_binding_costs_a_scroll_and_never_a_shorter_row() {
        // **This test used to say the opposite.** It checked that every
        // binding still fitted one fixed panel, because the screen drew
        // all of them at once and divided the panel between them -- so
        // the assertion that "passed" was the rows being squeezed, and
        // the day it failed the answer would have been to squeeze them
        // further. The list scrolls now (`menu::ListRows`), so what has
        // to hold is the reverse: the row height owes nothing to how
        // many bindings there are, and a nineteenth binding is one more
        // row below the fold rather than a thinner row for everybody.
        const PANEL_HEIGHT: f32 = 0.76 - -0.62;
        let row = crate::ui::menu::controls_row_height();
        assert!(row >= 0.055, "the rows are too short to read");
        // A window worth scrolling: several full-size rows at once, not
        // a slot showing one.
        let visible = ((PANEL_HEIGHT - 0.06) / (row + 0.014)) as usize;
        assert!(
            visible >= 8,
            "only {visible} of the {} bindings are on screen at a time",
            Action::ALL.len()
        );
    }

    #[test]
    fn resetting_restores_every_default() {
        let mut binds = Keybinds::default();
        binds.bind(Action::Forward, KeyCode::KeyT);
        binds.bind(Action::Jump, KeyCode::KeyG);
        binds.reset();
        for action in Action::ALL {
            assert_eq!(binds.key(action), Some(action.default_key()));
        }
    }

    #[test]
    fn settings_survive_being_written_and_read_back() {
        let mut binds = Keybinds::default();
        binds.bind(Action::Drop, KeyCode::KeyG);
        let text = toml::to_string(&binds).expect("serialise");
        let back: Keybinds = toml::from_str(&text).expect("parse");
        assert_eq!(back.key(Action::Drop), Some(KeyCode::KeyG));
        assert_eq!(back.key(Action::Forward), Some(KeyCode::KeyW));
    }
}
