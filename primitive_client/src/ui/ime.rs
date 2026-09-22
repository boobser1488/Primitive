//! Keeping a phone's input method and the game's text field in step.
//!
//! On a desktop there is one copy of a text field and the game owns it.
//! On Android there are two: GameTextInput's `Editable`, which the
//! keyboard writes into, and the game's own `TextField`, which is what
//! gets drawn and saved. Neither can be deleted -- see
//! `platform::Window::ime_owns_text` -- so they are reconciled once a
//! frame, and this is where that is decided.
//!
//! ## Why any of this is a module rather than nine lines in the frame
//! loop
//!
//! Because it was nine lines in the frame loop, and they were wrong in
//! a way nothing could see. A phone cannot be driven (see `CLAUDE.md`),
//! so the only check this code ever had was a person typing into the
//! *first* field of a form. Everything about the second one --
//! whether tapping it asks for a keyboard, whose text the editor is
//! holding at that instant -- was reachable only by hand, which means
//! it was never checked twice by the same method. The player's report
//! was "the seed field does not work".
//!
//! What was actually wrong, both halves of it:
//!
//! * **The keyboard was requested on "is *something* taking text",
//!   which does not change when the player moves from one field to the
//!   next.** So a tap on the seed box asked for nothing at all, and on a
//!   phone the keyboard is dismissed by a back gesture the game never
//!   hears about -- which is also the gesture a player has to make to
//!   reach the CREATE button under it. Once it was down, no field on
//!   that screen could raise it again.
//! * **The mirror was one string with no idea whose text it was.** The
//!   editor still held the *previous* field's line at the moment the
//!   focus moved, and a commit that landed on the same frame as the tap
//!   read as "the player typed this" -- into the field they had just
//!   moved to. A name of `world2024` put `2024` in the seed box and
//!   took the four characters off the name.
//!
//! Both disappear once the mirror knows *which* field it is a mirror
//! of, which is what [`Mirror`] is. And once that is a value rather
//! than three locals in a match arm, the property is testable without a
//! phone: see the tests at the bottom, which walk the real hit-testing
//! of every form in the game.

use crate::ui::{chat::Chat, menu::Menu};

/// Which text field the on-screen keyboard is for.
///
/// **The identity is the field, not the screen it is on.** Two screens
/// that both focus `Field::Name` are, to a keyboard, the same field: it
/// is up, it is pointed at a name, and nothing has to be renegotiated.
/// What can still differ is the *content* -- a form that reset the name
/// under it -- and that is caught a frame later by the push-back in
/// [`Mirror::sync`], which is what the push-back is for. Putting the
/// screen in here as well would buy one frame and cost an enum that has
/// to be kept in step with `Menu::accepts_text` by hand, which is the
/// same class of bug as the one this module is fixing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Target {
    /// A field of one of the menu's forms: a world's name or seed, a
    /// server's name or address, the player's own name.
    Menu(crate::ui::menu::Field),
    /// The chat box.
    Chat,
}

/// The game's side of the conversation, as [`Mirror`] needs to see it.
///
/// A trait rather than the `(&mut Menu, &mut Chat)` pair the frame loop
/// actually has, so that the reconciliation can be exercised against a
/// field that is three lines long instead of against the whole menu.
/// The pair implements it just below.
pub trait Fields {
    /// Which field has the keyboard, or `None` when nothing does.
    fn focused(&self) -> Option<Target>;

    /// What that field holds. Empty when nothing has focus, rather than
    /// an `Option`: the question is "what should the editor be
    /// holding", and for a screen with no field the answer is
    /// "nothing", not "I do not know".
    fn text(&self) -> String;

    /// Replaces it with what the outside editor says it holds.
    ///
    /// May reject part of what it is given -- the glyph filter, the
    /// digits-only rule of a seed, a length cap -- which is exactly why
    /// [`Mirror::sync`] reads the field back afterwards instead of
    /// assuming it took.
    fn set_text(&mut self, text: &str);
}

/// What a frame's reconciliation asks of the platform.
///
/// Returned rather than performed, so that the decision can be tested
/// without a window: everything in here is applied by the frame loop
/// through `platform::Window`.
#[derive(Default, PartialEq, Eq, Debug)]
pub struct Request {
    /// Raise or dismiss the on-screen keyboard. `None` means "leave it
    /// where it is", which is the answer on almost every frame.
    pub keyboard: Option<bool>,
    /// Put this in the platform's own editor.
    pub editor: Option<String>,
    /// What a trace should say happened, and empty unless one was
    /// asked for.
    ///
    /// Carried out rather than printed here because this module has no
    /// business writing to stdout, and because a test can then assert
    /// on the one diagnostic that exists on the far side of an input
    /// method. See `PRIMITIVE_IME_TRACE` in `lib.rs`.
    pub notes: Vec<String>,
}

/// What the game and the platform's editor last agreed a field said.
#[derive(Default)]
pub struct Mirror {
    /// Whose text `agreed` is. `None` when no field has the keyboard.
    target: Option<Target>,
    /// The third string, and the reason "who changed it" is answerable
    /// at all: the editor differing from this means the player typed,
    /// and the game differing from it means the game edited the field
    /// itself -- a rejected character, a length cap, a form that reset.
    /// With only two strings the pair would ever only be "different",
    /// and there would be no way to say which way the text should
    /// travel.
    agreed: String,
}

impl Mirror {
    /// Forgets what the platform is holding, so the next frame
    /// renegotiates from scratch.
    ///
    /// **For regaining window focus, and it is not a tidy-up.** Android
    /// refuses to raise a keyboard for a window that does not have
    /// focus, and this game opens straight onto whichever screen it was
    /// last on, text field and all -- so the request went out on the
    /// first frame, was dropped without a word, and nothing asked again
    /// because as far as the edge was concerned the keyboard was
    /// already up.
    pub fn forget(&mut self) {
        self.target = None;
        self.agreed.clear();
    }

    /// One frame's reconciliation.
    ///
    /// `editor` is the platform's own copy of the field, or `None`
    /// where the platform keeps none -- every desktop, where the game
    /// is the only copy there is and there is nothing to reconcile.
    pub fn sync(&mut self, fields: &mut dyn Fields, editor: Option<&str>, trace: bool) -> Request {
        let now = fields.focused();
        if now != self.target {
            // **A different field, so the editor's contents are somebody
            // else's.** Read in first and ask questions later -- which
            // is what the reconciliation below does -- and the line the
            // player typed into the name would land in the seed box
            // they have just tapped. So the editor is overwritten,
            // unread, with what the new field holds.
            self.target = now;
            self.agreed = fields.text();
            let mut notes = Vec::new();
            if trace {
                notes.push(format!(
                    "[ime] field {:?} now, editor seeded with {:?}",
                    now, self.agreed
                ));
            }
            return Request {
                // Asked for on every move between fields, not only when
                // the answer changes from "no field" to "a field". A
                // keyboard the player dismissed -- with the back
                // gesture, which is how it is dismissed on a phone and
                // which the game never hears -- is a keyboard nothing
                // would ever raise again.
                keyboard: Some(now.is_some()),
                editor: now.is_some().then(|| self.agreed.clone()),
                notes,
            };
        }
        // Nothing has the keyboard, or the platform keeps no copy of
        // the field. Either way there is nothing to reconcile.
        let (Some(_), Some(editor)) = (now, editor) else {
            return Request::default();
        };
        let mut request = Request::default();
        if editor != self.agreed {
            if trace {
                request
                    .notes
                    .push(format!("[ime] editor {editor:?} (was {:?})", self.agreed));
            }
            fields.set_text(editor);
            self.agreed = editor.to_owned();
        }
        // ...and back, when the game would not have it. The game is the
        // authority on what a field may contain -- see
        // `Menu::set_focused_text` -- so a character it refused has to
        // come out of the editor too, or the next thing typed is read
        // against text the player cannot see.
        let ours = fields.text();
        if ours != self.agreed {
            if trace {
                request
                    .notes
                    .push(format!("[ime] wrote {ours:?} (editor had {:?})", self.agreed));
            }
            self.agreed.clone_from(&ours);
            request.editor = Some(ours);
        }
        request
    }
}

/// What `PRIMITIVE_IME_TYPE` asked to be typed, and into which box.
///
/// **The hook exists because a phone cannot be driven.** MIUI refuses
/// the shell `INJECT_EVENTS`, so there is no tap and no keystroke a
/// script can deliver, and everything that has to be checked on a
/// device has to be reachable from the environment or it cannot be
/// checked twice by the same method. See `CLAUDE.md`.
///
/// Two spellings:
///
/// * `PRIMITIVE_IME_TYPE=<text>` -- the original: `<text>` is committed
///   into the new world's name, exactly as an input method commits, and
///   the world is created. What that proves is that Cyrillic survives
///   the round trip.
/// * `PRIMITIVE_IME_TYPE=seed:<digits>` -- the same for the *second*
///   box on the form, which is the one the player reported. It types a
///   name first, then commits its last character on the same frame as
///   the tap that moves to the seed box, because that is the frame the
///   bug lived on: the editor's copy is polled rather than delivered,
///   so a character committed between two frames lands together with
///   the tap.
///
/// The name it uses for that run ends in digits **on purpose**. With
/// the bug, `world2024` in the name box put `2024` in the seed box and
/// left `world202` behind; both halves are then readable off the
/// created world's `world.toml` with `adb ... run-as`, which is the
/// only way to read anything off this phone.
pub struct Probe {
    pub name: String,
    /// What to type into the seed box, or `None` for a name-only run.
    pub seed: Option<String>,
}

/// The name the seed run types, chosen so that the failure is visible
/// in the artefact rather than only in a log.
const PROBE_NAME: &str = "world2024";

impl Probe {
    pub fn parse(value: &str) -> Self {
        match value.strip_prefix("seed:") {
            Some(seed) => Self {
                name: PROBE_NAME.to_owned(),
                seed: Some(seed.to_owned()),
            },
            None => Self {
                name: value.to_owned(),
                seed: None,
            },
        }
    }
}

/// The menu and the chat box, seen as "the one thing being typed into".
///
/// The game has two things a player types into and they are unrelated
/// -- a menu form and the chat box -- but a phone has one input method,
/// holding one field. This is the join.
///
/// The chat box wins where both are somehow open, which cannot happen
/// today (the box belongs to a running world and the forms to the menu)
/// and is written down anyway, because a rule that is not written is a
/// rule the next screen gets wrong.
pub struct Typing<'a> {
    pub menu: &'a mut Menu,
    pub chat: &'a mut Chat,
}

impl Fields for Typing<'_> {
    fn focused(&self) -> Option<Target> {
        if self.chat.is_typing() {
            Some(Target::Chat)
        } else if self.menu.accepts_text() {
            Some(Target::Menu(self.menu.focus))
        } else {
            None
        }
    }

    fn text(&self) -> String {
        if self.chat.is_typing() {
            self.chat.typed_text().to_owned()
        } else {
            self.menu.focused_text().to_owned()
        }
    }

    fn set_text(&mut self, text: &str) {
        if self.chat.is_typing() {
            self.chat.set_typed_text(text);
        } else {
            self.menu.set_focused_text(text);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logic::worlds::Worlds;
    use crate::ui::menu::{Action, Backdrop, Field, Menu, MenuContext, Screen, ServerList};
    use crate::ui::widgets;

    /// A field that is a `String`, so the reconciliation can be tested
    /// without a menu behind it.
    #[derive(Default)]
    struct Fake {
        target: Option<Target>,
        text: String,
        /// What the field refuses, one character at a time -- the
        /// stand-in for the seed's digits-only rule.
        digits_only: bool,
    }

    impl Fields for Fake {
        fn focused(&self) -> Option<Target> {
            self.target
        }
        fn text(&self) -> String {
            self.text.clone()
        }
        fn set_text(&mut self, text: &str) {
            self.text = if self.digits_only {
                text.chars().filter(char::is_ascii_digit).collect()
            } else {
                text.to_owned()
            };
        }
    }

    #[test]
    fn moving_to_a_second_field_asks_for_the_keyboard_all_over_again() {
        // The player's report, in one line: the world name takes text
        // and the seed does not. The keyboard used to be requested on
        // "is anything taking text", which is true for the whole
        // create-world screen and therefore does not change when the
        // focus moves from the name to the seed -- so tapping the seed
        // box asked the platform for nothing. On a phone the keyboard
        // is dismissed with the back gesture, which the game never
        // hears about, and after that no field on the screen could
        // raise it again.
        let mut mirror = Mirror::default();
        let mut fields = Fake {
            target: Some(Target::Menu(Field::Name)),
            text: "world".into(),
            ..Fake::default()
        };
        assert_eq!(
            mirror.sync(&mut fields, Some(""), false).keyboard,
            Some(true),
            "the first field did not ask for a keyboard either",
        );
        // Steady state: nothing is asked for on a frame where nothing
        // moved, because asking sixty times a second is not asking.
        assert_eq!(mirror.sync(&mut fields, Some("world"), false).keyboard, None);

        fields.target = Some(Target::Menu(Field::Seed));
        fields.text = String::new();
        fields.digits_only = true;
        let request = mirror.sync(&mut fields, Some("world"), false);
        assert_eq!(
            request.keyboard,
            Some(true),
            "tapping the seed box asked for no keyboard",
        );
    }

    #[test]
    fn text_committed_for_one_field_never_lands_in_the_one_just_tapped() {
        // The other half, and the one that looks like the seed field
        // corrupting itself. The input method's copy is *polled*, not
        // delivered as an event, so a character committed between two
        // frames arrives on the same frame as the tap that moved the
        // focus. The mirror had no idea whose text it was holding, so
        // "the editor differs from what we agreed" read as "the player
        // typed this" -- into the field they had just moved to.
        let mut mirror = Mirror::default();
        let mut fields = Fake {
            target: Some(Target::Menu(Field::Name)),
            text: "world202".into(),
            ..Fake::default()
        };
        mirror.sync(&mut fields, Some("world202"), false);

        // The last character of the name is committed and the seed box
        // is tapped, both before this frame's sync.
        fields.target = Some(Target::Menu(Field::Seed));
        fields.text = String::new();
        fields.digits_only = true;
        let request = mirror.sync(&mut fields, Some("world2024"), false);

        assert_eq!(fields.text, "", "the name's text was typed into the seed");
        assert_eq!(
            request.editor.as_deref(),
            Some(""),
            "the editor was left holding the name while the seed had focus",
        );
    }

    #[test]
    fn a_character_the_field_refuses_is_taken_back_out_of_the_editor() {
        // The seed takes digits only, and the editor has to be told, or
        // the next keystroke is read against text the player cannot
        // see. This is the pre-existing behaviour and the test is here
        // so that keying the mirror to a field did not lose it.
        let mut mirror = Mirror::default();
        let mut fields = Fake {
            target: Some(Target::Menu(Field::Seed)),
            digits_only: true,
            ..Fake::default()
        };
        mirror.sync(&mut fields, Some(""), false);
        let request = mirror.sync(&mut fields, Some("12a34"), false);
        assert_eq!(fields.text, "1234");
        assert_eq!(request.editor.as_deref(), Some("1234"));
    }

    #[test]
    fn leaving_every_field_puts_the_keyboard_away() {
        let mut mirror = Mirror::default();
        let mut fields = Fake {
            target: Some(Target::Menu(Field::Name)),
            ..Fake::default()
        };
        mirror.sync(&mut fields, Some(""), false);
        fields.target = None;
        let request = mirror.sync(&mut fields, Some("world"), false);
        assert_eq!(request.keyboard, Some(false));
        assert_eq!(request.editor, None, "a dismissed keyboard was still seeded");
        // ...and a screen with no field never reads the editor again,
        // whatever is left in it.
        assert_eq!(fields.text, "");
    }

    #[test]
    fn regaining_window_focus_asks_for_the_keyboard_again() {
        // Android refuses to raise a keyboard for a window that has no
        // focus, and says nothing when it refuses. The game comes up on
        // whichever screen it was last on, so the one request can go out
        // before the activity is focused and be dropped in silence.
        let mut mirror = Mirror::default();
        let mut fields = Fake {
            target: Some(Target::Menu(Field::Name)),
            ..Fake::default()
        };
        mirror.sync(&mut fields, Some(""), false);
        assert_eq!(mirror.sync(&mut fields, Some(""), false).keyboard, None);
        mirror.forget();
        assert_eq!(mirror.sync(&mut fields, Some(""), false).keyboard, Some(true));
    }

    #[test]
    fn the_trace_says_both_halves_of_what_happened() {
        // The trace is the only diagnostic there is on the far side of
        // an input method -- there is no debugger over there -- so what
        // it says is worth a test of its own. See `PRIMITIVE_IME_TRACE`.
        let mut mirror = Mirror::default();
        let mut fields = Fake {
            target: Some(Target::Menu(Field::Seed)),
            digits_only: true,
            ..Fake::default()
        };
        assert!(mirror.sync(&mut fields, Some(""), true).notes[0].contains("field"));
        let notes = mirror.sync(&mut fields, Some("1a"), true).notes;
        assert!(notes[0].starts_with("[ime] editor"), "{notes:?}");
        assert!(notes[1].starts_with("[ime] wrote"), "{notes:?}");
        assert!(mirror.sync(&mut fields, Some("1"), true).notes.is_empty());
    }

    #[test]
    fn the_probe_names_the_box_it_is_typing_into() {
        // The bare form is what it always was, so that a run written
        // down in `CLAUDE.md` still means the same thing. Cyrillic,
        // because that is what the hook was built to prove.
        let plain = Probe::parse("\u{43c}\u{438}\u{440}");
        assert_eq!(plain.name, "\u{43c}\u{438}\u{440}");
        assert_eq!(plain.seed, None);

        let seeded = Probe::parse("seed:1234");
        assert_eq!(seeded.seed.as_deref(), Some("1234"));
        assert!(
            seeded.name.ends_with(|c: char| c.is_ascii_digit()),
            "the seed run needs a name whose last character could be mistaken for a seed",
        );
    }

    #[test]
    fn every_field_of_every_form_raises_the_keyboard_when_it_is_tapped() {
        // **The property the seed field broke, checked where a finger
        // actually lands.** Not "the sync handles a focus change" --
        // that is the test above -- but "each box a form draws, hit at
        // the point it is drawn at, ends with the platform being asked
        // for a keyboard and its editor holding that box's text".
        //
        // Driven through `field_boxes`, so a field added to a form is
        // covered the day it is drawn, and a field that stops being
        // reachable fails the count at the end rather than passing
        // quietly.
        let settings = crate::settings::ClientSettings::default();
        // A path that cannot exist, so nothing is read from disk and
        // nothing can be written to it.
        let worlds = Worlds::load(
            std::env::temp_dir().join("primitive-ime-tests-no-such-folder"),
        );
        let layout = widgets::Layout::for_screen(2712.0 / 1220.0, 1.5);
        let mut tapped = 0;
        for screen in [Screen::CreatingWorld, Screen::Editing(None), Screen::Settings] {
            let mut menu = Menu::new(ServerList::default());
            let mut chat = Chat::new();
            menu.screen = screen.clone();
            if matches!(screen, Screen::Settings) {
                menu.begin_username_edit("player".into());
            }
            let ctx = MenuContext {
                version: "test",
                font: crate::engine::texture::FontAtlas::for_test(),
                settings: &settings,
                worlds: &worlds,
                background: Backdrop::Bare,
                layout,
            };
            let mut out = Vec::new();
            menu.build_into(&ctx, &mut out);
            let boxes = menu.field_boxes().to_vec();
            assert!(!boxes.is_empty(), "{screen:?} draws no field to tap");
            let mut mirror = Mirror::default();
            // The keyboard the screen came up with, so that what is
            // measured below is the *tap* and not the screen opening.
            mirror.sync(&mut Typing { menu: &mut menu, chat: &mut chat }, Some(""), false);
            for (rect, field) in boxes {
                // Something in the box, so that a keyboard raised with
                // an empty editor is distinguishable from one raised
                // with the field's own text.
                let filled = if field == Field::Seed { "4242" } else { "abc" };
                match field {
                    Field::Name => menu.name_input.set_text(filled),
                    Field::Address => menu.address_input.set_text(filled),
                    Field::Seed => menu.seed_input.set_text(filled),
                }
                menu.set_cursor(Some((rect.centre_x(), rect.centre_y())));
                let mut out = Vec::new();
                menu.build_into(&ctx, &mut out);
                assert_eq!(menu.click(), Some(Action::Focus(field)), "{screen:?}");
                // What the frame loop does with that action, and the
                // reason it does anything at all: a tap on the box that
                // *already* has the focus is still a player asking for
                // a keyboard, because the way one is dismissed on a
                // phone is a back gesture the game never hears about.
                // See the `Action::Focus` arm of `handle_action!`.
                mirror.forget();

                let request = mirror.sync(
                    &mut Typing { menu: &mut menu, chat: &mut chat },
                    Some("left over from the field before"),
                    false,
                );
                assert_eq!(
                    request.keyboard,
                    Some(true),
                    "{screen:?}: tapping {field:?} asked for no keyboard",
                );
                assert_eq!(
                    request.editor.as_deref(),
                    Some(filled),
                    "{screen:?}: the editor was not given {field:?}'s own text",
                );
                tapped += 1;
            }
        }
        // Two on the world form, two on the server form, one on the
        // settings row. A field that stops being reachable by finger is
        // a field a phone cannot fill in.
        assert_eq!(tapped, 5, "the forms offer a different set of fields now");
    }
}
