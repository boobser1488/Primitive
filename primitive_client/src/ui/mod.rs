//! The interface: everything the player reads, clicks or presses.
//!
//! Two halves that face each other:
//!
//! * **Output** -- `widgets` (panels, buttons, text), and the screens
//!   built out of them: `menu`, `hud`, `hotbar`, `chat`, `death`,
//!   `debug`, `inventory_screen` and `chest_screen`. All of them emit
//!   `HotbarVertex` into one list, so the whole interface is a single
//!   draw call through a single pipeline. There is no second shader and
//!   no second buffer to keep in step.
//! * **Input** -- `input` (what is held down and where the mouse went)
//!   and `keybinds` (which key means which action). Reading the keyboard
//!   is an interface concern, not a physics one: what the movement code
//!   is handed is a direction, never a key code.
//!
//! Nothing in here decides anything. A menu returns what the player
//! chose, a screen returns geometry, and the frame loop in `main` is
//! what acts on either -- so a screen cannot quietly change the world,
//! and the rules stay in [`crate::logic`] where the server can be the
//! judge of them.

pub mod chat;
pub mod chest_screen;
pub mod death;
pub mod debug;
pub mod hotbar;
pub mod hud;
pub mod input;
pub mod inventory_screen;
pub mod lang;
pub mod keybinds;
pub mod menu;
pub mod widgets;
