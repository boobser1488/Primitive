//! The client's copy of what the player is carrying.
//!
//! There is almost nothing here any more, and that is the point. The
//! inventory used to live on the client, which meant guessing at the
//! result of every edit before the server confirmed it and unwinding the
//! guesses that turned out wrong -- a whole machine of pending breaks,
//! reserved placements, refunds and timeouts.
//!
//! Now the server owns it and sends a snapshot whenever it changes. The
//! client draws that snapshot and sends *intents* (`MoveSlots`,
//! `SplitSlot`, `DropSlot`, `Craft`...). Nothing to reconcile, because
//! nothing is predicted: the machine is gone rather than fixed.
//!
//! The type itself is shared, so the two sides cannot drift on what a
//! stack is or how much one weighs.

pub use primitive_shared::inventory::{Inventory, HOTBAR_SLOTS, MAX_STACK, SLOTS};

/// The bar is the front of the pack, so the pack has to be at least as
/// long as the bar. Checked at compile time rather than in a test: it is
/// a relation between two constants, and the build is where a relation
/// between two constants should fail.
const _: () = assert!(SLOTS >= HOTBAR_SLOTS);

#[cfg(test)]
mod tests {
    use super::*;
    use primitive_shared::types::{BLOCK_DIRT, BLOCK_STONE};

    /// The client only ever *reads* this type, so what matters here is
    /// that the shared one answers the questions the UI asks.
    #[test]
    fn the_ui_can_ask_everything_it_needs_of_a_snapshot() {
        let mut inventory = Inventory::new();
        inventory.add(BLOCK_STONE, MAX_STACK + 3);
        inventory.add(BLOCK_DIRT, 2);

        assert_eq!(inventory.block_in(0), Some(BLOCK_STONE));
        assert_eq!(inventory.count_in(0), MAX_STACK);
        assert_eq!(inventory.count_in(1), 3);
        assert_eq!(inventory.block_in(2), Some(BLOCK_DIRT));
        assert!(inventory.total_weight() > 0.0);
        assert_eq!(inventory.slots().len(), SLOTS);
    }

    #[test]
    fn an_empty_snapshot_reads_as_empty_rather_than_as_missing() {
        let inventory = Inventory::new();
        assert!(inventory.is_empty());
        assert_eq!(inventory.total_weight(), 0.0);
        for slot in 0..SLOTS {
            assert_eq!(inventory.block_in(slot), None);
        }
    }
}
