//! The walk a player made while carrying a map, and the marks they left.
//!
//! ## What a map knows, and why the server knows it
//!
//! A map (`types::BLOCK_MAP`) shows the land the player has *been* to, not
//! the land their client happened to be sent. Those two used to be the
//! same thing -- the client surveyed every chunk that arrived and drew all
//! of it -- and the difference is the whole feature: a view distance is a
//! machine setting, and a map drawn from it would be a map that got bigger
//! when you bought a better computer.
//!
//! So the record is the server's, kept in the player's profile
//! (`profiles::Profile::trail`), sent to the client on join and extended a
//! cell at a time as they walk. The client still draws the picture from
//! its own survey of the chunks (`logic::map`) -- the server does not send
//! terrain twice -- but it draws **only inside this**. The two together
//! are "a circle around the path", which is what a map of a walk is.
//!
//! ## Only while a map is carried
//!
//! [`Trail::walk`] is called with what the player is carrying, and it does
//! nothing without a map in the pack. That is the decision the item
//! exists to create: the hide is weight and a slot and it goes with your
//! body, and the alternative -- record everything always, draw it once a
//! map is made -- would make the map a thing you craft *after* the
//! journey, at which point it is a reward for having travelled rather than
//! a tool for travelling.
//!
//! ## How it is kept: cells, not blocks
//!
//! One bit a [`CELL`]-block square, as a set of cell coordinates. A walk
//! of a kilometre with [`SIGHT`] to each side is about
//! `1000 * 2 * SIGHT / CELL²` cells -- a few thousand, forty kilobytes of
//! `i32` pairs before the file even packs them. A bitmap of the world
//! would be a bitmap of a world with no edges; a list of exact positions
//! would grow forever at twenty a second.
//!
//! Rejected: keeping the *chunks* the player was in (16 blocks, free to
//! index). A chunk is too coarse to read as a path -- a player who crosses
//! a corner reveals 256 blocks of land they never saw -- and too coarse in
//! the other direction as well: the circle a walk clears would come out as
//! a staircase. Eight is half a chunk, so the boundaries still line up
//! with everything else in the game, and the staircase is below what the
//! eye picks out at the zoom a map is read at.
//!
//! ## The marks
//!
//! A cairn piled or a blaze cut goes on the map **if the player was
//! carrying a map at the time** -- the trail's own rule, for the trail's
//! reason. What is stored is the cell, the kind and the name; what is not
//! stored is any live link to the block, so a cairn somebody takes apart
//! leaves its mark on the maps of everyone who wrote it down until they go
//! back and see (`Trail::forget_mark`). That is a hand-drawn map's
//! behaviour and it is the honest one: the paper does not know.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::inventory::Inventory;
use crate::types::{block_kind, BlockId, BLOCK_BLAZE, BLOCK_CAIRN, BLOCK_MAP};

/// How many blocks across one cell of the record is.
///
/// Half a chunk. See the module note for why not a whole one.
pub const CELL: i32 = 8;

/// How far to each side of the path is filled in, in blocks.
///
/// **What a walker takes in, not what the client was sent.** Forty-eight
/// is three chunks, comfortably inside every view distance the game
/// offers, so the picture is never short of the survey that draws it --
/// a trail wider than the streamed chunks would be a trail with holes in
/// it that fill in later, which reads as a broken map rather than as a
/// distant hill.
///
/// It is also about what you can honestly say you looked at from a path:
/// far enough that a walk down a valley puts both its sides on the paper,
/// near enough that the far ridge is somewhere you still have to go.
pub const SIGHT: i32 = 48;

/// What a mark on the map was in the world.
///
/// Two, and the difference is drawn (`ui::map_screen`): they are put down
/// in different places for different prices and a player wants to know
/// which of their marks is the heap of stones on the moor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MarkKind {
    Cairn,
    Blaze,
}

impl MarkKind {
    /// Which mark a block is, if it is one.
    pub fn of(block: BlockId) -> Option<MarkKind> {
        match block_kind(block) {
            BLOCK_CAIRN => Some(MarkKind::Cairn),
            BLOCK_BLAZE => Some(MarkKind::Blaze),
            _ => None,
        }
    }
}

/// The longest name a mark keeps, in characters: about what fits beside
/// a mark on a phone's map at the size it is drawn.
pub const MARK_NAME_CHARS: usize = 24;

/// One thing a player wrote down.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mark {
    /// The cell of the block in the world.
    pub at: (i32, i32, i32),
    pub kind: MarkKind,
    /// What the player called it; empty for one they did not name.
    pub name: String,
}

/// Where a player has been with a map, and what they marked.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Trail {
    /// The cells walked, in `CELL` units, sorted -- so the profile's bytes
    /// are the same for the same walk and a save with nothing new in it
    /// compares equal.
    cells: BTreeSet<(i32, i32)>,
    marks: Vec<Mark>,
}

/// Is there a map in this pack?
///
/// The hand is part of the pack here, and deliberately: a map held out in
/// front of you is emphatically being carried. See `BLOCK_MAP`.
pub fn carries_map(inventory: &Inventory) -> bool {
    inventory
        .slots()
        .iter()
        .flatten()
        .any(|stack| block_kind(stack.block) == BLOCK_MAP)
}

/// Which cell a world position falls in.
///
/// Floor division, not a truncation: `-1 / 8` is zero in Rust and the cell
/// west of the origin is `-1`. Getting this wrong puts a seam down the
/// axis of the world that nobody finds until somebody walks across it.
pub fn cell_of(x: i32, z: i32) -> (i32, i32) {
    (x.div_euclid(CELL), z.div_euclid(CELL))
}

impl Trail {
    /// How many cells have been walked. For the log and the tests.
    pub fn len(&self) -> usize {
        self.cells.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    /// Every cell walked, for the wire and the file.
    pub fn cells(&self) -> impl Iterator<Item = (i32, i32)> + '_ {
        self.cells.iter().copied()
    }

    pub fn marks(&self) -> &[Mark] {
        &self.marks
    }

    /// Is this block position somewhere the player has walked?
    pub fn knows(&self, x: i32, z: i32) -> bool {
        self.cells.contains(&cell_of(x, z))
    }

    /// Takes in cells the trail did not have. Answers the new ones, so a
    /// caller can send exactly those and nothing else.
    pub fn absorb(&mut self, cells: impl IntoIterator<Item = (i32, i32)>) -> Vec<(i32, i32)> {
        cells.into_iter().filter(|&cell| self.cells.insert(cell)).collect()
    }

    /// The player is at `(x, z)` carrying `inventory`. Answers the cells
    /// this added, which is nothing at all on the great majority of calls
    /// -- a player standing still, or walking inside a cell they have
    /// already been in.
    ///
    /// **A disc, not a square.** The corner of a square is `SIGHT * 1.41`
    /// away, and a map whose cleared area is square gives away that it is
    /// drawn by a machine: the eye reads the corners immediately. The test
    /// `the_walk_fills_a_circle_and_not_a_square` says so.
    pub fn walk(&mut self, x: i32, z: i32, inventory: &Inventory) -> Vec<(i32, i32)> {
        if !carries_map(inventory) {
            return Vec::new();
        }
        let (cx, cz) = cell_of(x, z);
        // In cells, rounded up, so the disc's edge is not clipped by the
        // grid the record is kept on.
        let reach = SIGHT.div_euclid(CELL) + 1;
        let mut added = Vec::new();
        for dz in -reach..=reach {
            for dx in -reach..=reach {
                // Measured from the player to the *middle* of the cell, in
                // blocks: a cell is counted walked when most of it is
                // inside the circle rather than when its corner clips it.
                let mx = (cx + dx) * CELL + CELL / 2 - x;
                let mz = (cz + dz) * CELL + CELL / 2 - z;
                if mx * mx + mz * mz > SIGHT * SIGHT {
                    continue;
                }
                if self.cells.insert((cx + dx, cz + dz)) {
                    added.push((cx + dx, cz + dz));
                }
            }
        }
        added
    }

    /// Writes a mark down, if the player is carrying a map. Answers it, so
    /// a caller can send exactly what was written.
    ///
    /// A mark already at that cell is left alone rather than replaced: it
    /// is the same block, and replacing it would throw away the name.
    pub fn mark(&mut self, at: (i32, i32, i32), kind: MarkKind, inventory: &Inventory) -> Option<Mark> {
        if !carries_map(inventory) || self.marks.iter().any(|mark| mark.at == at) {
            return None;
        }
        // The cell a mark stands in is walked by definition -- you were
        // there to put it there -- and saying so here means a mark is never
        // drawn on unseen ground, which would look like a bug in the map
        // rather than like a mark.
        self.cells.insert(cell_of(at.0, at.2));
        let mark = Mark { at, kind, name: String::new() };
        self.marks.push(mark.clone());
        Some(mark)
    }

    /// Names the mark at `at`. Answers whether anything changed.
    pub fn name_mark(&mut self, at: (i32, i32, i32), name: &str) -> bool {
        let name: String = name.trim().chars().take(MARK_NAME_CHARS).collect();
        let Some(mark) = self.marks.iter_mut().find(|mark| mark.at == at) else {
            return false;
        };
        if mark.name == name {
            return false;
        }
        mark.name = name;
        true
    }

    /// Rubs a mark out: the player went back and the heap was gone.
    /// Answers whether there was one.
    pub fn forget_mark(&mut self, at: (i32, i32, i32)) -> bool {
        let before = self.marks.len();
        self.marks.retain(|mark| mark.at != at);
        self.marks.len() != before
    }

    /// Puts marks in wholesale: the wire's arrival, and the file's.
    pub fn absorb_marks(&mut self, marks: impl IntoIterator<Item = Mark>) {
        for mark in marks {
            if let Some(old) = self.marks.iter_mut().find(|old| old.at == mark.at) {
                *old = mark;
            } else {
                self.marks.push(mark);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{BLOCK_PEBBLE, BLOCK_STONE};

    fn with_map() -> Inventory {
        let mut pack = Inventory::new();
        pack.add(BLOCK_MAP, 1);
        pack
    }

    fn without() -> Inventory {
        let mut pack = Inventory::new();
        pack.add(BLOCK_STONE, 4);
        pack
    }

    #[test]
    fn a_walk_with_no_map_in_the_pack_is_not_written_down() {
        let mut trail = Trail::default();
        assert!(trail.walk(0, 0, &without()).is_empty());
        assert!(trail.is_empty(), "the land filled in for a player carrying nothing");
        assert!(!trail.knows(0, 0));
        trail.walk(0, 0, &with_map());
        assert!(trail.knows(0, 0), "the map in the pack wrote nothing down");
    }

    #[test]
    fn the_map_fills_only_where_the_player_has_been() {
        let mut trail = Trail::default();
        trail.walk(0, 0, &with_map());
        assert!(trail.knows(SIGHT - CELL, 0), "the near ground was not filled in");
        // Twice the sight, in the same direction: never been there.
        assert!(!trail.knows(SIGHT * 2, 0), "land nobody walked to is on the map");
        assert!(!trail.knows(0, -SIGHT * 2));
        // ...and walking there fills it, and does not lose the first patch.
        trail.walk(SIGHT * 2, 0, &with_map());
        assert!(trail.knows(SIGHT * 2, 0));
        assert!(trail.knows(0, 0), "the walk forgot where it started");
    }

    #[test]
    fn the_walk_fills_a_circle_and_not_a_square() {
        let mut trail = Trail::default();
        trail.walk(0, 0, &with_map());
        // Straight out is inside the circle; the same distance on both
        // axes at once is a corner, and a corner is a square's giveaway.
        let out = SIGHT - CELL;
        assert!(trail.knows(out, 0) && trail.knows(0, out));
        assert!(!trail.knows(out, out), "the cleared ground has corners");
    }

    #[test]
    fn walking_the_same_ground_twice_adds_nothing() {
        let mut trail = Trail::default();
        let first = trail.walk(100, -100, &with_map());
        assert!(!first.is_empty());
        assert!(trail.walk(100, -100, &with_map()).is_empty(), "standing still grew the record");
        // One block over is the same cell and almost the same disc: a few
        // cells at the rim at most, never the whole disc again.
        let nudged = trail.walk(101, -100, &with_map());
        assert!(nudged.len() < first.len() / 4, "a step of one block redrew the circle");
    }

    #[test]
    fn a_cell_west_of_the_origin_is_not_the_cell_east_of_it() {
        // Truncating division would put -1 and +1 in cell 0 together, and
        // the map would show a strip it had never been sent.
        assert_eq!(cell_of(-1, -1), (-1, -1));
        assert_eq!(cell_of(0, 0), (0, 0));
        assert_ne!(cell_of(-CELL, 0), cell_of(0, 0));
    }

    #[test]
    fn a_mark_is_written_down_only_by_a_player_carrying_a_map() {
        let mut trail = Trail::default();
        assert!(trail.mark((10, 64, 10), MarkKind::Cairn, &without()).is_none());
        assert!(trail.marks().is_empty());
        assert!(trail.mark((10, 64, 10), MarkKind::Cairn, &with_map()).is_some());
        assert_eq!(trail.marks().len(), 1);
        // The same cell twice is one mark: the second placement is the
        // same heap of stones.
        assert!(trail.mark((10, 64, 10), MarkKind::Cairn, &with_map()).is_none());
        assert_eq!(trail.marks().len(), 1);
    }

    #[test]
    fn a_mark_is_named_and_stays_named_and_can_be_rubbed_out() {
        let mut trail = Trail::default();
        trail.mark((1, 2, 3), MarkKind::Blaze, &with_map());
        assert!(trail.name_mark((1, 2, 3), "  the ford  "));
        assert_eq!(trail.marks()[0].name, "the ford", "the name kept its whitespace");
        assert!(!trail.name_mark((1, 2, 3), "the ford"), "naming it the same thing was a change");
        assert!(!trail.name_mark((9, 9, 9), "nowhere"), "a mark that is not there took a name");
        assert!(trail.forget_mark((1, 2, 3)));
        assert!(trail.marks().is_empty());
        assert!(!trail.forget_mark((1, 2, 3)));
    }

    #[test]
    fn a_name_longer_than_the_map_can_draw_is_cut_to_what_fits() {
        let mut trail = Trail::default();
        trail.mark((0, 0, 0), MarkKind::Cairn, &with_map());
        trail.name_mark((0, 0, 0), &"я".repeat(MARK_NAME_CHARS * 3));
        assert_eq!(
            trail.marks()[0].name.chars().count(),
            MARK_NAME_CHARS,
            "a long name is cut by characters, never by bytes"
        );
    }

    #[test]
    fn the_ground_under_a_mark_is_on_the_map_even_at_the_first_step() {
        // Otherwise the very first cairn of a new map is a mark floating on
        // black, because the walk that put it there had not been recorded
        // yet.
        let mut trail = Trail::default();
        trail.mark((500, 70, -500), MarkKind::Cairn, &with_map());
        assert!(trail.knows(500, -500));
    }

    #[test]
    fn a_pebble_is_not_a_mark_and_a_cairn_is() {
        assert_eq!(MarkKind::of(BLOCK_CAIRN), Some(MarkKind::Cairn));
        assert_eq!(MarkKind::of(BLOCK_BLAZE), Some(MarkKind::Blaze));
        assert_eq!(MarkKind::of(BLOCK_PEBBLE), None);
    }
}
