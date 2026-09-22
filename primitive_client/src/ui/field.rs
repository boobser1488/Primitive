//! One editable line of text, and everything a person expects one to do.
//!
//! ## Why this exists
//!
//! Every text field in this game used to be a `String` with `push` and
//! `pop` on it. That is a field you can type into and nothing else: the
//! caret is wherever the end happens to be, a typo in the middle of a
//! server address means deleting back to it, and a name that was typed
//! wrong is retyped rather than corrected. The player's report was that
//! the fields "cannot really be used", and every item under that was
//! the same missing idea -- **the text has a place in it, and the place
//! is not always the end**.
//!
//! So this owns two byte offsets into the string and nothing else:
//! where the caret is, and where a selection started. Everything below
//! is those two moving.
//!
//! ## What it deliberately does not own
//!
//! * **What may be typed.** The glyph filter and the length cap belong
//!   to the caller, because they differ per field -- a seed takes ten
//!   digits and a name takes thirty-two of anything the font can draw.
//!   [`TextField::insert`] puts a character in and asks no questions;
//!   `Menu::type_char` is what decides whether to call it.
//! * **Where it is drawn.** Two screens draw fields at different sizes
//!   and one of them is a settings row, so the size is worked out from
//!   the rectangle at drawing time. `widgets::Painter::text_field` is
//!   what does it, and `widgets::window` is the part worth knowing
//!   about: which slice of a long value is on screen follows the caret.
//!
//! ## Bytes, not characters
//!
//! The offsets are byte indices, and they are always on a character
//! boundary -- every motion in this file moves by whole `char`s, which
//! is what `str::char_indices` hands out. Counting in characters instead
//! was tried first and thrown away: every operation then ends in a
//! `chars().nth(n)` walk, and the two representations have to be
//! converted at every call into `String`, which is where an off-by-one
//! becomes a panic on a Cyrillic name rather than a wrong caret.
//!
//! ## The phone
//!
//! On Android the *input method* owns the text and the caret while a
//! field has focus -- see `platform::Window::ime_owns_text`. Nothing in
//! this file fights it. The mirror rebuilds the line character by
//! character through the caller's own filter -- `Menu::set_focused_text`
//! and `Chat::set_typed_text` -- which leaves the caret at the end,
//! exactly where GameTextInput has it after a commit; [`TextField::set_text`]
//! does the same for the two places a field is opened on an existing
//! value. A caret this side that moved on its own would be a second
//! caret disagreeing with the one the player can see in their
//! keyboard's own strip, which is why every key that moves it is shut
//! off while the input method holds the field.

/// A line of text with a caret in it.
///
/// `caret == anchor` means nothing is selected; the pair is kept
/// unordered, because which end the player is dragging decides which
/// end a shifted arrow key moves.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TextField {
    text: String,
    caret: usize,
    anchor: usize,
}

/// How far a caret motion goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Motion {
    Left,
    Right,
    /// To the start of the word to the left, the way every editor since
    /// the eighties means it: over any run of spaces first, then over
    /// the word itself.
    WordLeft,
    WordRight,
    Home,
    End,
}

impl TextField {
    pub fn new() -> Self {
        Self::default()
    }

    /// A field holding `text`, with the caret at the end of it.
    ///
    /// The end rather than the start, for every case this has: a form
    /// opened on an existing server entry, a username being corrected.
    /// The player is going to add to it or delete from it, and both of
    /// those start where the writing stops.
    pub fn with(text: impl Into<String>) -> Self {
        let text = text.into();
        let end = text.len();
        Self {
            text,
            caret: end,
            anchor: end,
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// How many characters are in it -- which is what a length cap
    /// counts, because a cap in bytes would let a Latin name be twice
    /// as long as a Cyrillic one.
    pub fn chars(&self) -> usize {
        self.text.chars().count()
    }

    pub fn caret(&self) -> usize {
        self.caret
    }

    /// The selected range, low end first, or `None` when there is none.
    pub fn selection(&self) -> Option<(usize, usize)> {
        (self.caret != self.anchor)
            .then(|| (self.caret.min(self.anchor), self.caret.max(self.anchor)))
    }

    /// Replaces everything, putting the caret at the end.
    ///
    /// **What the Android mirror calls.** See the module note: the input
    /// method owns the caret while it owns the text, and the end is
    /// where it has one after a commit.
    pub fn set_text(&mut self, text: impl Into<String>) {
        *self = Self::with(text);
    }

    pub fn clear(&mut self) {
        *self = Self::new();
    }

    /// Puts one character where the caret is, replacing any selection.
    pub fn insert(&mut self, c: char) {
        self.replace_selection();
        self.text.insert(self.caret, c);
        self.caret += c.len_utf8();
        self.anchor = self.caret;
    }

    /// The character before the caret, or the selection if there is one.
    ///
    /// **A selection is deleted rather than one character taken off the
    /// end of it**, which is the whole reason selecting is worth having:
    /// select-all then type is how a field is replaced.
    pub fn backspace(&mut self) {
        if self.replace_selection() {
            return;
        }
        let Some(previous) = self.boundary_left(self.caret) else {
            return;
        };
        self.text.replace_range(previous..self.caret, "");
        self.caret = previous;
        self.anchor = previous;
    }

    /// ...and the character after it, which is what Delete means and
    /// what no field in this game used to do at all.
    pub fn delete(&mut self) {
        if self.replace_selection() {
            return;
        }
        let Some(next) = self.boundary_right(self.caret) else {
            return;
        };
        self.text.replace_range(self.caret..next, "");
    }

    /// A whole word backwards -- control-backspace.
    pub fn backspace_word(&mut self) {
        if self.replace_selection() {
            return;
        }
        let to = self.word_left(self.caret);
        self.text.replace_range(to..self.caret, "");
        self.caret = to;
        self.anchor = to;
    }

    /// ...and forwards.
    pub fn delete_word(&mut self) {
        if self.replace_selection() {
            return;
        }
        let to = self.word_right(self.caret);
        self.text.replace_range(self.caret..to, "");
    }

    /// Moves the caret. `extend` is the shift key: the anchor stays put
    /// and the selection grows, rather than collapsing.
    ///
    /// **An unshifted arrow with a selection up collapses to the end it
    /// is moving toward** rather than moving the caret from where it
    /// happens to be. That is what every editor does and the reason is
    /// worth writing down: after selecting a word, Left means "put me
    /// before that word", and a caret that instead stepped one character
    /// back from the far end lands somewhere the player did not point
    /// at.
    pub fn move_caret(&mut self, motion: Motion, extend: bool) {
        if !extend {
            if let Some((low, high)) = self.selection() {
                match motion {
                    Motion::Left | Motion::WordLeft => {
                        self.caret = low;
                        self.anchor = low;
                        if motion == Motion::Left {
                            return;
                        }
                    }
                    Motion::Right | Motion::WordRight => {
                        self.caret = high;
                        self.anchor = high;
                        if motion == Motion::Right {
                            return;
                        }
                    }
                    Motion::Home | Motion::End => {}
                }
            }
        }
        self.caret = match motion {
            Motion::Left => self.boundary_left(self.caret).unwrap_or(0),
            Motion::Right => self.boundary_right(self.caret).unwrap_or(self.text.len()),
            Motion::WordLeft => self.word_left(self.caret),
            Motion::WordRight => self.word_right(self.caret),
            Motion::Home => 0,
            Motion::End => self.text.len(),
        };
        if !extend {
            self.anchor = self.caret;
        }
    }

    /// Puts the caret at a byte offset, dropping any selection.
    ///
    /// **What a click in the field calls**, with the offset worked out
    /// by `widgets::caret_at_x` from the same numbers the field was
    /// drawn with. Clamped to a character boundary rather than trusted:
    /// this is the one entry point whose argument comes from arithmetic
    /// on a mouse position, and a byte offset in the middle of a
    /// Cyrillic letter is not a wrong caret, it is a panic the next time
    /// anything slices the string.
    pub fn place_caret(&mut self, at: usize) {
        // Walked back rather than sliced back: slicing at an offset
        // that is not a boundary is the panic this is here to prevent,
        // so the fix cannot be written with a slice.
        let mut at = at.min(self.text.len());
        while at > 0 && !self.text.is_char_boundary(at) {
            at -= 1;
        }
        self.caret = at;
        self.anchor = at;
    }

    /// Selects the lot, which is how a field is thrown away and retyped
    /// in one gesture.
    pub fn select_all(&mut self) {
        self.anchor = 0;
        self.caret = self.text.len();
    }

    /// Cuts out whatever is selected. Answers whether there was any.
    fn replace_selection(&mut self) -> bool {
        let Some((low, high)) = self.selection() else {
            return false;
        };
        self.text.replace_range(low..high, "");
        self.caret = low;
        self.anchor = low;
        true
    }

    /// The character boundary before `at`, or `None` at the start.
    fn boundary_left(&self, at: usize) -> Option<usize> {
        self.text[..at].char_indices().next_back().map(|(i, _)| i)
    }

    /// ...and after it, or `None` at the end.
    fn boundary_right(&self, at: usize) -> Option<usize> {
        self.text[at..].chars().next().map(|c| at + c.len_utf8())
    }

    /// Where a word-left motion lands: over any spaces, then over the
    /// word behind them.
    fn word_left(&self, from: usize) -> usize {
        let mut at = from;
        while let Some(previous) = self.boundary_left(at) {
            if !self.text[previous..].starts_with(char::is_whitespace) {
                break;
            }
            at = previous;
        }
        while let Some(previous) = self.boundary_left(at) {
            if self.text[previous..].starts_with(char::is_whitespace) {
                break;
            }
            at = previous;
        }
        at
    }

    /// ...and word-right: over the word, then over the spaces after it.
    ///
    /// The other way round from `word_left`, deliberately, and this is
    /// the asymmetry every editor has: going right leaves the caret at
    /// the start of the *next* word, going left leaves it at the start
    /// of *this* one, so the two are inverses in the only sense that
    /// matters -- pressing one and then the other gets you back.
    fn word_right(&self, from: usize) -> usize {
        let mut at = from;
        while let Some(next) = self.boundary_right(at) {
            if self.text[at..].starts_with(char::is_whitespace) {
                break;
            }
            at = next;
        }
        while let Some(next) = self.boundary_right(at) {
            if !self.text[at..].starts_with(char::is_whitespace) {
                break;
            }
            at = next;
        }
        at
    }
}

impl From<String> for TextField {
    fn from(text: String) -> Self {
        Self::with(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A field with `text` typed into it one character at a time,
    /// which is the only way anything gets into one -- see the module
    /// note on why the caller owns the filter.
    fn typed(text: &str) -> TextField {
        let mut field = TextField::new();
        for c in text.chars() {
            field.insert(c);
        }
        field
    }

    /// Text goes in where the caret is, not on the end.
    ///
    /// **The whole of the report this file answers.** A typo in the
    /// middle of `10.0.0.4:7878` used to mean deleting back to it,
    /// because the only edit a field had was "add a character to the
    /// end".
    #[test]
    fn a_character_is_typed_where_the_caret_is_and_not_at_the_end() {
        let mut field = typed("helo world");
        for _ in 0..7 {
            field.move_caret(Motion::Left, false);
        }
        field.insert('l');
        assert_eq!(field.text(), "hello world");
        // ...and the caret came with it, so the next character lands
        // after this one rather than back where it started.
        field.insert('!');
        assert_eq!(field.text(), "hell!o world");
    }

    /// Backspace and Delete take the character on either side of the
    /// caret.
    ///
    /// Delete did not exist at all: the key was read by the menu and
    /// used to remove the selected *world*, which is a fine binding on a
    /// list and a lost key in a form.
    #[test]
    fn backspace_and_delete_take_the_characters_on_either_side() {
        let mut field = typed("abcd");
        field.move_caret(Motion::Left, false);
        field.move_caret(Motion::Left, false);
        field.backspace();
        assert_eq!(field.text(), "acd");
        field.delete();
        assert_eq!(field.text(), "ad");
        // Both are no-ops at the ends rather than panics.
        field.move_caret(Motion::Home, false);
        field.backspace();
        field.move_caret(Motion::End, false);
        field.delete();
        assert_eq!(field.text(), "ad");
    }

    /// Home and End go to the ends, and the caret stays on a character
    /// boundary throughout.
    #[test]
    fn home_and_end_reach_both_ends_of_a_line_in_any_alphabet() {
        let mut field = typed("привет мир");
        field.move_caret(Motion::Home, false);
        assert_eq!(field.caret(), 0);
        field.insert('!');
        assert_eq!(field.text(), "!привет мир");
        field.move_caret(Motion::End, false);
        field.insert('?');
        assert_eq!(field.text(), "!привет мир?");
    }

    /// Every motion lands on a character boundary, whatever the
    /// alphabet.
    ///
    /// **This is the test that would have caught the panic**, and it is
    /// the reason the offsets in this file are bytes rather than
    /// characters: a Cyrillic letter is two bytes and a slice through
    /// the middle of one is not a `&str`, it is a process that has
    /// stopped.
    #[test]
    fn a_caret_never_lands_in_the_middle_of_a_letter() {
        for text in ["привет мир", "zażółć gęślą jaźń", "ascii only", ""] {
            let mut field = typed(text);
            for motion in [
                Motion::Home,
                Motion::Right,
                Motion::WordRight,
                Motion::End,
                Motion::Left,
                Motion::WordLeft,
            ] {
                for _ in 0..40 {
                    field.move_caret(motion, false);
                    assert!(
                        field.text().is_char_boundary(field.caret()),
                        "{motion:?} put the caret at {} of {text:?}",
                        field.caret(),
                    );
                }
            }
            // ...and the same while deleting from both sides, which is
            // where a bad boundary would actually panic.
            let mut chewing = typed(text);
            chewing.move_caret(Motion::Home, false);
            for _ in 0..40 {
                chewing.delete();
                chewing.backspace();
                chewing.move_caret(Motion::Right, false);
            }
        }
    }

    /// The word motions step over a word at a time, and left and right
    /// are inverses of each other.
    #[test]
    fn a_word_at_a_time_gets_back_to_where_it_started() {
        let mut field = typed("one two  three");
        field.move_caret(Motion::End, false);
        field.move_caret(Motion::WordLeft, false);
        assert_eq!(&field.text()[field.caret()..], "three");
        field.move_caret(Motion::WordLeft, false);
        assert_eq!(&field.text()[field.caret()..], "two  three");
        field.move_caret(Motion::WordRight, false);
        assert_eq!(&field.text()[field.caret()..], "three");
        field.move_caret(Motion::WordLeft, false);
        assert_eq!(&field.text()[field.caret()..], "two  three");
    }

    /// Control-backspace takes the word behind the caret.
    #[test]
    fn a_whole_word_can_be_taken_back_in_one_stroke() {
        let mut field = typed("10.0.0.4 7878");
        field.backspace_word();
        assert_eq!(field.text(), "10.0.0.4 ");
        field.backspace_word();
        assert_eq!(field.text(), "");
        // ...and on an empty field it is nothing, not a panic.
        field.backspace_word();
        assert_eq!(field.text(), "");
    }

    /// Shift with an arrow selects; typing over a selection replaces it.
    ///
    /// Select-all-then-type is how a field is thrown away and rewritten,
    /// and it is the one gesture that turns "I have to delete this
    /// thirty-character address" into one keystroke.
    #[test]
    fn typing_over_a_selection_replaces_it() {
        let mut field = typed("old name");
        field.select_all();
        assert_eq!(field.selection(), Some((0, 8)));
        field.insert('n');
        assert_eq!(field.text(), "n");
        assert_eq!(field.selection(), None, "the selection outlived what it selected");

        let mut partial = typed("keep this");
        partial.move_caret(Motion::End, false);
        for _ in 0..4 {
            partial.move_caret(Motion::Left, true);
        }
        assert_eq!(partial.selection(), Some((5, 9)));
        partial.backspace();
        assert_eq!(partial.text(), "keep ");
    }

    /// An unshifted arrow with a selection up collapses to the end it is
    /// moving toward.
    ///
    /// The alternative -- stepping one character from wherever the caret
    /// happens to be -- puts it somewhere the player never pointed at,
    /// and it is the difference between "put me before that word" and
    /// "put me one letter into the middle of it".
    #[test]
    fn an_arrow_collapses_a_selection_to_the_side_it_moves_toward() {
        let mut field = typed("abcdef");
        field.move_caret(Motion::Home, false);
        for _ in 0..3 {
            field.move_caret(Motion::Right, true);
        }
        assert_eq!(field.selection(), Some((0, 3)));
        field.move_caret(Motion::Left, false);
        assert_eq!(field.caret(), 0);
        assert_eq!(field.selection(), None);

        field.select_all();
        field.move_caret(Motion::Right, false);
        assert_eq!(field.caret(), 6);
    }

    /// A click lands the caret on a character boundary, whatever the
    /// arithmetic that produced the offset said.
    #[test]
    fn a_placed_caret_never_lands_in_the_middle_of_a_letter() {
        let mut field = typed("привет");
        for at in 0..=field.text().len() + 4 {
            field.place_caret(at);
            assert!(
                field.text().is_char_boundary(field.caret()),
                "placing at {at} gave a caret at {}",
                field.caret(),
            );
            assert_eq!(field.selection(), None, "placing a caret kept a selection");
        }
        // ...and it really moves, rather than clamping to one end.
        field.place_caret(4);
        assert_eq!(&field.text()[field.caret()..], "ивет");
    }

    /// A line handed over by an input method puts the caret at the end.
    ///
    /// On a phone the keyboard owns both, and a caret this side that
    /// stayed where it was would disagree with the one the player can
    /// see in their own keyboard's strip. See the module note.
    #[test]
    fn text_from_an_input_method_leaves_the_caret_where_that_method_has_it() {
        let mut field = typed("half");
        field.move_caret(Motion::Home, false);
        field.set_text("halfway through a word");
        assert_eq!(field.caret(), field.text().len());
        assert_eq!(field.selection(), None);
    }
}
