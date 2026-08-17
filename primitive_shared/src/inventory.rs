//! What a player is carrying.
//!
//! ## Why this is shared, and server-owned
//!
//! It used to live on the client, which meant every consequence of
//! carrying something had to be smuggled back across the wire: the
//! server needed the weight for fall damage, so the client *told* it,
//! and there was nothing to check that number against. It also meant the
//! client had to guess at its own inventory ahead of confirmation, with
//! a whole machine of pending edits and refunds to unwind the guesses
//! that turned out wrong.
//!
//! Now the server owns it and the client is sent the result. The pending
//! machinery is gone entirely -- there is nothing to reconcile, because
//! the client never changes anything itself. Weight is computed where it
//! is used. The type lives here because both sides need to read it: the
//! server to decide, the client to draw.

use serde::{Deserialize, Serialize};

use crate::types::{block_weight, BlockId};

/// Hotbar slots. Ten, because that is how far the number row reaches.
pub const HOTBAR_SLOTS: usize = 10;
/// Rows of storage behind the hotbar, reachable from the inventory
/// screen.
pub const STORAGE_ROWS: usize = 3;
/// Every slot a player has. The hotbar is the first `HOTBAR_SLOTS` of
/// them, so a slot index means the same thing everywhere.
pub const SLOTS: usize = HOTBAR_SLOTS * (1 + STORAGE_ROWS);

/// How many of one block fit in one slot.
///
/// A real constraint rather than a display one: a full stack of stone is
/// over three hundred kilograms, and that is what makes the inventory a
/// series of decisions instead of a bucket.
pub const MAX_STACK: u32 = 128;

/// One slot's contents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stack {
    pub block: BlockId,
    pub count: u32,
}

impl Stack {
    pub fn new(block: BlockId, count: u32) -> Self {
        Self { block, count }
    }

    pub fn weight(&self) -> f32 {
        block_weight(self.block) * self.count as f32
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Inventory {
    /// Every slot, hotbar first. A slot is claimed by the first block of
    /// its kind picked up and released when the last one is spent.
    slots: Vec<Option<Stack>>,
}

impl Default for Inventory {
    fn default() -> Self {
        Self::new()
    }
}

impl Inventory {
    pub fn new() -> Self {
        Self {
            slots: vec![None; SLOTS],
        }
    }

    /// Repairs an inventory that arrived over the wire, or out of a save
    /// written by an older build.
    ///
    /// Slot counts change between versions and stack limits get retuned,
    /// so the shape of what arrives is not this type's to assume.
    pub fn sanitize(&mut self) {
        self.slots.resize(SLOTS, None);
        for slot in &mut self.slots {
            match slot {
                Some(stack) if stack.count == 0 => *slot = None,
                Some(stack) => stack.count = stack.count.min(MAX_STACK),
                None => {}
            }
        }
    }

    pub fn slots(&self) -> &[Option<Stack>] {
        &self.slots
    }

    pub fn block_in(&self, slot: usize) -> Option<BlockId> {
        self.slots.get(slot).copied().flatten().map(|s| s.block)
    }

    pub fn count_in(&self, slot: usize) -> u32 {
        self.slots
            .get(slot)
            .copied()
            .flatten()
            .map(|s| s.count)
            .unwrap_or(0)
    }

    /// How many of one kind of block are carried, across every slot.
    pub fn count(&self, block: BlockId) -> u32 {
        self.slots
            .iter()
            .flatten()
            .filter(|s| s.block == block)
            .map(|s| s.count)
            .sum()
    }

    pub fn total_items(&self) -> u32 {
        self.slots.iter().flatten().map(|s| s.count).sum()
    }

    /// What the whole load weighs, in kilograms.
    pub fn total_weight(&self) -> f32 {
        self.slots.iter().flatten().map(|s| s.weight()).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.total_items() == 0
    }

    /// Swaps two slots. Out-of-range indices are ignored: these arrive
    /// from clicks on a grid, and from the network.
    pub fn swap(&mut self, a: usize, b: usize) {
        if a < self.slots.len() && b < self.slots.len() && a != b {
            self.slots.swap(a, b);
        }
    }

    /// Moves the stack in `from` onto `to`, merging rather than swapping
    /// when the two hold the same block.
    ///
    /// A plain swap is the wrong answer for the commonest gesture in the
    /// screen: two part-stacks of stone dragged together are meant to
    /// become one, and swapping them leaves the player doing it again
    /// with the same result. Whatever does not fit stays behind, so a
    /// merge into a nearly full stack tops it up instead of refusing.
    ///
    /// Returns whether anything actually moved.
    pub fn move_or_merge(&mut self, from: usize, to: usize) -> bool {
        if from == to || from >= self.slots.len() || to >= self.slots.len() {
            return false;
        }
        let Some(source) = self.slots[from] else {
            return false;
        };
        match self.slots[to] {
            Some(target) if target.block == source.block => {
                let moved = (MAX_STACK - target.count.min(MAX_STACK)).min(source.count);
                if moved == 0 {
                    return false;
                }
                self.slots[to] = Some(Stack::new(target.block, target.count + moved));
                self.take_from(from, moved);
                true
            }
            _ => {
                self.slots.swap(from, to);
                true
            }
        }
    }

    /// Splits `from` in two, putting the larger half in `to`.
    ///
    /// `to` has to be empty or hold the same block; anything else would
    /// have to be a swap, and a gesture that sometimes splits and
    /// sometimes swaps is one the player cannot aim.
    ///
    /// Returns whether anything moved.
    pub fn split_into(&mut self, from: usize, to: usize) -> bool {
        if from == to || from >= self.slots.len() || to >= self.slots.len() {
            return false;
        }
        let Some(source) = self.slots[from] else {
            return false;
        };
        let room = match self.slots[to] {
            None => MAX_STACK,
            Some(target) if target.block == source.block => {
                MAX_STACK - target.count.min(MAX_STACK)
            }
            Some(_) => return false,
        };
        // Rounded up, so splitting a single block moves it rather than
        // doing nothing at all.
        let moved = source.count.div_ceil(2).min(room);
        if moved == 0 {
            return false;
        }
        let taken = self.take_from(from, moved);
        match &mut self.slots[to] {
            Some(target) => target.count += taken,
            slot @ None => *slot = Some(Stack::new(source.block, taken)),
        }
        true
    }

    /// Sends a stack the other way between the hotbar and storage.
    ///
    /// The shift-click: from the bar it goes to the pile behind it, from
    /// the pile it comes to the bar. Merged into a part-stack of the same
    /// block where there is one, so shift-clicking a handful of dirt onto
    /// a bar that already has dirt tops that up rather than claiming a
    /// second slot for it.
    ///
    /// Returns whether anything moved.
    pub fn quick_move(&mut self, slot: usize) -> bool {
        if slot >= self.slots.len() {
            return false;
        }
        let Some(source) = self.slots[slot] else {
            return false;
        };
        let mut target_range = if slot < HOTBAR_SLOTS {
            HOTBAR_SLOTS..self.slots.len()
        } else {
            0..HOTBAR_SLOTS.min(self.slots.len())
        };

        // Part-stacks of the same block first, then an empty slot -- the
        // same order `add` uses, for the same reason.
        let mut left = source.count;
        for index in target_range.clone() {
            if left == 0 {
                break;
            }
            let Some(target) = self.slots[index] else {
                continue;
            };
            if target.block != source.block || target.count >= MAX_STACK {
                continue;
            }
            let moved = (MAX_STACK - target.count).min(left);
            self.slots[index] = Some(Stack::new(target.block, target.count + moved));
            left -= moved;
        }
        if left > 0 {
            if let Some(empty) = target_range.find(|&i| self.slots[i].is_none()) {
                self.slots[empty] = Some(Stack::new(source.block, left));
                left = 0;
            }
        }

        let moved = source.count - left;
        if moved == 0 {
            return false;
        }
        self.take_from(slot, moved);
        true
    }

    /// Takes a whole slot out, leaving it empty.
    ///
    /// The half of a cross-inventory move that reads: what comes back is
    /// the caller's to put somewhere, and if it cannot, to put back.
    pub fn take_slot(&mut self, slot: usize) -> Option<Stack> {
        self.slots.get_mut(slot).and_then(|s| s.take())
    }

    /// Puts a stack into one specific slot, merging with what is there.
    ///
    /// Returns what would not fit -- which the caller must put back
    /// somewhere, or it is gone. That is why this returns the remainder
    /// rather than a `bool`: a move between two inventories has a moment
    /// where the stack is in neither, and every path out of that moment
    /// has to end with the whole of it somewhere.
    pub fn put_in_slot(&mut self, slot: usize, stack: Stack) -> Option<Stack> {
        let Some(target) = self.slots.get_mut(slot) else {
            return Some(stack);
        };
        match target {
            None => {
                let fits = stack.count.min(MAX_STACK);
                *target = Some(Stack::new(stack.block, fits));
                (stack.count > fits).then(|| Stack::new(stack.block, stack.count - fits))
            }
            Some(held) if held.block == stack.block => {
                let room = MAX_STACK.saturating_sub(held.count);
                let moved = room.min(stack.count);
                held.count += moved;
                (moved < stack.count).then(|| Stack::new(stack.block, stack.count - moved))
            }
            // A different block: the caller wanted a swap, and a swap is
            // not this function's to decide -- see `move_between`.
            Some(_) => Some(stack),
        }
    }

    /// Tidies the storage rows: one kind of block per run of slots,
    /// part-stacks folded together, empties pushed to the end.
    ///
    /// The hotbar is deliberately left alone. Where things sit on the bar
    /// is an arrangement the player made and relies on mid-fight;
    /// storage is a pile. Sorting the pile is a convenience, sorting the
    /// bar is taking something away.
    ///
    /// Returns whether anything moved.
    pub fn sort_storage(&mut self) -> bool {
        if self.slots.len() <= HOTBAR_SLOTS {
            return false;
        }
        let before: Vec<Option<Stack>> = self.slots[HOTBAR_SLOTS..].to_vec();

        // One total per block, laid back out in block order, so the same
        // pack always tidies to the same place.
        let mut totals: Vec<(BlockId, u32)> = Vec::new();
        for stack in before.iter().flatten() {
            match totals.iter_mut().find(|(block, _)| *block == stack.block) {
                Some((_, count)) => *count += stack.count,
                None => totals.push((stack.block, stack.count)),
            }
        }
        totals.sort_unstable_by_key(|&(block, _)| block);

        let mut index = HOTBAR_SLOTS;
        let mut left_over = 0u32;
        for (block, mut count) in totals {
            while count > 0 {
                if index >= self.slots.len() {
                    left_over += count;
                    break;
                }
                let moved = count.min(MAX_STACK);
                self.slots[index] = Some(Stack::new(block, moved));
                count -= moved;
                index += 1;
            }
        }

        if left_over > 0 {
            // Cannot happen -- folding part-stacks never needs more
            // slots than it started with -- but the alternative to
            // checking is deleting whatever did not fit, and that is not
            // a bug worth risking for a comparison.
            self.slots[HOTBAR_SLOTS..].clone_from_slice(&before);
            return false;
        }
        for slot in &mut self.slots[index..] {
            *slot = None;
        }
        self.slots[HOTBAR_SLOTS..] != before[..]
    }

    /// Puts blocks in, topping up part-filled stacks before claiming a
    /// new slot.
    ///
    /// Returns however many did **not** fit. Callers that can put the
    /// remainder back in the world -- a pickup that overflows, say --
    /// need to know, and silently swallowing it is how items vanish.
    pub fn add(&mut self, block: BlockId, amount: u32) -> u32 {
        let mut left = amount;

        // Top up existing stacks first. Filling a new slot while an old
        // one sits one short is how an inventory ends up looking full
        // while holding almost nothing.
        for stack in self.slots.iter_mut().flatten() {
            if left == 0 {
                return 0;
            }
            if stack.block != block || stack.count >= MAX_STACK {
                continue;
            }
            let moved = (MAX_STACK - stack.count).min(left);
            stack.count += moved;
            left -= moved;
        }

        while left > 0 {
            let Some(empty) = self.slots.iter_mut().find(|s| s.is_none()) else {
                return left;
            };
            let moved = left.min(MAX_STACK);
            *empty = Some(Stack::new(block, moved));
            left -= moved;
        }
        0
    }

    /// Takes one block of a kind out, from the smallest stack of it.
    ///
    /// Smallest so that part-filled slots get consolidated by ordinary
    /// play rather than accumulating.
    pub fn take_one(&mut self, block: BlockId) -> bool {
        let Some(index) = self
            .slots
            .iter()
            .enumerate()
            .filter(|(_, s)| matches!(s, Some(stack) if stack.block == block && stack.count > 0))
            .min_by_key(|(_, s)| s.map(|stack| stack.count).unwrap_or(u32::MAX))
            .map(|(index, _)| index)
        else {
            return false;
        };
        self.take_from(index, 1) == 1
    }

    /// Removes up to `amount` from one slot, freeing it if emptied.
    /// Returns how many actually came out.
    pub fn take_from(&mut self, slot: usize, amount: u32) -> u32 {
        let Some(Some(stack)) = self.slots.get_mut(slot) else {
            return 0;
        };
        let taken = stack.count.min(amount);
        stack.count -= taken;
        if stack.count == 0 {
            // Released rather than left as an empty labelled slot: a
            // slot reserved for a block you ran out of is a slot the
            // next pickup cannot use.
            self.slots[slot] = None;
        }
        taken
    }

    /// Removes `amount` of `block` from anywhere, or nothing at all.
    ///
    /// All-or-nothing on purpose: crafting consumes several ingredients,
    /// and a partial take would leave the player short with nothing to
    /// show for it.
    pub fn take_exact(&mut self, block: BlockId, amount: u32) -> bool {
        if self.count(block) < amount {
            return false;
        }
        let mut left = amount;
        for index in 0..self.slots.len() {
            if left == 0 {
                break;
            }
            if self.slots[index].map(|s| s.block) != Some(block) {
                continue;
            }
            left -= self.take_from(index, left);
        }
        left == 0
    }

    /// Whether `amount` of `block` would fit.
    ///
    /// `saturating_sub` rather than `-`: a stack over the limit is not
    /// supposed to exist, but this type's whole reason for having
    /// `sanitize` is that inventories arrive from sockets and from saves
    /// written by builds with a different `MAX_STACK`. An overfull stack
    /// has *no* room in it, which is what saturating arithmetic says; a
    /// plain subtraction says the same thing in release and panics in
    /// debug, and the caller is a question about space, not a place to
    /// discover a bad save.
    pub fn has_room_for(&self, block: BlockId, amount: u32) -> bool {
        let mut room: u32 = 0;
        for slot in &self.slots {
            room += match slot {
                Some(stack) if stack.block == block => MAX_STACK.saturating_sub(stack.count),
                Some(_) => 0,
                None => MAX_STACK,
            };
            if room >= amount {
                return true;
            }
        }
        false
    }
}

// ---- moving things between two inventories ----
//
// A chest is a second inventory, and every gesture the inventory screen
// already has needs a version that crosses from one to the other. They
// are free functions rather than methods because neither side owns the
// gesture: `a.move_to(b)` reads as though the pack is doing something to
// the chest, and the two are symmetric.
//
// The one rule that makes all of them safe: **a stack is never in
// neither**. Each of these takes the source out, tries to put it down,
// and puts back whatever did not fit -- into the slot it came from,
// which is guaranteed to be empty because it was just emptied. There is
// no path out of any of them where a count goes missing, which is the
// only bug in an inventory that players will not forgive.

/// Moves a whole slot from one inventory into a slot of another.
///
/// Merges onto the same block, swaps with a different one, and moves
/// into an empty slot. Returns whether anything changed.
pub fn move_between(
    from: &mut Inventory,
    from_slot: usize,
    to: &mut Inventory,
    to_slot: usize,
) -> bool {
    let Some(source) = from.slots().get(from_slot).copied().flatten() else {
        return false;
    };
    let target = to.slots().get(to_slot).copied().flatten();
    match target {
        // A different block in the way: swap the two slots outright.
        // Both are whole stacks, so nothing has to fit anywhere.
        Some(target) if target.block != source.block => {
            from.take_slot(from_slot);
            to.take_slot(to_slot);
            from.put_in_slot(from_slot, target);
            to.put_in_slot(to_slot, source);
            true
        }
        _ => {
            let Some(taken) = from.take_slot(from_slot) else {
                return false;
            };
            let left = to.put_in_slot(to_slot, taken);
            match left {
                None => true,
                Some(left) => {
                    // Whatever did not fit goes back where it came from.
                    from.put_in_slot(from_slot, left);
                    left.count < taken.count
                }
            }
        }
    }
}

/// Moves half of a slot into a slot of another inventory.
///
/// The right-click gesture. Refuses onto a different block for the same
/// reason `split_into` does: a gesture that sometimes splits and
/// sometimes swaps is one the player cannot aim.
pub fn split_between(
    from: &mut Inventory,
    from_slot: usize,
    to: &mut Inventory,
    to_slot: usize,
) -> bool {
    let Some(source) = from.slots().get(from_slot).copied().flatten() else {
        return false;
    };
    if let Some(target) = to.slots().get(to_slot).copied().flatten() {
        if target.block != source.block {
            return false;
        }
    }
    // Rounded up, so splitting a single block moves it rather than
    // doing nothing at all.
    let wanted = source.count.div_ceil(2);
    let taken = from.take_from(from_slot, wanted);
    if taken == 0 {
        return false;
    }
    let left = to.put_in_slot(to_slot, Stack::new(source.block, taken));
    match left {
        None => true,
        Some(left) => {
            from.put_in_slot(from_slot, left);
            left.count < taken
        }
    }
}

/// Sends a whole slot to wherever it fits in another inventory.
///
/// The shift-click, across the two. Part-stacks of the same block first,
/// then the first empty slot -- the same order `add` uses, so a handful
/// of dirt tops up the dirt already in the chest instead of claiming a
/// slot beside it.
pub fn quick_move_between(from: &mut Inventory, from_slot: usize, to: &mut Inventory) -> bool {
    let Some(source) = from.slots().get(from_slot).copied().flatten() else {
        return false;
    };
    let Some(taken) = from.take_slot(from_slot) else {
        return false;
    };
    let left = to.add(source.block, taken.count);
    if left == taken.count {
        // Nowhere for any of it: put it back exactly as it was.
        from.put_in_slot(from_slot, taken);
        return false;
    }
    if left > 0 {
        from.put_in_slot(from_slot, Stack::new(source.block, left));
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{BLOCK_DIRT, BLOCK_STONE};

    #[test]
    fn a_new_inventory_is_empty() {
        let inventory = Inventory::new();
        assert!(inventory.is_empty());
        assert_eq!(inventory.slots().len(), SLOTS);
        for slot in 0..SLOTS {
            assert_eq!(inventory.block_in(slot), None);
        }
    }

    #[test]
    fn slots_are_claimed_in_pickup_order_and_topped_up_first() {
        let mut inventory = Inventory::new();
        assert_eq!(inventory.add(BLOCK_STONE, 1), 0);
        assert_eq!(inventory.block_in(0), Some(BLOCK_STONE));
        assert_eq!(inventory.block_in(1), None);

        assert_eq!(inventory.add(BLOCK_DIRT, 1), 0);
        assert_eq!(inventory.block_in(1), Some(BLOCK_DIRT));

        // More stone stacks where it already is.
        inventory.add(BLOCK_STONE, 5);
        assert_eq!(inventory.count_in(0), 6);
        assert_eq!(inventory.block_in(2), None);
    }

    #[test]
    fn a_stack_spills_into_the_next_slot_rather_than_being_capped() {
        let mut inventory = Inventory::new();
        assert_eq!(inventory.add(BLOCK_DIRT, MAX_STACK + 50), 0);
        assert_eq!(inventory.count(BLOCK_DIRT), MAX_STACK + 50);
        assert_eq!(inventory.count_in(0), MAX_STACK);
        assert_eq!(inventory.count_in(1), 50);
    }

    #[test]
    fn what_does_not_fit_is_reported_rather_than_swallowed() {
        // The caller has to be able to leave the remainder in the world.
        // Silently eating it is how items disappear.
        let mut inventory = Inventory::new();
        let capacity = SLOTS as u32 * MAX_STACK;
        let left = inventory.add(BLOCK_STONE, capacity + 40);
        assert_eq!(left, 40, "the overflow was lost instead of reported");
        assert_eq!(inventory.count(BLOCK_STONE), capacity);
    }

    #[test]
    fn taking_the_last_of_a_stack_frees_its_slot() {
        let mut inventory = Inventory::new();
        inventory.add(BLOCK_DIRT, 1);
        assert!(inventory.take_one(BLOCK_DIRT));
        assert_eq!(inventory.block_in(0), None);
        assert!(!inventory.take_one(BLOCK_DIRT), "took from an empty inventory");
    }

    #[test]
    fn taking_exactly_is_all_or_nothing() {
        // Crafting consumes several ingredients; a partial take would
        // leave the player short with nothing to show for it.
        let mut inventory = Inventory::new();
        inventory.add(BLOCK_STONE, 3);
        assert!(!inventory.take_exact(BLOCK_STONE, 4), "took more than it had");
        assert_eq!(inventory.count(BLOCK_STONE), 3, "a refused take still removed some");
        assert!(inventory.take_exact(BLOCK_STONE, 3));
        assert_eq!(inventory.count(BLOCK_STONE), 0);
    }

    #[test]
    fn taking_exactly_drains_across_several_slots() {
        let mut inventory = Inventory::new();
        inventory.add(BLOCK_STONE, MAX_STACK + 10);
        assert!(inventory.take_exact(BLOCK_STONE, MAX_STACK + 5));
        assert_eq!(inventory.count(BLOCK_STONE), 5);
    }

    #[test]
    fn room_is_measured_across_partial_stacks_and_empty_slots() {
        let mut inventory = Inventory::new();
        assert!(inventory.has_room_for(BLOCK_STONE, SLOTS as u32 * MAX_STACK));
        assert!(!inventory.has_room_for(BLOCK_STONE, SLOTS as u32 * MAX_STACK + 1));

        // Fill everything with something else, leaving one part-stack.
        for _ in 0..SLOTS {
            inventory.add(BLOCK_DIRT, MAX_STACK);
        }
        inventory.take_from(0, 5);
        assert!(inventory.has_room_for(BLOCK_DIRT, 5));
        assert!(!inventory.has_room_for(BLOCK_DIRT, 6));
        assert!(!inventory.has_room_for(BLOCK_STONE, 1), "no slot is free for a new kind");
    }

    #[test]
    fn weight_follows_what_is_carried() {
        let mut inventory = Inventory::new();
        assert_eq!(inventory.total_weight(), 0.0);
        inventory.add(BLOCK_STONE, 10);
        let expected = block_weight(BLOCK_STONE) * 10.0;
        assert!((inventory.total_weight() - expected).abs() < 1e-3);
    }

    #[test]
    fn swapping_moves_stacks_without_changing_anything_else() {
        let mut inventory = Inventory::new();
        inventory.add(BLOCK_STONE, 5);
        let weight = inventory.total_weight();

        inventory.swap(0, SLOTS - 1);
        assert_eq!(inventory.block_in(SLOTS - 1), Some(BLOCK_STONE));
        assert_eq!(inventory.block_in(0), None);
        assert_eq!(inventory.total_weight(), weight);

        // Indices off the end arrive from the network; they must not
        // panic and must not do anything.
        inventory.swap(0, 9_999);
        inventory.swap(9_999, 0);
        assert_eq!(inventory.count(BLOCK_STONE), 5);
    }

    #[test]
    fn moving_onto_the_same_block_merges_instead_of_swapping() {
        // The commonest gesture in the screen. Swapping two part-stacks
        // of stone leaves the player doing it again with the same
        // result, forever.
        // Built by hand: `add` tops up what is already there, so it
        // cannot produce the two separate part-stacks this is about.
        let mut inventory = Inventory::new();
        inventory.slots[0] = Some(Stack::new(BLOCK_STONE, 7));
        inventory.slots[3] = Some(Stack::new(BLOCK_STONE, 5));

        assert!(inventory.move_or_merge(0, 3));
        assert_eq!(inventory.count_in(3), 12);
        assert_eq!(inventory.block_in(0), None, "the emptied slot was not released");
    }

    #[test]
    fn a_merge_into_a_nearly_full_stack_tops_it_up_and_leaves_the_rest() {
        let mut inventory = Inventory::new();
        inventory.slots[0] = Some(Stack::new(BLOCK_STONE, 10));
        inventory.slots[5] = Some(Stack::new(BLOCK_STONE, MAX_STACK - 2));

        assert!(inventory.move_or_merge(0, 5));
        assert_eq!(inventory.count_in(5), MAX_STACK);
        assert_eq!(inventory.count_in(0), 8, "the overflow was lost");
    }

    #[test]
    fn moving_onto_a_different_block_still_swaps() {
        let mut inventory = Inventory::new();
        inventory.add(BLOCK_STONE, 5);
        inventory.add(BLOCK_DIRT, 3);
        assert!(inventory.move_or_merge(0, 1));
        assert_eq!(inventory.block_in(0), Some(BLOCK_DIRT));
        assert_eq!(inventory.block_in(1), Some(BLOCK_STONE));
    }

    #[test]
    fn a_move_that_cannot_change_anything_says_so() {
        let mut inventory = Inventory::new();
        inventory.add(BLOCK_STONE, MAX_STACK);
        inventory.add(BLOCK_STONE, MAX_STACK);
        assert!(!inventory.move_or_merge(0, 1), "two full stacks reported a merge");
        assert!(!inventory.move_or_merge(2, 3), "moving nothing reported a move");
        assert!(!inventory.move_or_merge(0, 0));
        // Off the end: these arrive from the network.
        assert!(!inventory.move_or_merge(0, 9_999));
        assert!(!inventory.move_or_merge(9_999, 0));
        assert_eq!(inventory.count(BLOCK_STONE), MAX_STACK * 2, "blocks changed anyway");
    }

    #[test]
    fn splitting_leaves_half_behind() {
        let mut inventory = Inventory::new();
        inventory.add(BLOCK_STONE, 5);
        assert!(inventory.split_into(0, 7));
        assert_eq!(inventory.count_in(7), 3);
        assert_eq!(inventory.count_in(0), 2);
        assert_eq!(inventory.count(BLOCK_STONE), 5, "splitting invented or ate blocks");
    }

    #[test]
    fn splitting_a_single_block_moves_it_rather_than_doing_nothing() {
        let mut inventory = Inventory::new();
        inventory.add(BLOCK_DIRT, 1);
        assert!(inventory.split_into(0, 4));
        assert_eq!(inventory.count_in(4), 1);
        assert_eq!(inventory.block_in(0), None);
    }

    #[test]
    fn splitting_onto_another_block_is_refused_rather_than_swapping() {
        // A gesture that sometimes splits and sometimes swaps is one the
        // player cannot aim.
        let mut inventory = Inventory::new();
        inventory.add(BLOCK_STONE, 6);
        inventory.add(BLOCK_DIRT, 6);
        assert!(!inventory.split_into(0, 1));
        assert_eq!(inventory.count_in(0), 6);
        assert_eq!(inventory.count_in(1), 6);
    }

    #[test]
    fn splitting_onto_the_same_block_tops_it_up() {
        let mut inventory = Inventory::new();
        inventory.slots[0] = Some(Stack::new(BLOCK_STONE, 3));
        inventory.slots[2] = Some(Stack::new(BLOCK_STONE, 8));
        assert!(inventory.split_into(2, 0));
        assert_eq!(inventory.count_in(0), 3 + 4);
        assert_eq!(inventory.count_in(2), 4);
    }

    #[test]
    fn a_quick_move_crosses_between_the_bar_and_the_pile() {
        let mut inventory = Inventory::new();
        inventory.add(BLOCK_STONE, 5); // slot 0, on the bar

        assert!(inventory.quick_move(0));
        assert_eq!(inventory.block_in(0), None, "it stayed on the bar");
        assert_eq!(inventory.block_in(HOTBAR_SLOTS), Some(BLOCK_STONE));

        // ...and back again.
        assert!(inventory.quick_move(HOTBAR_SLOTS));
        assert_eq!(inventory.block_in(0), Some(BLOCK_STONE));
        assert_eq!(inventory.block_in(HOTBAR_SLOTS), None);
    }

    #[test]
    fn a_quick_move_merges_rather_than_claiming_a_second_slot() {
        let mut inventory = Inventory::new();
        inventory.slots[0] = Some(Stack::new(BLOCK_STONE, 6));
        // A part-stack of the same block already waiting in storage.
        inventory.slots[HOTBAR_SLOTS] = Some(Stack::new(BLOCK_STONE, 4));

        assert!(inventory.quick_move(0));
        assert_eq!(inventory.count_in(HOTBAR_SLOTS), 10);
        assert_eq!(inventory.block_in(0), None);
        assert_eq!(
            inventory.count(BLOCK_STONE),
            10,
            "a quick move changed how much was carried"
        );
    }

    #[test]
    fn a_quick_move_with_nowhere_to_go_leaves_everything_alone() {
        let mut inventory = Inventory::new();
        // Every storage slot full of something else entirely.
        for slot in HOTBAR_SLOTS..SLOTS {
            inventory.slots[slot] = Some(Stack::new(BLOCK_DIRT, MAX_STACK));
        }
        inventory.slots[0] = Some(Stack::new(BLOCK_STONE, 3));

        assert!(!inventory.quick_move(0), "it claimed a slot that was not free");
        assert_eq!(inventory.count_in(0), 3);
        assert!(!inventory.quick_move(SLOTS), "an index off the end did something");
    }

    #[test]
    fn tidying_folds_part_stacks_together_and_leaves_the_bar_alone() {
        let mut inventory = Inventory::new();
        // A deliberate arrangement on the bar...
        inventory.add(BLOCK_DIRT, 1);
        inventory.swap(0, 4);
        let bar: Vec<Option<Stack>> = inventory.slots()[..HOTBAR_SLOTS].to_vec();

        // ...and a mess behind it: three part-stacks with a gap in them.
        for (offset, count) in [(0usize, 3u32), (2, 4), (5, 5)] {
            inventory.slots[HOTBAR_SLOTS + offset] = Some(Stack::new(BLOCK_STONE, count));
        }

        assert!(inventory.sort_storage());
        assert_eq!(inventory.slots()[..HOTBAR_SLOTS], bar[..], "the bar was rearranged");
        assert_eq!(inventory.count_in(HOTBAR_SLOTS), 12, "part-stacks were not folded");
        for slot in (HOTBAR_SLOTS + 1)..SLOTS {
            assert_eq!(inventory.block_in(slot), None, "slot {slot} was left occupied");
        }
        assert!(!inventory.sort_storage(), "tidying a tidy pile reported a change");
    }

    #[test]
    fn tidying_never_loses_anything() {
        let mut inventory = Inventory::new();
        for slot in HOTBAR_SLOTS..SLOTS {
            let block = if slot % 2 == 0 { BLOCK_STONE } else { BLOCK_DIRT };
            inventory.slots[slot] = Some(Stack::new(block, MAX_STACK - 7));
        }
        let stone = inventory.count(BLOCK_STONE);
        let dirt = inventory.count(BLOCK_DIRT);

        inventory.sort_storage();
        assert_eq!(inventory.count(BLOCK_STONE), stone);
        assert_eq!(inventory.count(BLOCK_DIRT), dirt);
        for slot in inventory.slots().iter().flatten() {
            assert!(slot.count <= MAX_STACK, "tidying built an oversized stack");
            assert!(slot.count > 0, "tidying left an empty stack in a slot");
        }
    }

    #[test]
    fn a_malformed_inventory_from_the_wire_is_repaired() {
        // Slot counts and stack limits change between versions, so the
        // shape of what arrives is not ours to assume.
        let mut inventory = Inventory {
            slots: vec![
                Some(Stack::new(BLOCK_STONE, MAX_STACK * 9)),
                Some(Stack::new(BLOCK_DIRT, 0)),
            ],
        };
        inventory.sanitize();
        assert_eq!(inventory.slots().len(), SLOTS);
        assert_eq!(inventory.count_in(0), MAX_STACK, "an oversized stack survived");
        assert_eq!(inventory.block_in(1), None, "an empty stack kept its slot");
    }

    #[test]
    fn taking_from_a_slot_that_is_not_there_is_not_a_panic() {
        let mut inventory = Inventory::new();
        assert_eq!(inventory.take_from(9_999, 1), 0);
        assert_eq!(inventory.take_from(0, 1), 0);
    }

    #[test]
    fn an_overfull_stack_has_no_room_rather_than_panicking() {
        // Stacks over the limit are not supposed to exist, and arrive
        // anyway: `sanitize` exists because inventories come off sockets
        // and out of saves written when `MAX_STACK` was a different
        // number. Asking such a slot how much room it has subtracted the
        // larger from the smaller -- a panic in a debug build, and an
        // answer of about four billion in a release one, which reads as
        // "yes, plenty of room" and hands the caller a slot that cannot
        // take anything.
        let mut inventory = Inventory::new();
        inventory.slots[0] = Some(Stack::new(BLOCK_STONE, MAX_STACK + 5));
        for slot in inventory.slots.iter_mut().skip(1) {
            *slot = Some(Stack::new(BLOCK_DIRT, MAX_STACK));
        }
        assert!(!inventory.has_room_for(BLOCK_STONE, 1));

        // ...and it is still counted honestly everywhere else, so
        // `sanitize` has something to find.
        assert_eq!(inventory.count(BLOCK_STONE), MAX_STACK + 5);
        inventory.sanitize();
        assert_eq!(inventory.count(BLOCK_STONE), MAX_STACK);
    }
}

/// Moving things between a pack and a chest.
///
/// Every one of these is checked for the same thing above all others:
/// that nothing is created and nothing disappears. A chest is where a
/// player leaves everything they own, so a gesture that loses a stack
/// once in a hundred is worse than no chest at all.
#[cfg(test)]
mod transfer_tests {
    use super::*;
    use crate::types::{BLOCK_DIRT, BLOCK_STONE};

    fn total(a: &Inventory, b: &Inventory) -> u32 {
        a.total_items() + b.total_items()
    }

    fn pack_and_chest() -> (Inventory, Inventory) {
        let mut pack = Inventory::new();
        pack.add(BLOCK_STONE, 20);
        (pack, Inventory::new())
    }

    #[test]
    fn a_stack_moves_into_an_empty_chest_slot() {
        let (mut pack, mut chest) = pack_and_chest();
        assert!(move_between(&mut pack, 0, &mut chest, 5));
        assert_eq!(pack.total_items(), 0);
        assert_eq!(chest.count_in(5), 20);
    }

    #[test]
    fn it_merges_onto_the_same_block_and_swaps_with_a_different_one() {
        let (mut pack, mut chest) = pack_and_chest();
        chest.put_in_slot(0, Stack::new(BLOCK_STONE, 5));
        assert!(move_between(&mut pack, 0, &mut chest, 0));
        assert_eq!(chest.count_in(0), 25, "the two stacks did not merge");
        assert_eq!(pack.total_items(), 0);

        // ...and a different block changes places rather than merging.
        let mut pack = Inventory::new();
        pack.add(BLOCK_DIRT, 7);
        assert!(move_between(&mut pack, 0, &mut chest, 0));
        assert_eq!(chest.block_in(0), Some(BLOCK_DIRT));
        assert_eq!(chest.count_in(0), 7);
        assert_eq!(pack.block_in(0), Some(BLOCK_STONE));
        assert_eq!(pack.count_in(0), 25);
    }

    #[test]
    fn what_does_not_fit_goes_back_where_it_came_from() {
        // The moment that matters: the stack is out of the pack and the
        // chest slot is nearly full. Whatever is left has to land
        // somewhere, and the slot it came from is the only place it can.
        let mut pack = Inventory::new();
        pack.put_in_slot(0, Stack::new(BLOCK_STONE, MAX_STACK));
        let mut chest = Inventory::new();
        chest.put_in_slot(3, Stack::new(BLOCK_STONE, MAX_STACK - 4));
        let before = total(&pack, &chest);

        assert!(move_between(&mut pack, 0, &mut chest, 3));
        assert_eq!(chest.count_in(3), MAX_STACK, "the chest slot is not full");
        assert_eq!(pack.count_in(0), MAX_STACK - 4, "the remainder went missing");
        assert_eq!(total(&pack, &chest), before, "blocks were created or lost");
    }

    #[test]
    fn half_a_stack_can_be_dealt_across() {
        let (mut pack, mut chest) = pack_and_chest();
        let before = total(&pack, &chest);
        assert!(split_between(&mut pack, 0, &mut chest, 0));
        assert_eq!(chest.count_in(0), 10);
        assert_eq!(pack.count_in(0), 10);
        assert_eq!(total(&pack, &chest), before);

        // A single block splits as itself rather than as nothing.
        let mut pack = Inventory::new();
        pack.add(BLOCK_DIRT, 1);
        assert!(split_between(&mut pack, 0, &mut chest, 1));
        assert_eq!(chest.count_in(1), 1);
        assert_eq!(pack.total_items(), 0);
    }

    #[test]
    fn a_split_onto_a_different_block_is_refused_rather_than_swapped() {
        let (mut pack, mut chest) = pack_and_chest();
        chest.put_in_slot(0, Stack::new(BLOCK_DIRT, 3));
        assert!(!split_between(&mut pack, 0, &mut chest, 0));
        assert_eq!(pack.count_in(0), 20, "the pack changed on a refused split");
        assert_eq!(chest.count_in(0), 3);
    }

    #[test]
    fn a_quick_move_fills_part_stacks_before_claiming_a_slot() {
        let (mut pack, mut chest) = pack_and_chest();
        chest.put_in_slot(4, Stack::new(BLOCK_STONE, MAX_STACK - 5));
        let before = total(&pack, &chest);
        assert!(quick_move_between(&mut pack, 0, &mut chest));
        assert_eq!(chest.count_in(4), MAX_STACK, "the part-stack was not topped up");
        // Five of the twenty topped that slot up; the other fifteen had
        // to claim one of their own.
        assert_eq!(chest.count_in(0), 15, "the rest did not follow it");
        assert_eq!(pack.total_items(), 0);
        assert_eq!(total(&pack, &chest), before);
    }

    #[test]
    fn a_quick_move_into_a_full_chest_changes_nothing_at_all() {
        // The refusal has to be complete: a stack half-moved into a full
        // chest is a stack the player has to hunt for.
        let mut pack = Inventory::new();
        pack.add(BLOCK_DIRT, 30);
        let mut chest = Inventory::new();
        for slot in 0..SLOTS {
            chest.put_in_slot(slot, Stack::new(BLOCK_STONE, MAX_STACK));
        }
        let before = total(&pack, &chest);

        assert!(!quick_move_between(&mut pack, 0, &mut chest));
        assert_eq!(pack.count_in(0), 30, "the pack lost a stack to a full chest");
        assert_eq!(total(&pack, &chest), before);
    }

    #[test]
    fn a_quick_move_that_only_half_fits_keeps_the_rest() {
        let mut pack = Inventory::new();
        pack.put_in_slot(0, Stack::new(BLOCK_DIRT, MAX_STACK));
        let mut chest = Inventory::new();
        // Every slot full of something else but one, which has room for
        // ten.
        for slot in 0..SLOTS {
            chest.put_in_slot(slot, Stack::new(BLOCK_STONE, MAX_STACK));
        }
        chest.take_slot(2);
        chest.put_in_slot(2, Stack::new(BLOCK_DIRT, MAX_STACK - 10));
        let before = total(&pack, &chest);

        assert!(quick_move_between(&mut pack, 0, &mut chest));
        assert_eq!(chest.count_in(2), MAX_STACK);
        assert_eq!(pack.count_in(0), MAX_STACK - 10, "the remainder was lost");
        assert_eq!(total(&pack, &chest), before);
    }

    #[test]
    fn moving_from_an_empty_slot_or_off_the_end_does_nothing() {
        // These arrive from the wire: the slot index is whatever the
        // client said it was.
        let (mut pack, mut chest) = pack_and_chest();
        let before = total(&pack, &chest);
        assert!(!move_between(&mut pack, 7, &mut chest, 0), "moved nothing somewhere");
        assert!(!move_between(&mut pack, 9_999, &mut chest, 0));
        assert!(!move_between(&mut pack, 0, &mut chest, 9_999));
        assert!(!split_between(&mut pack, 0, &mut chest, 9_999));
        assert!(!quick_move_between(&mut pack, 9_999, &mut chest));
        assert_eq!(total(&pack, &chest), before);
        assert_eq!(pack.count_in(0), 20, "a refused move still moved something");
    }
}
