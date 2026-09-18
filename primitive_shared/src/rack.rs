//! A frame with a skin stretched on it, and what the two sides of it
//! both have to know.
//!
//! ## Why a rack has an inside now
//!
//! A drying rack used to be a block you right-clicked with a hide and
//! right-clicked again when it was done, and the state -- one skin and
//! how far along it was -- lived in a map of its own on the server. That
//! is the smallest thing that works, and it has the same three holes the
//! hearth had before it got a screen:
//!
//! * **The one question a rack exists to answer had nowhere to be
//!   asked.** "Is it dry yet?" was answered by right-clicking it, which
//!   is also the gesture that takes the skin *off* -- so the only way to
//!   check was to interfere, and a hide taken off early comes back a
//!   hide.
//! * **Nothing said why it was not moving.** Rain stops a rack and frost
//!   stops it; both look exactly like a rack that is working, because a
//!   process that takes twelve minutes looks like nothing at all from
//!   the outside.
//! * **It was a second container system.** Everything a chest has --
//!   opening, moving a stack in and out, spilling when the block breaks,
//!   being saved -- had a bespoke second implementation for the one
//!   block that held one item.
//!
//! So a rack is a container, on the terms `hearth` already set: two
//! slots with roles, the contents in the ordinary container store, and
//! what the *weather* is doing to it sent along to whoever is looking.
//!
//! ## The two slots
//!
//! A skin goes in the near one and the leather comes out of the far one,
//! and the frame works through the stack a skin at a time -- which is
//! what a rack in a camp is: a thing you load, walk away from, and come
//! back to. Nothing may be put *into* the far slot, for the reason
//! nothing may be put into a hearth's output: a rack whose output can be
//! filled by hand is a rack that can be jammed.
//!
//! ## Why the layout is here and not in the server
//!
//! Both sides count on it. The server refuses a move that would put a
//! stone in the frame or anything at all in the output; the client draws
//! the two slots in their places and must not offer a gesture the server
//! will silently drop. One table, read twice -- see the same note in
//! [`crate::hearth`].

use crate::inventory::Inventory;
use crate::types::{
    block_kind, BlockId, BLOCK_DRIED_MEAT, BLOCK_DRIED_PEAT, BLOCK_DRYING_RACK, BLOCK_HIDE, BLOCK_HIDE_FRAME,
    BLOCK_LEATHER, BLOCK_PEAT, BLOCK_RAW_MEAT,
};

/// The slot a raw skin is laid in.
pub const HIDE_SLOT: usize = 0;
/// ...and the one the cured leather comes off into.
pub const LEATHER_SLOT: usize = 1;
/// How many slots of the underlying inventory a rack uses at all.
///
/// The rest of the forty are refused by the server and never drawn, the
/// way a hearth's are: a rack shares the container store with the
/// chests, and the price of that is a type with more slots in it than
/// this needs. The price of the alternative is a second container
/// system, which is what this replaced.
pub const USED_SLOTS: usize = 2;

/// How long one hide takes to cure in ideal weather, in seconds.
///
/// Twelve minutes, which at the default day length is a bit over three
/// quarters of a day. Long enough that a player puts skins out and goes
/// and does something else -- which is the whole point of the mechanic
/// -- and short enough that the first coat is a thing they get in their
/// first evening rather than their third.
///
/// Shared rather than the server's own, because the screen counts the
/// minutes left out of it: a bar with no number under it is a bar that
/// says "wait" and nothing else.
pub const CURE_SECONDS: f32 = 720.0;

/// What a raw thing on the frame turns into, and `None` for anything
/// that does not cure.
///
/// **The extension point.** Adding a second row is a line here, a line
/// in the block table, and nothing else: the rack, the save format, the
/// screen and the plugin hook are all written against "a thing that
/// cures into another thing".
///
/// The meat is the row that proves it. A haunch hung in the sun and the
/// wind keeps, the way a skin does -- same frame, same weather, same
/// rules about rain and frost and smoke. It is the hunter's answer to
/// the question the fire already answered better: cooked meat is still
/// the better meal, but drying costs no fuel and happens while you are
/// somewhere else.
pub fn cures_into(raw: BlockId) -> Option<BlockId> {
    match block_kind(raw) {
        BLOCK_HIDE => Some(BLOCK_LEATHER),
        // Every skin cures, and all of them cure into the same leather:
        // what a tanned hide *is* does not depend on what wore it, and
        // a pelt-leather and a bear-leather would be two words for one
        // material. What differs is how many skins an animal gives --
        // see `Species::butchering`.
        crate::types::BLOCK_PELT | crate::types::BLOCK_BEAR_HIDE => Some(BLOCK_LEATHER),
        BLOCK_RAW_MEAT => Some(BLOCK_DRIED_MEAT),
        // ...and every meat dries into the same dried meat, for the
        // same reason: a strip of dried venison and a strip of dried
        // hare are a strip of dried meat.
        crate::types::BLOCK_HARE_MEAT
        | crate::types::BLOCK_FOWL_MEAT
        | crate::types::BLOCK_BEAR_MEAT
        | crate::types::BLOCK_WOLF_MEAT => Some(BLOCK_DRIED_MEAT),
        // A sod of peat dries into fuel -- **on the ground now, and on no
        // rack** (`primitive_server`'s `peat`; `Trade::takes` refuses it on
        // both). The row stays for the sods a save left on a rack when the
        // rack was the only way: they finish where they are rather than
        // stopping half dried on a frame nobody can put them on any more.
        BLOCK_PEAT => Some(BLOCK_DRIED_PEAT),
        // ...and a frond of kelp dries into a strip that keeps, which is
        // what a shore with no trees on it has for a larder.
        crate::types::BLOCK_KELP_FROND => Some(crate::types::BLOCK_DRIED_KELP),
        // A fish dries into a dried fish and not into dried meat: it used to
        // have no row at all, because hanging a catch on the frame to come
        // off as dried *meat* would have been a lie in the pack.
        crate::types::BLOCK_RAW_FISH => Some(crate::types::BLOCK_DRIED_FISH),
        // **Salted, then dried**, which is how a larder that has to last a
        // winter is made: the salt draws the water the rack would otherwise
        // wait for the wind to take, and what comes off keeps longest of
        // anything (`food::rot_every`). Its own row rather than dried meat,
        // so the pack says which one lasts.
        crate::types::BLOCK_SALTED_MEAT => Some(crate::types::BLOCK_DRIED_SALTED_MEAT),
        crate::types::BLOCK_SALTED_FISH => Some(crate::types::BLOCK_DRIED_SALTED_FISH),
        _ => None,
    }
}

/// How long one of `raw` takes to cure in ideal weather, in seconds.
///
/// [`CURE_SECONDS`] for everything. A function rather than the constant
/// because grass had a minute of its own while it dried into hay for the pit
/// kiln; the kiln takes fibre now and nothing here is quicker than a skin.
pub fn cure_seconds(raw: BlockId) -> f32 {
    let _ = raw;
    CURE_SECONDS
}

/// What a column of a rack of two by two can be seen to carry, by the four
/// bits its two cells hold (`types::rack_column_goods`). Index 0 is a bare
/// ridge.
///
/// **A kind of goods, raw or cured, and not a count.** Four bits a column is
/// sixteen pictures and there are fourteen goods to show, so each is one
/// row here and a column hangs two pieces of it (`mesh::rack_column_block`).
/// A count would have been two of those bits, and then the rack could not
/// have shown the difference a player walks over to check -- whether it is
/// dry yet.
///
/// Every skin shows as a hide and every haunch as a strip of meat, for the
/// reason `cures_into` gives: they all cure into the same thing.
pub const HANGING: [Option<BlockId>; 15] = [
    None,
    Some(BLOCK_HIDE),
    Some(BLOCK_LEATHER),
    Some(BLOCK_RAW_MEAT),
    Some(BLOCK_DRIED_MEAT),
    Some(crate::types::BLOCK_RAW_FISH),
    Some(crate::types::BLOCK_DRIED_FISH),
    Some(crate::types::BLOCK_SALTED_MEAT),
    Some(crate::types::BLOCK_DRIED_SALTED_MEAT),
    Some(crate::types::BLOCK_SALTED_FISH),
    Some(crate::types::BLOCK_DRIED_SALTED_FISH),
    Some(BLOCK_PEAT),
    Some(BLOCK_DRIED_PEAT),
    Some(crate::types::BLOCK_KELP_FROND),
    Some(crate::types::BLOCK_DRIED_KELP),
];

/// Which row of [`HANGING`] shows this item on a ridge: 0 for anything a
/// rack neither takes nor makes.
pub fn hanging_index(item: BlockId) -> u8 {
    let shown = match block_kind(item) {
        crate::types::BLOCK_PELT | crate::types::BLOCK_BEAR_HIDE => BLOCK_HIDE,
        crate::types::BLOCK_HARE_MEAT
        | crate::types::BLOCK_FOWL_MEAT
        | crate::types::BLOCK_BEAR_MEAT
        | crate::types::BLOCK_WOLF_MEAT => BLOCK_RAW_MEAT,
        kind => kind,
    };
    HANGING.iter().position(|&row| row == Some(shown)).unwrap_or(0) as u8
}

/// What the two columns of a rack show, near and far, for what is in it.
///
/// **Raw at the near end and cured at the far one when there is both**, and
/// whichever there is along the whole ridge when there is one: a rack of
/// strips just hung is a ridge of red, and as the first of them come off
/// dry the far half turns dark -- which is the one thing a player looks at a
/// rack to find out, seen from across the camp.
pub fn columns_showing(contents: &Inventory) -> (u8, u8) {
    let raw = contents.block_in(HIDE_SLOT).filter(|&b| cures_into(b).is_some()).map_or(0, hanging_index);
    let cured = contents.block_in(LEATHER_SLOT).map_or(0, hanging_index);
    match (raw, cured) {
        (0, cured) => (cured, cured),
        (raw, 0) => (raw, raw),
        both => both,
    }
}

/// Whether a block is a rack at all: the rack of two by two or the hide
/// frame. Both are the same container with the same two slots and the same
/// weather; what differs is what each takes ([`Trade`]).
#[inline]
pub fn is_rack(block: BlockId) -> bool {
    matches!(block_kind(block), BLOCK_DRYING_RACK | BLOCK_HIDE_FRAME)
}

/// **What a rack is for**: two racks, two jobs.
///
/// "сделай эту сушилку только для мяса и рыбы, и верни старую сушилку (раму
/// для шкуры)". The split is by how a thing dries, which is also how it is
/// seen to dry: **a skin is stretched, and food is hung.**
///
/// * [`Trade::Larder`], the rack of two by two: every meat raw or salted,
///   every fish raw or salted -- and **kelp**, which is food, dries into a
///   strip that keeps (`BLOCK_DRIED_KELP`), and is hung over a line in
///   fronds the way a fish is, not laced flat. A shore with no trees on it
///   dries its kelp where it dries its catch.
/// * [`Trade::Skins`], the hide frame: the hide, the pelt, the bear's hide.
///   Nothing else in the world is stretched to cure -- wool is shorn, not
///   cured, and fibre dries in a pit kiln's packing, not on a frame -- so the
///   frame's list is the skins, and nothing is added to it for symmetry.
///
/// **Peat is on neither.** It dries lying on the ground in the open
/// (`primitive_server`'s `peat`); a rack full of turf was a rack the larder
/// could not use, and the bog's fuel wanted a way that cost nothing to build.
///
/// **What is already on the wrong rack stays and finishes.** A save from
/// before the split can have a skin on the big rack or a haunch on a lone
/// frame; the refusal is at the gesture ([`accepts_on`]), and the drying goes
/// on by [`cures_into`], which asks what a thing becomes and not where it
/// is. Rejected: throwing it off at load, which spills a player's twelve
/// minutes onto the grass for a rule they never broke, or stopping it, which
/// is the same loss slower.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Trade {
    /// The rack of two by two: meat, fish and kelp, hung.
    Larder,
    /// The hide frame: skins, stretched.
    Skins,
}

impl Trade {
    /// Which trade this rack block plies, and `None` for anything that is
    /// not a rack.
    ///
    /// A lone cell of the big rack is a frame in all but id, and it becomes
    /// one when its world is read; one the reader has not reached still has
    /// the big rack's kind and answers `Larder`, the id's own answer -- which
    /// changes nothing already on it (see above).
    pub fn of(rack: BlockId) -> Option<Trade> {
        match block_kind(rack) {
            BLOCK_DRYING_RACK => Some(Trade::Larder),
            BLOCK_HIDE_FRAME => Some(Trade::Skins),
            _ => None,
        }
    }

    /// Does this rack take `item` onto its frame?
    ///
    /// Every row of [`cures_into`] but peat, split in two by how it dries.
    pub fn takes(self, item: BlockId) -> bool {
        use crate::types::{
            BLOCK_BEAR_HIDE, BLOCK_BEAR_MEAT, BLOCK_FOWL_MEAT, BLOCK_HARE_MEAT, BLOCK_KELP_FROND, BLOCK_PELT,
            BLOCK_RAW_FISH, BLOCK_SALTED_FISH, BLOCK_SALTED_MEAT, BLOCK_WOLF_MEAT,
        };
        let kind = block_kind(item);
        match self {
            Trade::Skins => matches!(kind, BLOCK_HIDE | BLOCK_PELT | BLOCK_BEAR_HIDE),
            Trade::Larder => matches!(
                kind,
                BLOCK_RAW_MEAT
                    | BLOCK_HARE_MEAT
                    | BLOCK_FOWL_MEAT
                    | BLOCK_BEAR_MEAT
                    | BLOCK_WOLF_MEAT
                    | BLOCK_SALTED_MEAT
                    | BLOCK_RAW_FISH
                    | BLOCK_SALTED_FISH
                    | BLOCK_KELP_FROND
            ),
        }
    }

    /// The other rack: where a thing this one refused does go, if anywhere.
    pub fn other(self) -> Trade {
        match self {
            Trade::Larder => Trade::Skins,
            Trade::Skins => Trade::Larder,
        }
    }
}

/// May this block go in this slot of a rack plying `trade`?
///
/// [`accepts`] and the trade's own list: the frame for what this rack dries,
/// and nothing in the tray. The server asks this one; see
/// [`refused_for_its_trade`] for when a refusal comes back with words.
pub fn accepts_on(trade: Trade, slot: usize, block: BlockId) -> bool {
    accepts(slot, block) && trade.takes(block)
}

/// Was `block` refused by a rack plying `trade` **only because it is the
/// other rack's work** -- a skin at the larder, a fish at the frame?
///
/// That refusal is the one worth a word: a stone on a rack is obviously
/// wrong, and a hide that will not go on the new rack reads as a broken rack
/// unless something says where it does go (`ServerMessage::RackRefused`).
pub fn refused_for_its_trade(trade: Trade, block: BlockId) -> bool {
    !trade.takes(block) && trade.other().takes(block)
}

/// May this block go in this slot of *some* rack?
///
/// Two rules and both of them are about not wasting the player's time: a
/// thing that cures is the only thing a frame does anything with, and the
/// far slot is where the rack *puts* things. Which rack is [`accepts_on`]:
/// this is the half both racks share, and what the screen asks to decide
/// what may be dragged at all -- the refusal by trade is the server's, and
/// comes back with words (`ServerMessage::RackRefused`).
pub fn accepts(slot: usize, block: BlockId) -> bool {
    slot == HIDE_SLOT && cures_into(block).is_some()
}

/// What the frame is curing, if it is curing anything.
///
/// `None` for an empty rack, for one loaded with something that is not a
/// skin, and for one whose output slot is too full to take what the skin
/// would become -- the last of which is not an error anywhere: a rack
/// with a full tray simply stops, and starts again when somebody empties
/// it. Exactly the rule [`crate::hearth::next_recipe`] follows.
pub fn curing(contents: &Inventory) -> Option<BlockId> {
    let raw = contents.block_in(HIDE_SLOT)?;
    let cured = cures_into(raw)?;
    let mut trial = contents.clone();
    trial.take_from(HIDE_SLOT, 1);
    (trial.add_within(LEATHER_SLOT..LEATHER_SLOT + 1, cured, 1) == 0).then_some(cured)
}

/// Takes one cured skin off the frame and puts the leather in the tray.
///
/// Answers `false` and changes nothing if the skin is no longer there or
/// the tray no longer has room, which a player at the screen can arrange
/// between the tick that finished it and the tick that banks it.
pub fn complete(contents: &mut Inventory) -> bool {
    let Some(cured) = curing(contents) else {
        return false;
    };
    if contents.take_from(HIDE_SLOT, 1) == 0 {
        return false;
    }
    contents.add_within(LEATHER_SLOT..LEATHER_SLOT + 1, cured, 1);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory::Stack;
    use crate::types::BLOCK_STONE;

    fn loaded(hides: u32) -> Inventory {
        let mut contents = Inventory::new();
        if hides > 0 {
            contents.put_in_slot(HIDE_SLOT, Stack::new(BLOCK_HIDE, hides));
        }
        contents
    }

    #[test]
    fn a_haunch_dries_by_the_same_rules_as_a_skin() {
        // The extension point, used: raw meat goes on the frame, dried
        // meat comes off it, and every rule in this file -- one at a
        // time, a full tray stops it -- applies without a word of new
        // code.
        use crate::types::{BLOCK_DRIED_MEAT, BLOCK_RAW_MEAT};
        assert_eq!(cures_into(BLOCK_RAW_MEAT), Some(BLOCK_DRIED_MEAT));
        assert!(accepts(HIDE_SLOT, BLOCK_RAW_MEAT));
        assert!(!accepts(LEATHER_SLOT, BLOCK_DRIED_MEAT));

        let mut contents = Inventory::new();
        contents.put_in_slot(HIDE_SLOT, Stack::new(BLOCK_RAW_MEAT, 2));
        assert_eq!(curing(&contents), Some(BLOCK_DRIED_MEAT));
        assert!(complete(&mut contents));
        assert_eq!(contents.count_in(HIDE_SLOT), 1);
        assert_eq!(contents.count(BLOCK_DRIED_MEAT), 1);
    }

    #[test]
    fn nothing_but_a_skin_goes_on_a_frame() {
        assert!(accepts(HIDE_SLOT, BLOCK_HIDE));
        assert!(!accepts(HIDE_SLOT, BLOCK_STONE));
        // The tray is the rack's, not the player's: filling it by hand
        // is how a rack gets jammed with something it will never move.
        assert!(!accepts(LEATHER_SLOT, BLOCK_LEATHER));
        assert!(!accepts(LEATHER_SLOT, BLOCK_HIDE));
    }

    #[test]
    fn a_skin_cures_into_leather_one_at_a_time() {
        let mut contents = loaded(3);
        assert_eq!(curing(&contents), Some(BLOCK_LEATHER));
        assert!(complete(&mut contents));
        assert_eq!(contents.count_in(HIDE_SLOT), 2, "it took more than one skin");
        assert_eq!(contents.count_in(LEATHER_SLOT), 1);
    }

    #[test]
    fn an_empty_frame_is_curing_nothing() {
        assert_eq!(curing(&loaded(0)), None);
        assert!(!complete(&mut loaded(0)));
    }

    #[test]
    fn the_meat_rack_refuses_a_hide_and_the_frame_refuses_a_fish() {
        use crate::types::{BLOCK_BEAR_HIDE, BLOCK_KELP_FROND, BLOCK_PELT, BLOCK_RAW_FISH, BLOCK_SALTED_MEAT};
        let larder = Trade::of(BLOCK_DRYING_RACK).expect("the big rack plies a trade");
        let frame = Trade::of(BLOCK_HIDE_FRAME).expect("the frame plies a trade");
        assert_eq!((larder, frame), (Trade::Larder, Trade::Skins));
        for skin in [BLOCK_HIDE, BLOCK_PELT, BLOCK_BEAR_HIDE] {
            assert!(!accepts_on(larder, HIDE_SLOT, skin), "the meat rack took a skin ({skin})");
            assert!(accepts_on(frame, HIDE_SLOT, skin), "the frame refused a skin ({skin})");
            assert!(refused_for_its_trade(larder, skin), "the larder said nothing about where {skin} goes");
        }
        for food in [BLOCK_RAW_FISH, BLOCK_RAW_MEAT, BLOCK_SALTED_MEAT, BLOCK_KELP_FROND] {
            assert!(!accepts_on(frame, HIDE_SLOT, food), "the frame took food ({food})");
            assert!(accepts_on(larder, HIDE_SLOT, food), "the larder refused food ({food})");
            assert!(refused_for_its_trade(frame, food));
        }
        // ...and a stone is nobody's work, which is not the refusal with words.
        assert!(!refused_for_its_trade(frame, BLOCK_STONE));
    }

    #[test]
    fn peat_goes_on_neither_rack_but_a_sod_left_on_one_still_finishes() {
        // Peat dries on the ground now. What a save left on a rack is not
        // stranded: the drying asks `cures_into`, which still knows it.
        for trade in [Trade::Larder, Trade::Skins] {
            assert!(!accepts_on(trade, HIDE_SLOT, BLOCK_PEAT), "{trade:?} took peat");
        }
        let mut contents = Inventory::new();
        contents.put_in_slot(HIDE_SLOT, Stack::new(BLOCK_PEAT, 1));
        assert_eq!(curing(&contents), Some(BLOCK_DRIED_PEAT));
    }

    #[test]
    fn everything_that_cures_but_peat_has_exactly_one_rack() {
        // The split must not strand a row of `cures_into`: a thing that
        // cures and that neither rack takes is a thing nobody can make.
        for &(id, _) in crate::types::ALL_BLOCK_IDS {
            if cures_into(id).is_none() || block_kind(id) == BLOCK_PEAT {
                continue;
            }
            let racks = [Trade::Larder, Trade::Skins].iter().filter(|trade| trade.takes(id)).count();
            assert_eq!(racks, 1, "{id} cures, and {racks} racks take it");
        }
    }

    #[test]
    fn a_full_tray_stops_it_rather_than_losing_the_leather() {
        // The rule that makes a rack safe to leave: come back to a full
        // tray and the skins are still skins, waiting.
        let mut contents = loaded(2);
        contents.put_in_slot(
            LEATHER_SLOT,
            Stack::new(BLOCK_LEATHER, crate::inventory::MAX_STACK),
        );
        assert_eq!(curing(&contents), None);
        assert!(!complete(&mut contents));
        assert_eq!(contents.count_in(HIDE_SLOT), 2, "a skin went nowhere");
    }
}
