//! A 6x9 bitmap font, kept as a picture rather than as code.
//!
//! One bit per pixel, nine rows per glyph. Covers printable ASCII,
//! Cyrillic and the Polish letters that are not already in ASCII;
//! anything else renders as a hollow box, which is visible, unlike a
//! silent skip.
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
//! **Descenders are real.** The font before this one was 5x7 with no
//! room below the baseline, so `g j p q y` were squashed up onto it and
//! any word containing one -- `player`, `singleplayer`, a username --
//! read as though it were set in small caps with a few letters wrong.
//! Two extra rows fix it, and cost two bytes a glyph. The Cyrillic was
//! drawn afterwards and did not use them for years: `р у д ц щ ф` all
//! sat on the baseline, so Russian -- the language this game actually
//! ships in -- was still being set in the font that argument rejected.
//! They descend now.
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
//!
//! ## Why the font is a file
//!
//! It used to be a `match` of a hundred and seventy-nine binary
//! literals in this file, and that is where every one of the faults the
//! rewrite fixed had been able to hide. `Д`, `Щ` and `Ю` had ink in
//! column 5 and therefore touched the letter after them; `ż` floated a
//! pixel above the line its neighbours stood on; `Ą` was not an `A` with
//! an ogonek but a narrower letter borrowed from the Cyrillic `А`. None
//! of that is visible in a column of `0b100010`s, and all of it is
//! obvious the moment the font is a picture you can open.
//!
//! So the font is `assets/fonts/primitive.png`: one cell per character,
//! `SHEET_COLUMNS` cells to a row, in exactly the order of `ORDER`. Ink
//! is any texel that is not transparent -- the sheet is drawn white on
//! nothing, and the alpha is what decides, so a pack may draw its font
//! in whatever colour is easiest to see in an editor without changing
//! what the game draws.
//!
//! A file on disk beside the executable wins over the copy compiled in,
//! the same way a block texture does; see `use_assets_dir`.
//!
//! ## What was rejected
//!
//! **Proportional widths.** An `i` in a six-pixel cell is mostly air,
//! and per-glyph advances would tighten every line in the game. They
//! were not done, and the reason is not effort: `text_width`,
//! `ink_width`, `measure`, `fit` and `fitted_scale` all rest on the
//! advance being one constant, and so does every layout that reserves
//! room for a label -- and so does the promise that a widget is
//! hit-tested by the exact inverse of what draws it. A variable advance
//! turns each of those into a measurement of a particular string, which
//! is a change to the interface's arithmetic rather than to its font.
//! The fixed grid also buys something real: a column of buttons whose
//! labels each sit on the same grid looks straighter than one where
//! every label is individually optimal.
//!
//! **A real font file -- TTF, BDF, anything parsed.** A parser, a
//! rasteriser and a dependency, in exchange for a typeface that would
//! then have to be hinted to look like anything at nine pixels. The game
//! draws blocks of sixteen texels; its letters are pixels for the same
//! reason its stone is.
//!
//! **Generating the picture from the table at build time.** That keeps
//! the table -- and the table is what the faults hid in.

use std::path::Path;
use std::sync::OnceLock;

/// Glyph width in pixels, including the blank column that separates it
/// from the next glyph.
pub const GLYPH_WIDTH: usize = 6;
/// Full cell height, descender rows included.
pub const GLYPH_HEIGHT: usize = 9;

/// The *cell* a glyph is packed into: its ink, plus the gap that keeps
/// it out of its neighbour.
///
/// **One definition, because there were two and they disagreed.** The
/// atlas packs several glyphs to a layer, and three separate places
/// worked out where each one goes: the sheet that draws them, the table
/// that says where they are, and the count of how many fit. Two of the
/// three used the glyph's size and one used a cell of `+2` by `+1` --
/// which agreed only at a tile of sixteen texels, where the grid happens
/// to be two by one and the vertical stride is never applied. At any
/// other pack resolution the text came out reading somebody else's
/// letters.
///
/// The gap is not decoration: the array has a mip chain, and without it
/// a glyph's ink bleeds into the one beside it at every level below the
/// first.
///
/// Note that this is the cell *in the texture array*, not in the font
/// file: the sheet on disk packs its glyphs edge to edge, because
/// nothing samples it and a font is easier to draw without gutters.
pub const CELL_WIDTH: usize = GLYPH_WIDTH + 2;
pub const CELL_HEIGHT: usize = GLYPH_HEIGHT + 1;
/// Rows from the top of the cell down to the baseline -- what the eye
/// reads as the height of the text.
pub const CAP_HEIGHT: usize = 7;
/// Extra pixels between glyphs. Zero: see the module docs.
pub const GLYPH_SPACING: usize = 0;

/// Every character the font draws, in the order its cells appear in the
/// sheet.
///
/// **A list rather than a range**, because the game speaks four
/// languages and their alphabets do not sit next to each other in
/// Unicode. Printable ASCII, then Cyrillic, then the Polish letters that
/// are not already in ASCII.
///
/// This is also the order of the layers of the font in the texture
/// array, which is why `texture::GLYPHS` is this same constant: the
/// sheet's cells, the atlas's slots and the answer to "can the player
/// type this" are one list, and were never allowed to become three.
///
/// Adding a letter means one more cell at the end of the picture.
pub const ORDER: &str = concat!(
    " !\"#$%&'()*+,-./0123456789:;<=>?@",
    "ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^_`",
    "abcdefghijklmnopqrstuvwxyz{|}~",
    "АБВГДЕЖЗИЙКЛМНОПРСТУФХЦЧШЩЪЫЬЭЮЯЁабвгдежзийклмнопрстуфхцчшщъыьэюяёĄąĆćĘęŁłŃńÓóŚśŹźŻż",
    // **The punctuation a Russian keyboard types**, after the letters. The
    // chat drew `№` -- Shift+3 on that layout -- as the missing box, and a
    // mod's help line with an em dash in it came up with a pink hole where
    // the dash was, in the first screenshot anybody took of a new world.
    // The game's own strings are held to the font by a test; what a player
    // or a mod types is not, so the font has to carry what they reach for.
    "—–№«»…°",
);

/// How many glyph cells there are across the font sheet.
///
/// Sixteen, so the picture is a familiar code-page shape and a glyph's
/// place in it can be counted on two hands. Nothing but this constant
/// decides it: a sheet is read as `ORDER` in reading order, and the
/// only thing that would break by changing it is every existing font
/// file.
pub const SHEET_COLUMNS: usize = 16;

/// The missing-glyph box: a hollow rectangle, so a hole in the font is
/// visible rather than silently swallowed.
///
/// Deliberately *not* a cell of the sheet. It is what a character the
/// font has never heard of gets, and it has to exist even if the sheet
/// itself fails to decode -- which is the one case where drawing boxes
/// is exactly the right thing to do, because the alternative is a game
/// whose menus are blank.
const MISSING: [u8; GLYPH_HEIGHT] = [
    0b111110, 0b100010, 0b100010, 0b100010, 0b100010, 0b100010, 0b111110, 0b000000, 0b000000,
];

/// The decoded sheet: one entry per character of `ORDER`.
static SHEET: OnceLock<Vec<[u8; GLYPH_HEIGHT]>> = OnceLock::new();

/// Prefer the font in an assets folder over the one in the binary.
///
/// The same bargain every block texture gets -- a file on disk replaces
/// the built-in copy, so a pack can bring its own letters -- and it is
/// worth saying why it needs a call at all when a texture needs none.
/// `glyph` is a free function that half the interface reaches for
/// without a handle to anything, so there is nowhere to hang a path;
/// the loader hands one over instead, once, before the atlas is built.
///
/// **A sheet that is the wrong shape is refused rather than used.** A
/// picture with too few cells would leave the last letters of the
/// Polish alphabet as boxes, and a picture of the wrong width would read
/// every glyph off the middle of two others -- both of which look like
/// the game is broken rather than like the font is. It says so on
/// stderr and keeps the font it already had.
///
/// Called after the first character has been drawn, this does nothing
/// but complain: the table is fixed for the life of the process, and a
/// font that changed halfway through would be worse than either.
pub fn use_assets_dir(assets_dir: &Path) {
    let path = assets_dir.join("fonts").join(SHEET_FILE);
    if !path.is_file() {
        return;
    }
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(e) => {
            eprintln!("warning: {} could not be read ({e}); using the built-in font", path.display());
            return;
        }
    };
    let sheet = match decode_sheet(&bytes) {
        Ok(sheet) => sheet,
        Err(e) => {
            eprintln!("warning: {} is not a font sheet ({e}); using the built-in font", path.display());
            return;
        }
    };
    if SHEET.set(sheet).is_err() {
        eprintln!(
            "warning: text was drawn before {} was found, so the font in the binary is the \
             one on screen",
            path.display()
        );
    }
}

/// The name of the font file, inside `assets/fonts/`.
///
/// ASCII, and that is a hard requirement rather than a habit: an asset
/// in an APK is opened through `AAssetManager_open`, which takes a C
/// string, and the packaging script skips anything it cannot name that
/// way -- so a font called `шрифт.png` would simply not be on the phone.
pub const SHEET_FILE: &str = "primitive.png";

/// Turns a font sheet into one row of bits per character of `ORDER`.
///
/// Any texel that is not fully transparent is ink. Colour is ignored on
/// purpose: the game tints text itself, and a sheet drawn in black so
/// its author could see it must draw the same letters as one drawn in
/// white.
fn decode_sheet(bytes: &[u8]) -> anyhow::Result<Vec<[u8; GLYPH_HEIGHT]>> {
    let image = image::load_from_memory(bytes)?.to_rgba8();
    let wanted = ORDER.chars().count();
    let rows = wanted.div_ceil(SHEET_COLUMNS);
    anyhow::ensure!(
        image.width() as usize == SHEET_COLUMNS * GLYPH_WIDTH
            && image.height() as usize >= rows * GLYPH_HEIGHT,
        "a font sheet is {} by at least {} pixels ({SHEET_COLUMNS} cells of \
         {GLYPH_WIDTH}x{GLYPH_HEIGHT} across, {wanted} of them); this one is {}x{}",
        SHEET_COLUMNS * GLYPH_WIDTH,
        rows * GLYPH_HEIGHT,
        image.width(),
        image.height(),
    );
    Ok((0..wanted)
        .map(|index| {
            let origin_x = (index % SHEET_COLUMNS) * GLYPH_WIDTH;
            let origin_y = (index / SHEET_COLUMNS) * GLYPH_HEIGHT;
            let mut glyph = [0u8; GLYPH_HEIGHT];
            for (row, bits) in glyph.iter_mut().enumerate() {
                for column in 0..GLYPH_WIDTH {
                    let texel =
                        image.get_pixel((origin_x + column) as u32, (origin_y + row) as u32);
                    if texel.0[3] != 0 {
                        // Bit 5 is the leftmost pixel, which is the way
                        // every reader of this table walks a row.
                        *bits |= 1 << (GLYPH_WIDTH - 1 - column);
                    }
                }
            }
            glyph
        })
        .collect())
}

/// The font, decoded once.
///
/// A sheet that will not decode leaves this empty, and an empty table
/// draws every character as the missing-glyph box -- which is a menu
/// full of rectangles, and therefore a bug report, rather than a menu
/// full of nothing.
fn sheet() -> &'static [[u8; GLYPH_HEIGHT]] {
    SHEET.get_or_init(|| match decode_sheet(EMBEDDED_SHEET) {
        Ok(sheet) => sheet,
        Err(e) => {
            eprintln!("the font compiled into this build will not decode ({e})");
            Vec::new()
        }
    })
}

/// The font sheet, compiled in, so the game is still one file.
const EMBEDDED_SHEET: &[u8] = crate::embedded::FONT_SHEET;

/// Rows for one character, or the missing-glyph box.
///
/// Bit 5 (`0b100000`) is the leftmost pixel of each row.
///
/// The character is found by walking `ORDER`, which is a hundred and
/// seventy-nine comparisons and looks like the wrong shape for a font
/// lookup. It is not on any hot path: text is drawn from the texture
/// atlas, which asks this once per glyph while it is being built, and
/// the only other callers are the offscreen snapshotter and the tests.
/// A second table mapping character to index would be a second thing
/// that can disagree with `ORDER`, which is the failure this file has
/// been paying for elsewhere.
pub fn glyph(c: char) -> [u8; GLYPH_HEIGHT] {
    let Some(index) = ORDER.chars().position(|g| g == c) else {
        return MISSING;
    };
    sheet().get(index).copied().unwrap_or(MISSING)
}

/// Width in pixels of a rendered string.
pub fn text_width(text: &str) -> usize {
    text.chars().count() * (GLYPH_WIDTH + GLYPH_SPACING)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SNOWMAN: char = '\u{2603}';

    /// The letters that hang below the baseline, in all three alphabets.
    ///
    /// Written out rather than derived, because the point of the list is
    /// that it is a decision: `р` and `у` belong here and did not use to
    /// be, and nothing but a person can say that `ф` does and `Ф` does
    /// not.
    const DESCENDING: &[char] = &[
        ',', ';', '_', 'g', 'j', 'p', 'q', 'y', 'Д', 'Ц', 'Щ', 'д', 'р', 'у', 'ф', 'ц', 'щ', 'Ą',
        'ą', 'Ę', 'ę',
    ];

    #[test]
    fn every_character_the_font_promises_is_actually_drawn() {
        // A character falling through to the box is a hole in the font,
        // and the menus draw arbitrary text the player has typed. This
        // walks the whole of `ORDER` rather than printable ASCII, which
        // is what the old version of it did -- and that is exactly why
        // the faults in the Cyrillic and the Polish went unseen.
        for c in ORDER.chars() {
            if c == ' ' {
                continue;
            }
            assert_ne!(glyph(c), MISSING, "{c:?} has no drawing of its own");
            assert!(glyph(c).iter().any(|r| *r != 0), "{c:?} is blank");
        }
    }

    #[test]
    fn no_letter_in_any_alphabet_touches_the_next_one() {
        // **The fault this test was written for.** The gap between
        // characters lives in column 5 of each cell, and the check for
        // it used to walk `0x20..=0x7e` -- so `Д`, `Щ`, `Ю`, `д`, `щ`
        // and `ю` had ink in it for as long as the game has spoken
        // Russian, and `ЮЖНЫЙ ЩИТ` came out as a run of blots.
        for c in ORDER.chars() {
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
    fn no_glyph_is_wider_than_its_cell() {
        for c in ORDER.chars() {
            assert!(
                glyph(c).iter().all(|r| *r < 0b1000000),
                "{c:?} is wider than {GLYPH_WIDTH} px"
            );
        }
    }

    #[test]
    fn only_the_letters_that_should_hang_below_the_line_do() {
        // Anything else that did would collide with the line beneath.
        for c in ORDER.chars() {
            let descends = glyph(c)[CAP_HEIGHT..].iter().any(|r| *r != 0);
            assert_eq!(
                descends,
                DESCENDING.contains(&c),
                "{c:?} descends: {descends}"
            );
        }
    }

    #[test]
    fn the_letters_with_descenders_actually_use_them() {
        // The whole reason for the two extra rows -- and the reason the
        // Cyrillic was redrawn: `путь` and `цель` were squashed into the
        // x-height while `player` was not.
        for &c in DESCENDING {
            assert!(
                glyph(c)[CAP_HEIGHT..].iter().any(|r| *r != 0),
                "{c:?} is still sitting on the baseline"
            );
        }
    }

    #[test]
    fn every_capital_reaches_the_cap_line_and_stands_on_the_baseline() {
        // `Ц`, `Щ` and `Д` used to be a row shorter than every other
        // capital, because their tails were folded up into the cap
        // height instead of hanging below it -- so a word with one in it
        // had a letter that looked slightly shrunk.
        for c in ORDER.chars().filter(|c| c.is_alphabetic() && c.is_uppercase()) {
            let rows = glyph(c);
            assert_ne!(rows[0], 0, "{c:?} does not reach the top of the line");
            assert_ne!(rows[CAP_HEIGHT - 1], 0, "{c:?} does not stand on the baseline");
        }
    }

    #[test]
    fn every_small_letter_stands_on_the_baseline() {
        // `ż` used to end a row early, so `żółty` had one letter
        // floating a pixel above the other four.
        for c in ORDER.chars().filter(|c| c.is_alphabetic() && c.is_lowercase()) {
            assert_ne!(
                glyph(c)[CAP_HEIGHT - 1],
                0,
                "{c:?} floats above the line the rest of the word stands on"
            );
        }
    }

    #[test]
    fn a_mark_over_a_letter_leaves_the_letter_alone() {
        // **The rule the Cyrillic already followed and the Polish did
        // not.** `й` is `и` with a breve in the two rows an x-height
        // letter leaves empty, and `ё` is `е` with a diaeresis. But
        // `ć ń ó ś ź` were each their own, shorter drawing of the letter
        // underneath -- `ś` had lost the row that makes an `s` an `s` --
        // so Polish text mixed two sizes of the same alphabet.
        for (accented, base) in [
            ('й', 'и'),
            ('ё', 'е'),
            ('ć', 'c'),
            ('ń', 'n'),
            ('ó', 'o'),
            ('ś', 's'),
            ('ź', 'z'),
            ('ż', 'z'),
        ] {
            assert_eq!(
                glyph(accented)[2..],
                glyph(base)[2..],
                "{accented:?} is not {base:?} with a mark above it"
            );
            assert!(
                glyph(accented)[..2].iter().any(|r| *r != 0),
                "{accented:?} has lost its mark"
            );
        }
    }

    #[test]
    fn a_mark_under_a_letter_leaves_the_letter_alone() {
        // The same rule downwards. `Ą` broke it in the worst way
        // available: it was not an `A` with an ogonek at all, it was the
        // Cyrillic `А`, which is a column narrower -- so a Polish word
        // held two different letter As.
        for (accented, base) in [('Ą', 'A'), ('ą', 'a'), ('Ę', 'E'), ('ę', 'e')] {
            assert_eq!(
                glyph(accented)[..CAP_HEIGHT],
                glyph(base)[..CAP_HEIGHT],
                "{accented:?} is not {base:?} with an ogonek under it"
            );
            assert!(
                glyph(accented)[CAP_HEIGHT..].iter().any(|r| *r != 0),
                "{accented:?} has lost its ogonek"
            );
        }
    }

    #[test]
    fn a_stroked_letter_still_contains_the_letter_it_strikes_through() {
        // `Ł` is an `L` with a bar across it and `ł` is an `l`. Drawn
        // separately they drifted: `ł`'s stem stood a column to the left
        // of `l`'s, so a word with both in it had two different stems.
        for (stroked, base) in [('Ł', 'L'), ('ł', 'l')] {
            let (stroked_rows, base_rows) = (glyph(stroked), glyph(base));
            for (row, (a, b)) in stroked_rows.iter().zip(base_rows.iter()).enumerate() {
                assert_eq!(a & b, *b, "{stroked:?} has lost row {row} of {base:?}");
            }
            assert_ne!(stroked_rows, base_rows, "{stroked:?} has no stroke");
        }
    }

    #[test]
    fn a_space_is_blank_and_a_letter_is_not() {
        assert!(glyph(' ').iter().all(|r| *r == 0));
        assert!(glyph('A').iter().any(|r| *r != 0));
    }

    #[test]
    fn an_unmapped_character_shows_a_box_rather_than_nothing() {
        assert_eq!(glyph(SNOWMAN), MISSING);
        assert!(glyph(SNOWMAN).iter().any(|r| *r != 0), "must be visible");
    }

    #[test]
    fn width_is_a_fixed_grid() {
        assert_eq!(text_width(""), 0);
        assert_eq!(text_width("A"), GLYPH_WIDTH);
        assert_eq!(text_width("AB"), GLYPH_WIDTH * 2);
    }

    #[test]
    fn the_font_in_the_binary_is_a_sheet_of_the_right_shape() {
        // The picture is the font now, so a build whose picture will not
        // decode is a build with no letters in it -- and the fallback
        // for that is deliberately a screen full of boxes, which nobody
        // would want to discover from a player's screenshot.
        let sheet = decode_sheet(EMBEDDED_SHEET).expect("the shipped font sheet decodes");
        assert_eq!(sheet.len(), ORDER.chars().count());
    }

    #[test]
    fn a_replacement_sheet_of_the_wrong_shape_is_refused() {
        // A pack that ships half a font must not silently become a game
        // whose Polish is missing: the loader keeps the font it has and
        // says why.
        let mut small = image::RgbaImage::new(
            (SHEET_COLUMNS * GLYPH_WIDTH) as u32,
            (GLYPH_HEIGHT * 2) as u32,
        );
        small.put_pixel(0, 0, image::Rgba([255, 255, 255, 255]));
        let mut bytes = std::io::Cursor::new(Vec::new());
        small
            .write_to(&mut bytes, image::ImageFormat::Png)
            .expect("the fixture encodes");
        assert!(decode_sheet(bytes.get_ref()).is_err(), "too few rows of cells");
        assert!(decode_sheet(b"not a picture at all").is_err());
    }

    #[test]
    fn ink_is_alpha_and_not_colour() {
        // A pack may draw its font in any colour that is easy to see in
        // an editor; the game tints the text itself. Reading brightness
        // instead would turn a black-on-transparent sheet into a font
        // with no letters in it.
        let mut sheet = image::RgbaImage::new(
            (SHEET_COLUMNS * GLYPH_WIDTH) as u32,
            (ORDER.chars().count().div_ceil(SHEET_COLUMNS) * GLYPH_HEIGHT) as u32,
        );
        sheet.put_pixel(0, 0, image::Rgba([0, 0, 0, 255]));
        let mut bytes = std::io::Cursor::new(Vec::new());
        sheet
            .write_to(&mut bytes, image::ImageFormat::Png)
            .expect("the fixture encodes");
        let decoded = decode_sheet(bytes.get_ref()).expect("the fixture is the right shape");
        assert_eq!(decoded[0][0], 0b100000, "black ink is still ink");
    }
}
