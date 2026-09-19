//! **A barter stall**: goods left out with a price on them, for whoever
//! comes by while their owner is away.
//!
//! On a server the two people who want to trade are rarely online at once.
//! A stall is how one of them trades anyway: "four flint for a hide", with
//! the flint on the counter and the hide waiting in the till when the owner
//! comes home. What it decides for the owner is *how much to put out and
//! where* -- a stall is not a lock (see the server's `stall_broken`), so a
//! stall stocked with a winter's copper by the road is a bet on the road.
//!
//! ## Where the goods are
//!
//! In the container store, like a chest's: the counter is squares
//! [`STOCK`] and the till is squares [`TAKINGS`] of one inventory at the
//! stall's cell. That is the whole of why a stall saves, spills, closes
//! and shows itself the way a chest does without a line of that written
//! twice. What is *not* a chest's is who owns it and what it asks, which
//! is the server's (`logic::stalls`), and the rule of a trade, which is
//! here so that the rule can be tested on two inventories.
//!
//! Rejected: **stock and takings as counts on the offer** ("twelve flint
//! left"). A count cannot carry a worn tool or a jug of grain -- both live
//! in the stack's own `damage` -- so an axe sold off a stall would arrive
//! new, and a counter that makes things better is a counter people would
//! use as a repair bench.

use serde::{Deserialize, Serialize};

use crate::inventory::Inventory;
use crate::types::{block_kind, is_known_block, BlockId, BLOCK_AIR};

/// The counter: what the owner has put out to sell.
pub const STOCK: std::ops::Range<usize> = 0..10;
/// The till: what buyers have paid, waiting for the owner.
///
/// **As small as the counter, on purpose.** A full till refuses the next
/// trade ([`Refusal::TillFull`]), and that is the decision it puts to the
/// owner: a stall left for a week sells until the till is full and then
/// stops, so a busy stall has to be visited. A till as big as a chest
/// would be a stall that never needs its owner, which is a shop with
/// nobody in it -- not a trade.
pub const TAKINGS: std::ops::Range<usize> = 10..20;
/// How many offers a stall carries.
///
/// Three: enough for "flint for hides, flint for meat, an axe for copper",
/// and few enough to read at a glance on a phone. It is also the number of
/// rows the screen has room for above the pack without shrinking it.
pub const OFFERS: usize = 3;
/// The most of either side one lot may be.
///
/// Half a stack. More would be a price nobody could carry in one trip, and
/// a lot is what one click moves.
pub const MAX_LOT: u32 = 64;

/// "`give_count` of `give` for `take_count` of `take`": one lot.
///
/// **Kinds, not exact ids**, the way a recipe names its ingredients
/// (`Inventory::count`): a price of bread is paid in bread whether it was
/// baked this morning or yesterday, and a stall that refused yesterday's
/// loaf would be a stall nobody could work out how to pay.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Offer {
    pub give: BlockId,
    pub give_count: u32,
    pub take: BlockId,
    pub take_count: u32,
}

impl Offer {
    /// Could this be a real price at all? Checked on the server for every
    /// offer a client names and for every one read back off a disk.
    pub fn is_valid(&self) -> bool {
        let real = |block: BlockId| block_kind(block) != BLOCK_AIR && is_known_block(block);
        real(self.give)
            && real(self.take)
            && (1..=MAX_LOT).contains(&self.give_count)
            && (1..=MAX_LOT).contains(&self.take_count)
    }
}

/// Why a trade or a change to a stall was refused.
///
/// **A reason rather than a sentence**, for `ServerMessage::RackRefused`'s
/// reason: the words belong to the reader's language, and `Error` carries
/// English.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Refusal {
    /// Only the owner may set a price, stock the counter or empty the till.
    NotYours,
    /// There is no offer in that row.
    NoOffer,
    /// **The offer changed between the look and the click.** The buyer names
    /// the offer they saw (`ClientMessage::StallBuy`), and one that has been
    /// repriced since is refused rather than taken at the new price -- or an
    /// owner could watch a buyer reach for "four flint for a hide" and make
    /// it "one flint for a hide" under their hand.
    OfferChanged,
    /// Not a whole lot of it left on the counter.
    SoldOut,
    /// The buyer is not carrying the price.
    CannotPay,
    /// The buyer's pack has no room for what they would get.
    NoRoom,
    /// The till has no room for the price. See [`TAKINGS`].
    TillFull,
    /// Not a price the game can have: an unknown thing, or a count of none.
    BadOffer,
}

/// May `block` be put into `slot` of a stall by hand?
///
/// The counter only. **Nothing is put into the till by hand**: it is where
/// the stall puts the price, and a till the owner could fill themselves is
/// a till full of stones that refuses every buyer (`Refusal::TillFull`).
pub fn accepts(slot: usize, _block: BlockId) -> bool {
    STOCK.contains(&slot)
}

/// How many whole lots of `offer` the counter holds.
pub fn lots_in_stock(store: &Inventory, offer: &Offer) -> u32 {
    store.count_within(STOCK, offer.give) / offer.give_count.max(1)
}

/// Can this pack pay for one lot?
pub fn can_pay(pack: &Inventory, offer: &Offer) -> bool {
    pack.count(offer.take) >= offer.take_count
}

/// One lot of `offer`: the goods off the counter into `pack`, the price out
/// of `pack` into the till -- **both or neither**.
///
/// Worked on copies and written back only when every step has succeeded,
/// so no order of failures can leave a count missing: the rule every
/// inventory move in this game keeps ("a stack is never in neither"). Each
/// stack goes as the thing it is, wear and contents included (see the
/// module note's rejected alternative).
///
/// Nothing here knows who is buying or whether they are allowed to; that is
/// the server's, which calls this under the locks that make two buyers one
/// after the other (see the server's `stall_buy`).
pub fn trade(offer: &Offer, store: &mut Inventory, pack: &mut Inventory) -> Result<(), Refusal> {
    if !offer.is_valid() {
        return Err(Refusal::BadOffer);
    }
    if lots_in_stock(store, offer) == 0 {
        return Err(Refusal::SoldOut);
    }
    if !can_pay(pack, offer) {
        return Err(Refusal::CannotPay);
    }
    let mut new_store = store.clone();
    let mut new_pack = pack.clone();
    let whole_pack = 0..new_pack.slots().len();
    // The price first: a buyer paying with the last of something frees the
    // square the goods then land in, which is the order a person would
    // hand things over in.
    if !move_units(&mut new_pack, whole_pack.clone(), &mut new_store, TAKINGS, offer.take, offer.take_count) {
        return Err(Refusal::TillFull);
    }
    if !move_units(&mut new_store, STOCK, &mut new_pack, whole_pack, offer.give, offer.give_count) {
        return Err(Refusal::NoRoom);
    }
    *store = new_store;
    *pack = new_pack;
    Ok(())
}

/// Moves `count` of `kind` out of `from`'s squares in `from_range` into
/// `to`'s in `to_range`, a stack at a time and each as itself. False if
/// there were not that many or they did not all fit -- in which case both
/// are left half-done, which is why [`trade`] only ever hands this copies.
fn move_units(
    from: &mut Inventory,
    from_range: std::ops::Range<usize>,
    to: &mut Inventory,
    to_range: std::ops::Range<usize>,
    kind: BlockId,
    count: u32,
) -> bool {
    let kind = block_kind(kind);
    let mut left = count;
    let end = from_range.end.min(from.slots().len());
    for slot in from_range.start..end {
        if left == 0 {
            break;
        }
        let Some(stack) = from.slots()[slot] else {
            continue;
        };
        if block_kind(stack.block) != kind {
            continue;
        }
        let moving = stack.count.min(left);
        if to.add_worn_within(to_range.clone(), stack.block, moving, stack.damage) != 0 {
            return false;
        }
        from.take_from(slot, moving);
        left -= moving;
    }
    left == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory::Stack;
    use crate::types::{BLOCK_COBBLESTONE, BLOCK_FLINT, BLOCK_HIDE, BLOCK_STICK};

    const FLINT_FOR_HIDE: Offer = Offer { give: BLOCK_FLINT, give_count: 4, take: BLOCK_HIDE, take_count: 1 };

    fn stall_with(flint: u32) -> Inventory {
        let mut store = Inventory::chest();
        store.add_within(STOCK, BLOCK_FLINT, flint);
        store
    }

    fn pack_with(block: BlockId, count: u32) -> Inventory {
        let mut pack = Inventory::new();
        pack.add(block, count);
        pack
    }

    #[test]
    fn an_offer_taken_moves_exactly_the_goods_both_ways() {
        let mut store = stall_with(10);
        let mut pack = pack_with(BLOCK_HIDE, 3);
        assert_eq!(trade(&FLINT_FOR_HIDE, &mut store, &mut pack), Ok(()));
        assert_eq!(pack.count(BLOCK_FLINT), 4, "the buyer did not get the lot");
        assert_eq!(pack.count(BLOCK_HIDE), 2, "the buyer did not pay exactly the price");
        assert_eq!(store.count_within(STOCK, BLOCK_FLINT), 6, "the counter did not give up exactly the lot");
        assert_eq!(store.count_within(TAKINGS, BLOCK_HIDE), 1, "the till did not get exactly the price");
        // And nothing was made or lost anywhere else.
        assert_eq!(store.count(BLOCK_FLINT) + pack.count(BLOCK_FLINT), 10);
        assert_eq!(store.count(BLOCK_HIDE) + pack.count(BLOCK_HIDE), 3);
    }

    #[test]
    fn a_buyer_without_enough_of_the_price_gets_nothing_and_keeps_what_they_had() {
        let mut store = stall_with(10);
        let offer = Offer { take_count: 2, ..FLINT_FOR_HIDE };
        let mut pack = pack_with(BLOCK_HIDE, 1);
        let (store_before, pack_before) = (store.clone(), pack.clone());
        assert_eq!(trade(&offer, &mut store, &mut pack), Err(Refusal::CannotPay));
        assert_eq!(store.slots(), store_before.slots(), "a refused trade touched the stall");
        assert_eq!(pack.slots(), pack_before.slots(), "a refused trade touched the pack");
    }

    #[test]
    fn a_counter_short_of_a_whole_lot_sells_nothing() {
        let mut store = stall_with(3);
        let mut pack = pack_with(BLOCK_HIDE, 1);
        assert_eq!(trade(&FLINT_FOR_HIDE, &mut store, &mut pack), Err(Refusal::SoldOut));
        assert_eq!(pack.count(BLOCK_HIDE), 1);
        assert_eq!(store.count(BLOCK_FLINT), 3);
    }

    #[test]
    fn a_full_till_refuses_the_trade_whole() {
        let mut store = stall_with(10);
        for slot in TAKINGS {
            store.put_in_slot(slot, Stack::new(BLOCK_COBBLESTONE, crate::inventory::MAX_STACK));
        }
        let mut pack = pack_with(BLOCK_HIDE, 1);
        let (store_before, pack_before) = (store.clone(), pack.clone());
        assert_eq!(trade(&FLINT_FOR_HIDE, &mut store, &mut pack), Err(Refusal::TillFull));
        assert_eq!(store.slots(), store_before.slots());
        assert_eq!(pack.slots(), pack_before.slots());
    }

    #[test]
    fn a_pack_with_no_room_for_the_goods_pays_nothing() {
        let mut store = stall_with(10);
        // Every square full of sticks but one holding two hides: paying one
        // leaves the other in its square, so nothing is freed for the flint.
        let mut pack = Inventory::new();
        for slot in 0..pack.slots().len() {
            pack.put_in_slot(slot, Stack::new(BLOCK_STICK, crate::types::stack_limit(BLOCK_STICK)));
        }
        pack.take_slot(0);
        pack.put_in_slot(0, Stack::new(BLOCK_HIDE, 2));
        let offer = Offer { take_count: 1, ..FLINT_FOR_HIDE };
        let (store_before, pack_before) = (store.clone(), pack.clone());
        assert_eq!(trade(&offer, &mut store, &mut pack), Err(Refusal::NoRoom));
        assert_eq!(store.slots(), store_before.slots(), "the till kept a price for goods never handed over");
        assert_eq!(pack.slots(), pack_before.slots());
    }

    #[test]
    fn a_worn_tool_sold_off_a_stall_arrives_as_worn_as_it_was() {
        use crate::types::BLOCK_STONE_AXE;
        let mut store = Inventory::chest();
        store.put_in_slot(STOCK.start, Stack::worn(BLOCK_STONE_AXE, 1, 17));
        let offer = Offer { give: BLOCK_STONE_AXE, give_count: 1, take: BLOCK_HIDE, take_count: 1 };
        let mut pack = pack_with(BLOCK_HIDE, 1);
        assert_eq!(trade(&offer, &mut store, &mut pack), Ok(()));
        let axe = pack.slots().iter().flatten().find(|s| s.block == BLOCK_STONE_AXE).expect("no axe");
        assert_eq!(axe.damage, 17, "the stall mended the axe");
    }

    #[test]
    fn only_the_counter_takes_things_by_hand() {
        assert!(STOCK.clone().all(|slot| accepts(slot, BLOCK_FLINT)));
        assert!(!TAKINGS.clone().any(|slot| accepts(slot, BLOCK_FLINT)), "the till can be filled by hand");
        assert!(!accepts(TAKINGS.end, BLOCK_FLINT));
    }

    #[test]
    fn a_price_of_nothing_or_of_no_known_thing_is_not_an_offer() {
        assert!(FLINT_FOR_HIDE.is_valid());
        assert!(!Offer { take_count: 0, ..FLINT_FOR_HIDE }.is_valid());
        assert!(!Offer { give_count: MAX_LOT + 1, ..FLINT_FOR_HIDE }.is_valid());
        assert!(!Offer { take: BLOCK_AIR, ..FLINT_FOR_HIDE }.is_valid());
        assert!(!Offer { give: 0x3ff, ..FLINT_FOR_HIDE }.is_valid());
    }
}
