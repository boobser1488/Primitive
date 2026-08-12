//! A 6x9 bitmap font, as data.
//!
//! One bit per pixel, nine bytes per glyph, no font file to ship or
//! parse and no dependency to add. Covers printable ASCII; anything else
//! renders as a hollow box, which is visible, unlike a silent skip.
//!
//! ## The cell
//!
//! ```text
//!   rows 0..6   cap height -- where capitals, digits and x-height live
//!   rows 7..8   below the baseline, for descenders
//!   cols 0..4   the glyph
//!   col  5      the gap to the next glyph
//! ```
//!
//! Two decisions worth stating, because both replaced something worse.
//!
//! **Descenders are real.** The previous font was 5x7 with no room
//! below the baseline, so `g j p q y` were squashed up onto it and any
//! word containing one -- `player`, `singleplayer`, a username -- read
//! as though it were set in small caps with a few letters wrong. Two
//! extra rows fix it, and cost two bytes a glyph.
//!
//! **The spacing is inside the cell.** Column 5 is blank in every glyph
//! rather than a gap added between glyphs, so advancing is one add and
//! text lines up on a fixed 6-pixel grid. `_` is the deliberate
//! exception: it fills all six columns so a run of underscores joins
//! into a continuous rule, which is what an underscore is for.
//!
//! Vertical placement uses `CAP_HEIGHT`, not `GLYPH_HEIGHT`. Centring a
//! line on the full cell would sit it visibly low, because the two
//! descender rows are empty for all but a handful of characters.

/// Glyph width in pixels, including the blank column that separates it
/// from the next glyph.
pub const GLYPH_WIDTH: usize = 6;
/// Full cell height, descender rows included.
pub const GLYPH_HEIGHT: usize = 9;
/// Rows from the top of the cell down to the baseline -- what the eye
/// reads as the height of the text.
pub const CAP_HEIGHT: usize = 7;
/// Extra pixels between glyphs. Zero: see the module docs.
pub const GLYPH_SPACING: usize = 0;

/// Rows for one character, or the missing-glyph box.
///
/// Bit 5 (`0b100000`) is the leftmost pixel of each row.
pub fn glyph(c: char) -> [u8; GLYPH_HEIGHT] {
    match c {
        ' ' => [0b000000, 0b000000, 0b000000, 0b000000, 0b000000, 0b000000, 0b000000, 0b000000, 0b000000],
        '!' => [0b011000, 0b011000, 0b011000, 0b011000, 0b011000, 0b000000, 0b011000, 0b000000, 0b000000],
        '"' => [0b010100, 0b010100, 0b010100, 0b000000, 0b000000, 0b000000, 0b000000, 0b000000, 0b000000],
        '#' => [0b010100, 0b010100, 0b111110, 0b010100, 0b111110, 0b010100, 0b010100, 0b000000, 0b000000],
        '$' => [0b001000, 0b011110, 0b101000, 0b011100, 0b001010, 0b111100, 0b001000, 0b000000, 0b000000],
        '%' => [0b110010, 0b110010, 0b000100, 0b001000, 0b010000, 0b100110, 0b100110, 0b000000, 0b000000],
        '&' => [0b011000, 0b100100, 0b101000, 0b010000, 0b101010, 0b100100, 0b011010, 0b000000, 0b000000],
        '\'' => [0b011000, 0b011000, 0b010000, 0b000000, 0b000000, 0b000000, 0b000000, 0b000000, 0b000000],
        '(' => [0b001100, 0b010000, 0b010000, 0b010000, 0b010000, 0b010000, 0b001100, 0b000000, 0b000000],
        ')' => [0b011000, 0b000100, 0b000100, 0b000100, 0b000100, 0b000100, 0b011000, 0b000000, 0b000000],
        '*' => [0b000000, 0b101010, 0b011100, 0b111110, 0b011100, 0b101010, 0b000000, 0b000000, 0b000000],
        '+' => [0b000000, 0b001000, 0b001000, 0b111110, 0b001000, 0b001000, 0b000000, 0b000000, 0b000000],
        ',' => [0b000000, 0b000000, 0b000000, 0b000000, 0b000000, 0b011000, 0b011000, 0b001000, 0b010000],
        '-' => [0b000000, 0b000000, 0b000000, 0b111110, 0b000000, 0b000000, 0b000000, 0b000000, 0b000000],
        '.' => [0b000000, 0b000000, 0b000000, 0b000000, 0b000000, 0b011000, 0b011000, 0b000000, 0b000000],
        '/' => [0b000010, 0b000010, 0b000100, 0b001000, 0b010000, 0b100000, 0b100000, 0b000000, 0b000000],
        '0' => [0b011100, 0b100010, 0b100110, 0b101010, 0b110010, 0b100010, 0b011100, 0b000000, 0b000000],
        '1' => [0b001000, 0b011000, 0b001000, 0b001000, 0b001000, 0b001000, 0b011100, 0b000000, 0b000000],
        '2' => [0b011100, 0b100010, 0b000010, 0b000100, 0b001000, 0b010000, 0b111110, 0b000000, 0b000000],
        '3' => [0b111100, 0b000010, 0b000010, 0b011100, 0b000010, 0b100010, 0b011100, 0b000000, 0b000000],
        '4' => [0b000100, 0b001100, 0b010100, 0b100100, 0b111110, 0b000100, 0b000100, 0b000000, 0b000000],
        '5' => [0b111110, 0b100000, 0b111100, 0b000010, 0b000010, 0b100010, 0b011100, 0b000000, 0b000000],
        '6' => [0b001100, 0b010000, 0b100000, 0b111100, 0b100010, 0b100010, 0b011100, 0b000000, 0b000000],
        '7' => [0b111110, 0b000010, 0b000100, 0b001000, 0b001000, 0b001000, 0b001000, 0b000000, 0b000000],
        '8' => [0b011100, 0b100010, 0b100010, 0b011100, 0b100010, 0b100010, 0b011100, 0b000000, 0b000000],
        '9' => [0b011100, 0b100010, 0b100010, 0b011110, 0b000010, 0b000100, 0b011000, 0b000000, 0b000000],
        ':' => [0b000000, 0b011000, 0b011000, 0b000000, 0b011000, 0b011000, 0b000000, 0b000000, 0b000000],
        ';' => [0b000000, 0b011000, 0b011000, 0b000000, 0b011000, 0b011000, 0b001000, 0b010000, 0b000000],
        '<' => [0b000100, 0b001000, 0b010000, 0b100000, 0b010000, 0b001000, 0b000100, 0b000000, 0b000000],
        '=' => [0b000000, 0b000000, 0b111110, 0b000000, 0b111110, 0b000000, 0b000000, 0b000000, 0b000000],
        '>' => [0b100000, 0b010000, 0b001000, 0b000100, 0b001000, 0b010000, 0b100000, 0b000000, 0b000000],
        '?' => [0b011100, 0b100010, 0b000010, 0b000100, 0b001000, 0b000000, 0b001000, 0b000000, 0b000000],
        '@' => [0b011100, 0b100010, 0b101110, 0b101010, 0b101110, 0b100000, 0b011110, 0b000000, 0b000000],
        'A' => [0b011100, 0b100010, 0b100010, 0b111110, 0b100010, 0b100010, 0b100010, 0b000000, 0b000000],
        'B' => [0b111100, 0b100010, 0b100010, 0b111100, 0b100010, 0b100010, 0b111100, 0b000000, 0b000000],
        'C' => [0b011100, 0b100010, 0b100000, 0b100000, 0b100000, 0b100010, 0b011100, 0b000000, 0b000000],
        'D' => [0b111000, 0b100100, 0b100010, 0b100010, 0b100010, 0b100100, 0b111000, 0b000000, 0b000000],
        'E' => [0b111110, 0b100000, 0b100000, 0b111100, 0b100000, 0b100000, 0b111110, 0b000000, 0b000000],
        'F' => [0b111110, 0b100000, 0b100000, 0b111100, 0b100000, 0b100000, 0b100000, 0b000000, 0b000000],
        'G' => [0b011100, 0b100010, 0b100000, 0b101110, 0b100010, 0b100010, 0b011100, 0b000000, 0b000000],
        'H' => [0b100010, 0b100010, 0b100010, 0b111110, 0b100010, 0b100010, 0b100010, 0b000000, 0b000000],
        'I' => [0b011100, 0b001000, 0b001000, 0b001000, 0b001000, 0b001000, 0b011100, 0b000000, 0b000000],
        'J' => [0b001110, 0b000010, 0b000010, 0b000010, 0b100010, 0b100010, 0b011100, 0b000000, 0b000000],
        'K' => [0b100010, 0b100100, 0b101000, 0b110000, 0b101000, 0b100100, 0b100010, 0b000000, 0b000000],
        'L' => [0b100000, 0b100000, 0b100000, 0b100000, 0b100000, 0b100000, 0b111110, 0b000000, 0b000000],
        'M' => [0b100010, 0b110110, 0b101010, 0b101010, 0b100010, 0b100010, 0b100010, 0b000000, 0b000000],
        'N' => [0b100010, 0b110010, 0b101010, 0b101010, 0b100110, 0b100010, 0b100010, 0b000000, 0b000000],
        'O' => [0b011100, 0b100010, 0b100010, 0b100010, 0b100010, 0b100010, 0b011100, 0b000000, 0b000000],
        'P' => [0b111100, 0b100010, 0b100010, 0b111100, 0b100000, 0b100000, 0b100000, 0b000000, 0b000000],
        'Q' => [0b011100, 0b100010, 0b100010, 0b100010, 0b101010, 0b100100, 0b011010, 0b000000, 0b000000],
        'R' => [0b111100, 0b100010, 0b100010, 0b111100, 0b101000, 0b100100, 0b100010, 0b000000, 0b000000],
        'S' => [0b011110, 0b100000, 0b100000, 0b011100, 0b000010, 0b000010, 0b111100, 0b000000, 0b000000],
        'T' => [0b111110, 0b001000, 0b001000, 0b001000, 0b001000, 0b001000, 0b001000, 0b000000, 0b000000],
        'U' => [0b100010, 0b100010, 0b100010, 0b100010, 0b100010, 0b100010, 0b011100, 0b000000, 0b000000],
        'V' => [0b100010, 0b100010, 0b100010, 0b100010, 0b100010, 0b010100, 0b001000, 0b000000, 0b000000],
        'W' => [0b100010, 0b100010, 0b100010, 0b101010, 0b101010, 0b110110, 0b100010, 0b000000, 0b000000],
        'X' => [0b100010, 0b100010, 0b010100, 0b001000, 0b010100, 0b100010, 0b100010, 0b000000, 0b000000],
        'Y' => [0b100010, 0b100010, 0b010100, 0b001000, 0b001000, 0b001000, 0b001000, 0b000000, 0b000000],
        'Z' => [0b111110, 0b000010, 0b000100, 0b001000, 0b010000, 0b100000, 0b111110, 0b000000, 0b000000],
        '[' => [0b011100, 0b010000, 0b010000, 0b010000, 0b010000, 0b010000, 0b011100, 0b000000, 0b000000],
        '\\' => [0b100000, 0b100000, 0b010000, 0b001000, 0b000100, 0b000010, 0b000010, 0b000000, 0b000000],
        ']' => [0b011100, 0b000100, 0b000100, 0b000100, 0b000100, 0b000100, 0b011100, 0b000000, 0b000000],
        '^' => [0b001000, 0b010100, 0b100010, 0b000000, 0b000000, 0b000000, 0b000000, 0b000000, 0b000000],
        '_' => [0b000000, 0b000000, 0b000000, 0b000000, 0b000000, 0b000000, 0b000000, 0b111111, 0b000000],
        '`' => [0b010000, 0b001000, 0b000000, 0b000000, 0b000000, 0b000000, 0b000000, 0b000000, 0b000000],
        'a' => [0b000000, 0b000000, 0b011100, 0b000010, 0b011110, 0b100010, 0b011110, 0b000000, 0b000000],
        'b' => [0b100000, 0b100000, 0b111100, 0b100010, 0b100010, 0b100010, 0b111100, 0b000000, 0b000000],
        'c' => [0b000000, 0b000000, 0b011110, 0b100000, 0b100000, 0b100000, 0b011110, 0b000000, 0b000000],
        'd' => [0b000010, 0b000010, 0b011110, 0b100010, 0b100010, 0b100010, 0b011110, 0b000000, 0b000000],
        'e' => [0b000000, 0b000000, 0b011100, 0b100010, 0b111110, 0b100000, 0b011100, 0b000000, 0b000000],
        'f' => [0b001100, 0b010010, 0b010000, 0b111100, 0b010000, 0b010000, 0b010000, 0b000000, 0b000000],
        'g' => [0b000000, 0b000000, 0b011110, 0b100010, 0b100010, 0b011110, 0b000010, 0b100010, 0b011100],
        'h' => [0b100000, 0b100000, 0b111100, 0b100010, 0b100010, 0b100010, 0b100010, 0b000000, 0b000000],
        'i' => [0b001000, 0b000000, 0b011000, 0b001000, 0b001000, 0b001000, 0b011100, 0b000000, 0b000000],
        'j' => [0b000100, 0b000000, 0b001100, 0b000100, 0b000100, 0b000100, 0b000100, 0b100100, 0b011000],
        'k' => [0b100000, 0b100000, 0b100100, 0b101000, 0b110000, 0b101000, 0b100100, 0b000000, 0b000000],
        'l' => [0b011000, 0b001000, 0b001000, 0b001000, 0b001000, 0b001000, 0b011100, 0b000000, 0b000000],
        'm' => [0b000000, 0b000000, 0b110100, 0b101010, 0b101010, 0b101010, 0b101010, 0b000000, 0b000000],
        'n' => [0b000000, 0b000000, 0b111100, 0b100010, 0b100010, 0b100010, 0b100010, 0b000000, 0b000000],
        'o' => [0b000000, 0b000000, 0b011100, 0b100010, 0b100010, 0b100010, 0b011100, 0b000000, 0b000000],
        'p' => [0b000000, 0b000000, 0b111100, 0b100010, 0b100010, 0b111100, 0b100000, 0b100000, 0b100000],
        'q' => [0b000000, 0b000000, 0b011110, 0b100010, 0b100010, 0b011110, 0b000010, 0b000010, 0b000010],
        'r' => [0b000000, 0b000000, 0b101110, 0b110000, 0b100000, 0b100000, 0b100000, 0b000000, 0b000000],
        's' => [0b000000, 0b000000, 0b011110, 0b100000, 0b011100, 0b000010, 0b111100, 0b000000, 0b000000],
        't' => [0b010000, 0b010000, 0b111100, 0b010000, 0b010000, 0b010010, 0b001100, 0b000000, 0b000000],
        'u' => [0b000000, 0b000000, 0b100010, 0b100010, 0b100010, 0b100110, 0b011010, 0b000000, 0b000000],
        'v' => [0b000000, 0b000000, 0b100010, 0b100010, 0b100010, 0b010100, 0b001000, 0b000000, 0b000000],
        'w' => [0b000000, 0b000000, 0b100010, 0b101010, 0b101010, 0b101010, 0b010100, 0b000000, 0b000000],
        'x' => [0b000000, 0b000000, 0b100010, 0b010100, 0b001000, 0b010100, 0b100010, 0b000000, 0b000000],
        'y' => [0b000000, 0b000000, 0b100010, 0b100010, 0b100010, 0b011110, 0b000010, 0b100010, 0b011100],
        'z' => [0b000000, 0b000000, 0b111110, 0b000100, 0b001000, 0b010000, 0b111110, 0b000000, 0b000000],
        '{' => [0b000110, 0b001000, 0b001000, 0b010000, 0b001000, 0b001000, 0b000110, 0b000000, 0b000000],
        '|' => [0b001000, 0b001000, 0b001000, 0b001000, 0b001000, 0b001000, 0b001000, 0b000000, 0b000000],
        '}' => [0b011000, 0b000100, 0b000100, 0b000010, 0b000100, 0b000100, 0b011000, 0b000000, 0b000000],
        '~' => [0b000000, 0b000000, 0b011000, 0b100110, 0b000000, 0b000000, 0b000000, 0b000000, 0b000000],
        // Missing glyph: a hollow box, so a gap in the font is visible
        // rather than silently swallowed.
        _ => [
            0b111110, 0b100010, 0b100010, 0b100010, 0b100010, 0b100010, 0b111110, 0b000000,
            0b000000,
        ],
    }
}

/// Width in pixels of a rendered string.
pub fn text_width(text: &str) -> usize {
    text.chars().count() * (GLYPH_WIDTH + GLYPH_SPACING)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SNOWMAN: char = '\u{2603}';

    #[test]
    fn every_printable_ascii_has_its_own_glyph() {
        // A character falling through to the box is a hole in the font,
        // and the menus draw arbitrary text the player has typed.
        let box_glyph = glyph(SNOWMAN);
        for byte in 0x20u8..=0x7e {
            let c = byte as char;
            assert_ne!(glyph(c), box_glyph, "{c:?} has no glyph of its own");
        }
    }

    #[test]
    fn no_glyph_is_wider_than_its_cell() {
        for byte in 0x20u8..=0x7e {
            let rows = glyph(byte as char);
            assert!(
                rows.iter().all(|r| *r < 0b1000000),
                "{:?} is wider than {GLYPH_WIDTH} px",
                byte as char
            );
        }
    }

    #[test]
    fn glyphs_leave_the_spacing_column_clear() {
        // The gap between characters lives in column 5 of each cell. A
        // glyph that fills it runs into its neighbour.
        for byte in 0x20u8..=0x7e {
            let c = byte as char;
            if c == '_' {
                continue; // deliberately joins up, see the module docs
            }
            assert!(
                glyph(c).iter().all(|r| r & 0b000001 == 0),
                "{c:?} touches the next glyph"
            );
        }
    }

    #[test]
    fn only_the_descenders_reach_below_the_baseline() {
        // Anything else that did would collide with the line beneath.
        let expected = [',', ';', 'g', 'j', 'p', 'q', 'y', '_'];
        for byte in 0x20u8..=0x7e {
            let c = byte as char;
            let descends = glyph(c)[CAP_HEIGHT..].iter().any(|r| *r != 0);
            assert_eq!(descends, expected.contains(&c), "{c:?} descends: {descends}");
        }
    }

    #[test]
    fn the_letters_with_descenders_actually_use_them() {
        // The whole reason for the two extra rows.
        for c in ['g', 'j', 'p', 'q', 'y'] {
            assert!(
                glyph(c)[CAP_HEIGHT..].iter().any(|r| *r != 0),
                "{c:?} is still sitting on the baseline"
            );
        }
    }

    #[test]
    fn a_space_is_blank_and_a_letter_is_not() {
        assert!(glyph(' ').iter().all(|r| *r == 0));
        assert!(glyph('A').iter().any(|r| *r != 0));
    }

    #[test]
    fn an_unmapped_character_shows_a_box_rather_than_nothing() {
        assert!(glyph(SNOWMAN).iter().any(|r| *r != 0), "must be visible");
    }

    #[test]
    fn width_is_a_fixed_grid() {
        assert_eq!(text_width(""), 0);
        assert_eq!(text_width("A"), GLYPH_WIDTH);
        assert_eq!(text_width("AB"), GLYPH_WIDTH * 2);
    }
}
