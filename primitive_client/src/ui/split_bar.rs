//! The row of buttons a stack in a finger's hand comes with.
//!
//! ## What it is for
//!
//! **"На телефоне невозможно разделить слот на части."** The pack and every
//! container split a stack with the right mouse button -- half of what is
//! held into the square pressed -- and a finger has no right button. It
//! had a stand-in: a finger *rested* on the glass for half a second is the
//! right button (`platform::touch::Chord`). Nobody found it, for the reason
//! `InventoryScreen::set_touch` already gives about long presses: nobody
//! rests a finger on a thing that looks like a button to see what else it
//! does. And even found, it could only ever halve.
//!
//! So on glass, a stack picked up comes with this: a plate beside the
//! square it came from, saying what it is, with a row of finger-sized
//! buttons that decide how much of it the next square pressed receives.
//!
//! ```text
//!   Бронзовый топор x12
//!   [ 1 ][1/2][ - ][ 7 ][ + ][ ВСЁ ][ ^ ]
//! ```
//!
//! * `1`, `1/2`, `ВСЁ` -- the three amounts anybody wants most, one tap
//!   each. The one in force is drawn pressed in.
//! * `-` `7` `+` -- any other number, a step at a time, with the number in
//!   force between them. It starts from whatever the three above last set,
//!   so "five of twelve" is `1/2` and one `-`, not five `+`.
//! * `^` / `v` -- the shift-click: the whole stack straight across, to the
//!   other half of the pack or the other side of a chest, whichever way the
//!   arrow points. Two taps from a full square to a chest -- the one move a
//!   player makes most at a chest, and on glass it had been a two-finger
//!   chord nobody knew about.
//! * `ВЫЛИТЬ` -- only on a jug with something in it: the right click with
//!   empty hands, which is how a jug is tipped out.
//!
//! The amount stays chosen while the stack is in hand, so dealing seeds
//! out one to a square is `1` and then a tap per square -- the right
//! button's "keep holding the rest", which is what makes splitting worth
//! having -- and it goes back to `ВСЁ` with the next thing picked up, so a
//! player who never touches the bar moves stacks exactly as before.
//!
//! ## What was turned down
//!
//! * **A long press, alone.** It is what there was, and it is how the
//!   player came to say a split was impossible. Kept, not relied on: a
//!   rested finger still halves, for anybody who knows.
//! * **A slider.** Precise to a unit only when it is long, and it is dragged
//!   -- and a drag on these screens is the scroll (`touch::Gesture::Scrolled`)
//!   that every list already answers to. A slider inside a scrolling screen
//!   is two gestures fighting over one thumb.
//! * **Typing the number.** The phone's keyboard comes up over half the
//!   glass, over the squares the stack is about to go into.
//! * **Drag the stack and let go.** Every gesture here is tap-then-tap
//!   (see the top of `inventory_screen`) for the reasons written there; a
//!   drag that meant "split" on glass and nothing with a mouse would be a
//!   second grammar for one screen.
//!
//! ## Only on glass
//!
//! A mouse has the right button, the shift key and a pointer that hovers,
//! which between them already say everything this bar says. The screens
//! draw it only after `set_touch(true)`, so a desktop is exactly what it
//! was.
//!
//! ## Where it goes
//!
//! Beside the square the stack came from, never over it: above it where
//! there is room inside the panel, below it where there is not. Across, it
//! is centred on the square and then slid back inside the panel, so a
//! square at the edge of the glass gets a bar that ends at the panel's
//! frame rather than off the screen. It is drawn over the squares next
//! door; a stack in hand that wants to go under the bar goes there with the
//! bar gone, which is one tap away (the source square puts it back).

use crate::ui::lang::{Language, Msg};
use crate::ui::widgets::{self, Painter, Press, Rect};
use primitive_shared::inventory::Portion;

/// One button of the bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    /// Exactly one.
    One,
    /// Half, rounded up.
    Half,
    /// One fewer than the number in force.
    Less,
    /// One more.
    More,
    /// The whole stack.
    All,
    /// The whole stack straight across: the shift-click. `true` points up
    /// the screen.
    Across { up: bool },
    /// Tip a jug out: the right click with empty hands.
    Empty,
}

impl Choice {
    /// The portion this button sets, given the one in force and the stack.
    ///
    /// `None` for the buttons that do something instead of choosing.
    pub fn portion(self, now: Portion, count: u32) -> Option<Portion> {
        let held = now.of(count);
        Some(match self {
            Choice::One => Portion::Exactly(1),
            Choice::Half => Portion::Half,
            Choice::All => Portion::Whole,
            // Never under one: a split of nothing is not a gesture.
            Choice::Less => Portion::Exactly(held.saturating_sub(1).max(1)),
            // Up to the whole stack and no further; the whole stack is
            // `All`, which is what it reads as.
            Choice::More if held + 1 >= count => Portion::Whole,
            Choice::More => Portion::Exactly(held + 1),
            Choice::Across { .. } | Choice::Empty => return None,
        })
    }
}

/// How tall one button is: a finger, which is the whole point.
pub const BUTTON_TALL: f32 = widgets::FINGER_SIDE;
/// How wide: a finger and a little over, because `ВЫЛИТЬ` and `CAŁOŚĆ` are
/// six letters and a finger-square button letters six at under the size a
/// button may be lettered at (`widgets::button_label_scale`'s floor). The
/// width was the thing to give -- the bar is a row across a wide page --
/// and a word was not: `ВЫЛЬ` is not Russian.
pub const BUTTON_WIDE: f32 = 0.170;
/// Air between two buttons.
const GAP: f32 = 0.008;
/// Air inside the plate, round the buttons and the caption.
const PAD: f32 = 0.016;
/// Air between the plate and the square it is about.
const AIR: f32 = 0.012;
/// What the caption is written at: the tooltip's size, because it is the
/// tooltip -- a finger has no hover, and this is where it reads what it
/// is holding.
const CAPTION_SCALE: f32 = widgets::size::NOTE;

/// How tall the caption band is: a line of the tooltip's text and a
/// little air under it.
fn caption_band() -> f32 {
    widgets::cell_height(CAPTION_SCALE) + 0.010
}

/// The bar, laid out: the plate, where the caption is written, and every
/// button's rectangle.
///
/// **One value both halves read.** The screen draws from it and hit-tests
/// from it, so a button cannot be drawn in one place and answer in
/// another -- the interface's one rule, held by construction here and by
/// `every_button_answers_where_it_is_drawn` below.
#[derive(Debug, Clone, PartialEq)]
pub struct Bar {
    pub plate: Rect,
    /// The pen for the caption's first letter: left edge and top.
    caption: (f32, f32),
    /// Room the caption may take across.
    caption_width: f32,
    pub buttons: Vec<(Choice, Rect)>,
    /// Where the number in force is shown, between `-` and `+`; `None` on a
    /// bar that chooses no amount (a recipe's, see [`Bar::lay_out_row`]).
    pub readout: Option<Rect>,
}

/// Where a plate of this size goes beside `anchor`, inside `bounds`.
///
/// Across: centred on the square, then slid back inside. Down: above the
/// square if it fits, below if not, and whichever side has more room if
/// neither does -- clamped, which is the one case the plate may reach the
/// square, and the tests say no square of any screen is in it.
fn place(anchor: Rect, bounds: Rect, width: f32, height: f32) -> Rect {
    let x0 = (anchor.centre_x() - width / 2.0)
        .min(bounds.x1 - width)
        .max(bounds.x0);
    let above = anchor.y1 + AIR;
    let below = anchor.y0 - AIR - height;
    let y0 = if above + height <= bounds.y1 {
        above
    } else if below >= bounds.y0 {
        below
    } else if bounds.y1 - anchor.y1 > anchor.y0 - bounds.y0 {
        (bounds.y1 - height).max(bounds.y0)
    } else {
        bounds.y0
    };
    Rect::new(x0, y0, x0 + width, y0 + height)
}

impl Bar {
    /// A bar of just these buttons in one row, with no amount: what a
    /// recipe a finger has read comes with -- `1` to make one and `ВСЁ` to
    /// make as many as the pack allows, the right click a finger lacks.
    pub fn lay_out_row(anchor: Rect, bounds: Rect, choices: &[Choice]) -> Bar {
        let n = choices.len().max(1);
        let width = n as f32 * BUTTON_WIDE + (n - 1) as f32 * GAP + PAD * 2.0;
        let height = PAD * 2.0 + caption_band() + BUTTON_TALL;
        let plate = place(anchor, bounds, width, height);
        let top = plate.y1 - PAD - caption_band();
        let buttons = choices
            .iter()
            .enumerate()
            .map(|(column, choice)| {
                let x = plate.x0 + PAD + column as f32 * (BUTTON_WIDE + GAP);
                (*choice, Rect::new(x, top - BUTTON_TALL, x + BUTTON_WIDE, top))
            })
            .collect();
        Bar {
            plate,
            caption: (plate.x0 + PAD, plate.y1 - PAD),
            caption_width: width - PAD * 2.0,
            buttons,
            readout: None,
        }
    }

    /// Lays the bar out beside `anchor`, inside `bounds`.
    ///
    /// `extra` are the buttons this stack earns beyond the amounts -- the
    /// arrow across, the jug's tipping-out -- in the order they are drawn.
    /// One row where the bounds are wide enough, as on the pack; two where
    /// they are not, as in a chest, which is a single column of squares a
    /// finger-width row of seven does not fit across: the three amounts
    /// and the extras over the number and its two steps.
    pub fn lay_out(anchor: Rect, bounds: Rect, extra: &[Choice]) -> Bar {
        let steps = [Choice::Less, Choice::More];
        let firsts: Vec<Choice> = [Choice::One, Choice::Half, Choice::All]
            .into_iter()
            .chain(extra.iter().copied())
            .collect();
        let span = |n: usize| n as f32 * BUTTON_WIDE + (n.saturating_sub(1)) as f32 * GAP;
        // One row: the three amounts, the steps round the number, then the
        // extras -- `1 1/2 - n + ALL extras`, the order a reader wants.
        let one_row = span(firsts.len() + steps.len() + 1) + PAD * 2.0;
        let rows = if one_row <= bounds.width() { 1 } else { 2 };
        let width = if rows == 1 {
            one_row
        } else {
            span(firsts.len().max(3)) + PAD * 2.0
        };
        let height = PAD * 2.0 + caption_band() + rows as f32 * BUTTON_TALL + (rows as f32 - 1.0) * GAP;
        let plate = place(anchor, bounds, width, height);
        let inner_top = plate.y1 - PAD - caption_band();
        let caption = (plate.x0 + PAD, plate.y1 - PAD);

        let cell = |column: usize, row: usize, left: f32| {
            let x = left + column as f32 * (BUTTON_WIDE + GAP);
            let top = inner_top - row as f32 * (BUTTON_TALL + GAP);
            Rect::new(x, top - BUTTON_TALL, x + BUTTON_WIDE, top)
        };
        let mut buttons = Vec::new();
        let readout;
        if rows == 1 {
            let left = plate.x0 + PAD;
            // 1, 1/2, -, n, +, ALL, extras.
            buttons.push((Choice::One, cell(0, 0, left)));
            buttons.push((Choice::Half, cell(1, 0, left)));
            buttons.push((Choice::Less, cell(2, 0, left)));
            readout = Some(cell(3, 0, left));
            buttons.push((Choice::More, cell(4, 0, left)));
            buttons.push((Choice::All, cell(5, 0, left)));
            for (n, choice) in extra.iter().enumerate() {
                buttons.push((*choice, cell(6 + n, 0, left)));
            }
        } else {
            let left = plate.x0 + PAD;
            for (n, choice) in firsts.iter().enumerate() {
                buttons.push((*choice, cell(n, 0, left)));
            }
            // The number and its steps, centred under the row above.
            let second = plate.centre_x() - span(3) / 2.0;
            buttons.push((Choice::Less, cell(0, 1, second)));
            readout = Some(cell(1, 1, second));
            buttons.push((Choice::More, cell(2, 1, second)));
        }
        Bar {
            plate,
            caption,
            caption_width: width - PAD * 2.0,
            buttons,
            readout,
        }
    }

    /// Which button a point is on.
    pub fn at(&self, cursor: (f32, f32)) -> Option<Choice> {
        self.buttons
            .iter()
            .find(|(_, rect)| rect.contains(cursor.0, cursor.1))
            .map(|(choice, _)| *choice)
    }

    /// Whether a point is anywhere on the plate -- a tap between two
    /// buttons is a tap on the bar, not on the square under it.
    pub fn covers(&self, cursor: (f32, f32)) -> bool {
        self.plate.contains(cursor.0, cursor.1)
    }

    /// Draws it: the plate, what is held, the buttons, the number.
    pub fn draw(
        &self,
        p: &mut Painter,
        caption: &str,
        // The amount in force and the stack it is of; `None` on a bar that
        // chooses no amount.
        amount: Option<(Portion, u32)>,
        cursor: Option<(f32, f32)>,
        language: Language,
    ) {
        p.deep_panel(self.plate);
        let caption = widgets::fit(caption, CAPTION_SCALE, self.caption_width);
        p.text(&caption, self.caption.0, self.caption.1, CAPTION_SCALE, widgets::INK);
        for (choice, rect) in &self.buttons {
            let label = match choice {
                Choice::One => "1",
                Choice::Half => "1/2",
                Choice::Less => "-",
                Choice::More => "+",
                Choice::All => language.text(Msg::SplitAll),
                Choice::Across { up: true } => "^",
                Choice::Across { up: false } => "v",
                Choice::Empty => language.text(Msg::PourOut),
            };
            // The amount in force is the button drawn pressed in: a latch,
            // which is what a choice that stays chosen looks like.
            let latched = amount.is_some_and(|(portion, _)| {
                matches!(
                    (choice, portion),
                    (Choice::One, Portion::Exactly(1))
                        | (Choice::Half, Portion::Half)
                        | (Choice::All, Portion::Whole)
                )
            });
            let press = if latched {
                Press::Held
            } else if cursor.is_some_and(|(x, y)| rect.contains(x, y)) {
                Press::Hovered
            } else {
                Press::Idle
            };
            p.pressable(*rect, label, press, true);
        }
        // The number, in a well rather than on a button: it is read, not
        // pressed.
        if let (Some(readout), Some((portion, count))) = (self.readout, amount) {
            p.well(readout, widgets::WELL);
            let shown = portion.of(count).to_string();
            let scale = widgets::fitted_scale(&shown, widgets::size::BODY, readout.width() - 0.03, 0.6);
            p.label_in(readout, &shown, scale, widgets::ACCENT);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a_panel() -> Rect {
        Rect::new(-1.0, -0.5, 1.0, 0.5)
    }

    #[test]
    fn every_button_is_a_finger_both_ways() {
        for bounds in [a_panel(), Rect::new(-0.5, -0.9, 0.5, 0.9)] {
            let bar = Bar::lay_out(Rect::new(-0.05, -0.05, 0.05, 0.05), bounds, &[Choice::Across { up: true }, Choice::Empty]);
            let readout = bar.readout.expect("a stack's bar shows its number");
            for (choice, rect) in bar.buttons.iter().chain([(Choice::All, readout)].iter()) {
                assert!(
                    rect.width() >= widgets::FINGER_SIDE - 1e-5 && rect.height() >= widgets::FINGER_SIDE - 1e-5,
                    "{choice:?} is {rect:?}, under a finger",
                );
            }
        }
    }

    #[test]
    fn every_button_answers_where_it_is_drawn() {
        let bar = Bar::lay_out(Rect::new(0.3, 0.0, 0.4, 0.1), a_panel(), &[Choice::Across { up: false }]);
        for (choice, rect) in &bar.buttons {
            assert_eq!(bar.at((rect.centre_x(), rect.centre_y())), Some(*choice));
            assert_eq!(bar.at((rect.x0 + 0.002, rect.y1 - 0.002)), Some(*choice));
            assert_eq!(bar.at((rect.x1 - 0.002, rect.y0 + 0.002)), Some(*choice));
        }
        let readout = bar.readout.expect("a stack's bar shows its number");
        assert_eq!(bar.at((readout.centre_x(), readout.centre_y())), None, "the number is a button");
        // No two buttons share a point.
        for (i, (_, a)) in bar.buttons.iter().enumerate() {
            for (_, b) in &bar.buttons[i + 1..] {
                assert!(!widgets::crossing(*a, *b, 0.0), "{a:?} and {b:?} overlap");
            }
        }
    }

    #[test]
    fn the_bar_stays_inside_its_bounds_and_off_its_square_at_every_edge() {
        let bounds = a_panel();
        for x in [-0.95, -0.6, 0.0, 0.6, 0.85] {
            for y in [-0.45, -0.2, 0.0, 0.2, 0.35] {
                let square = Rect::new(x, y, x + 0.096, y + 0.096);
                let bar = Bar::lay_out(square, bounds, &[Choice::Across { up: true }]);
                assert!(widgets::holds(bounds, bar.plate, 1e-4), "at {square:?} the bar leaves {bounds:?}");
                assert!(
                    !widgets::crossing(bar.plate, square, 0.0),
                    "at {square:?} the bar covers the square it is about",
                );
            }
        }
    }

    #[test]
    fn the_steps_never_go_under_one_or_past_the_stack() {
        assert_eq!(Choice::Less.portion(Portion::Exactly(1), 12), Some(Portion::Exactly(1)));
        assert_eq!(Choice::Less.portion(Portion::Whole, 12), Some(Portion::Exactly(11)));
        assert_eq!(Choice::Less.portion(Portion::Half, 12), Some(Portion::Exactly(5)));
        assert_eq!(Choice::More.portion(Portion::Exactly(11), 12), Some(Portion::Whole));
        assert_eq!(Choice::More.portion(Portion::Exactly(3), 12), Some(Portion::Exactly(4)));
        assert_eq!(Choice::More.portion(Portion::Whole, 12), Some(Portion::Whole));
        assert_eq!(Choice::Across { up: true }.portion(Portion::Whole, 12), None);
    }
}
