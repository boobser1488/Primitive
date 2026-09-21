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

use crate::types::{block_weight, stack_limit, BlockId};

/// Hotbar slots. Ten, because that is how far the number row reaches.
pub const HOTBAR_SLOTS: usize = 10;
/// Rows of storage behind the hotbar, reachable from the inventory
/// screen.
///
/// **Three rows, as it was before it was halved and after.** It was cut
/// to one, on the argument that forty squares are four hundred kilograms
/// of stone and a pack with room for everything has no decisions in it.
/// Played, the player asked for it back ("верни инвентарь старого
/// размера"): ten squares behind the belt is not a choice about what to
/// leave, it is a trip home every few minutes, and the choice the design
/// wants is made by weight (`load`) long before the squares run out. The
/// rucksack stays half the pack, so it is still worth making.
pub const STORAGE_ROWS: usize = 3;
/// Every slot a player has *on their person*. The hotbar is the first
/// `HOTBAR_SLOTS` of them, so a slot index means the same thing
/// everywhere.
pub const SLOTS: usize = HOTBAR_SLOTS * (1 + STORAGE_ROWS);

/// How many more squares a worn rucksack is worth.
///
/// **Half the pack, which is the player's own phrasing and also the
/// right number.** A pack that doubled what you carry would put the
/// game back where halving `STORAGE_ROWS` took it from; half of it is a
/// visible, wearable reason to go home lighter, and it is exactly the
/// pile behind the belt again -- so "with a rucksack" is "with the pack
/// I used to have", and the player can feel what the rucksack bought.
pub const BACKPACK_SLOTS: usize = SLOTS / 2;

/// The most squares an inventory can ever have: the body's own, plus a
/// rucksack's.
///
/// **The rucksack is not a second container.** It is these slots,
/// appended to the one the player already has, and they exist only
/// while something is worn in `equipment::Slot::Back` -- see
/// [`Inventory::open_backpack`]. A separate `Inventory` for the
/// rucksack was written first and thrown away: every gesture on the
/// pack screen is `Move { from, to }` over one index space, and a
/// second container means every one of them grows a "which container"
/// word, the server grows a second snapshot message, and death has two
/// piles to drop instead of one. Growing the `Vec` costs a length check
/// in `sanitize` and nothing else.
pub const MAX_SLOTS: usize = SLOTS + BACKPACK_SLOTS;

/// The squares a worn rucksack adds, as a range into the slot list.
///
/// Empty of meaning when no rucksack is on -- the slots are simply not
/// there -- which is why everything that reads it asks
/// [`Inventory::backpack_open`] first, or walks `slots()` and finds
/// nothing past `SLOTS`.
pub const BACKPACK_RANGE: std::ops::Range<usize> = SLOTS..MAX_SLOTS;

/// How many squares a chest, a corpse or any other box in the world has.
///
/// **Forty, which is what a player used to be able to carry** -- and it
/// stayed forty when the pack was halved. Two reasons, and the first one
/// is not a design argument at all:
///
/// 1. **Every chest in every existing save is forty squares long.**
///    `sanitize` runs over each of them on load, and a `sanitize` that
///    resized to the *player's* slot count would have emptied the back
///    half of every chest in the world the first time the game started
///    after `STORAGE_ROWS` changed. A container's size is a fact about
///    the container, not about whoever is standing in front of it.
/// 2. **A chest is meant to be bigger than a pack.** It was exactly a
///    pack's worth, which was a tidy identity and also meant a chest
///    could never do anything a pack could not. The whole point of
///    halving the pack is that a base is where things live and a trip is
///    what you can carry, and that reads only if the box at home is the
///    larger of the two.
pub const CHEST_SLOTS: usize = 40;

/// The rucksack's compartment on a body: the squares after a chest's forty
/// that a corpse has when the player who died was wearing a rucksack.
///
/// **A dead player's rucksack is kept as a rucksack, square for square.**
/// Before this a body was forty squares whatever was worn, and a pack of
/// sixty was folded into it: the rucksack's twenty went into whatever was
/// free in the pack's part and the rest were spilled on the ground. So a
/// player who walked back found their things re-dealt and some of them in
/// the grass beside the body -- the one trip in the game whose whole point
/// is "it is all still here", answered with "most of it, somewhere".
///
/// **Squares of the same container, not a second one**, for the reason
/// `MAX_SLOTS` gives for the rucksack itself: every gesture at a container
/// is `(side, slot)`, the save is one `Inventory` per cell, and breaking the
/// body spills one pile. A second container per body was the rejected
/// version -- a second entry to save, to keep in step, and to forget when
/// the body rots to bones.
///
/// Where it starts is `CHEST_SLOTS` rather than `SLOTS`, because it is a
/// fact about the body; that the two are both forty is what lets a pack's
/// square keep its own number on the body (asserted below).
pub const CORPSE_COMPARTMENT: std::ops::Range<usize> = CHEST_SLOTS..CHEST_SLOTS + BACKPACK_SLOTS;

/// How long a body with a rucksack's compartment is.
pub const CORPSE_SLOTS: usize = CORPSE_COMPARTMENT.end;

// A pack's own squares land on a body's squares of the same number. A pack
// that outgrew a chest would have nowhere square-for-square to go, and
// `leave_corpse` would quietly go back to re-dealing it.
const _: () = assert!(SLOTS <= CHEST_SLOTS);

const fn larger(a: usize, b: usize) -> usize {
    if a > b {
        a
    } else {
        b
    }
}

/// The most squares any container in the game has: a chest, a pack with a
/// rucksack on it, or a body with a rucksack's compartment, whichever is
/// largest. What `sanitize` clamps to -- and a clamp at anything smaller
/// is a load that empties every dead player's rucksack.
pub const LARGEST_CONTAINER: usize = larger(larger(MAX_SLOTS, CHEST_SLOTS), CORPSE_SLOTS);

/// How many of one block fit in one slot.
///
/// A real constraint rather than a display one: a full stack of stone is
/// over three hundred kilograms, and that is what makes the inventory a
/// series of decisions instead of a bucket.
pub const MAX_STACK: u32 = 128;

// **`MAX_STACK` is the ceiling, not the rule.** What actually fits in a
// slot is `types::stack_limit`, which is this for everything a player
// gathers and *one* for everything they hold -- see
// `blocks::BlockDef::stack`. Every path below that puts something into a
// slot asks the block, because the one that forgets is the one that
// lets a player carry a slot full of axes.

/// What one swing did to a tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wear {
    /// Nothing there that wears out.
    None,
    /// A swing off its life.
    Worn,
    /// That was the last one; the slot is empty now.
    Broke,
}

/// What one tick of burning did to a lit torch. See `burn_torch`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TorchBurn {
    /// The slot holds no lit torch, which is almost every slot.
    NoTorch,
    /// Still alight.
    Burning,
    /// The fibre is gone and a spent torch is in its place.
    WentOut,
}

/// One slot's contents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stack {
    pub block: BlockId,
    pub count: u32,
    /// How worn a tool is: swings taken, against
    /// `types::tool_durability`.
    ///
    /// **On the stack rather than on the block id**, because it is a
    /// fact about *this* axe rather than about axes -- two of them in a
    /// pack are two different amounts of worn. That is affordable only
    /// because a tool stacks to one (see `blocks::BlockDef::stack`): a
    /// stack of a hundred and twenty-eight things with one number
    /// between them would have to say what happens when you split it.
    ///
    /// Zero for everything that is not a tool, and for a tool that has
    /// never been swung. Nothing reads it unless the block has a
    /// durability, so a lump of coal carrying a seven is harmless -- and
    /// `sanitize` clears it anyway.
    ///
    /// **Three meanings now, and never two at once.** The low
    /// twenty-four bits (`quality::WEAR_MASK`) are the wear above for a
    /// tool and a jug's contents for a jug (see `jug_contents`); the top
    /// eight are how well the thing was made (`quality`). Nothing may
    /// read this field raw and mean "swings taken" -- that is what
    /// [`Stack::wear`] is for. The whole argument for stacking three
    /// facts in one word, and what it buys, is in `quality`'s module
    /// note.
    pub damage: u32,
}

impl Stack {
    pub fn new(block: BlockId, count: u32) -> Self {
        Self {
            block,
            count,
            damage: 0,
        }
    }

    /// The same, part worn. What comes back out of a save, and what a
    /// swing produces.
    pub fn worn(block: BlockId, count: u32, damage: u32) -> Self {
        Self {
            block,
            count,
            damage,
        }
    }

    /// Swings taken off this tool, with whatever else the word is
    /// carrying masked away.
    ///
    /// **Every reader of wear goes through here.** `damage` also holds
    /// how well the thing was made, in its top byte, and a site that
    /// compared the raw word against a durability would call a fine axe
    /// broken the moment it was forged.
    pub fn wear(&self) -> u32 {
        self.damage & crate::quality::WEAR_MASK
    }

    /// How well this one was made. See `quality`.
    pub fn quality(&self) -> crate::quality::Quality {
        crate::quality::Quality::from_byte((self.damage >> crate::quality::QUALITY_SHIFT) as u8)
    }

    /// The same thing, judged. What a freshly made piece comes out of the
    /// bench as.
    pub fn with_quality(mut self, quality: crate::quality::Quality) -> Self {
        self.damage = (self.damage & crate::quality::WEAR_MASK)
            | (u32::from(quality.byte()) << crate::quality::QUALITY_SHIFT);
        self
    }

    /// How many swings *this* tool has in it, as opposed to how many the
    /// kind has.
    ///
    /// `None` for anything that does not wear out. This is the one place
    /// quality touches durability, so that everything asking "is it
    /// broken", "how full is the bar" and "did that swing finish it" gets
    /// the same answer from the same arithmetic -- the first cut of this
    /// scaled the bar in the interface and not the break, and a fine pick
    /// showed a third of a bar left and vanished.
    pub fn life(&self) -> Option<u32> {
        let base = crate::types::tool_durability(self.block)?;
        let scaled = base as f32 * crate::quality::durability_scale(self.quality());
        // At least one swing, whatever the arithmetic says: a tool that
        // breaks on the swing that makes it is a bug report.
        Some((scaled.round() as u32).max(1))
    }

    /// How much of this tool is left, 0..1. One for anything that does
    /// not wear out.
    pub fn condition(&self) -> f32 {
        match self.life() {
            None => 1.0,
            Some(total) => 1.0 - (self.wear() as f32 / total.max(1) as f32).clamp(0.0, 1.0),
        }
    }

    /// Whether this tool has been used up.
    pub fn is_broken(&self) -> bool {
        self.life().is_some_and(|total| self.wear() >= total)
    }

    pub fn weight(&self) -> f32 {
        // **What is in a jug weighs what it weighs.** Without this a
        // jug was a hole in the load: sixteen stones poured into one
        // came to a kilo, and the whole reason an inventory is a series
        // of decisions (see `MAX_STACK`) is that a stone weighs a
        // stone wherever it is being carried.
        let inside = match jug_contents(self) {
            Some((block, count)) => block_weight(block) * count as f32,
            None => 0.0,
        };
        block_weight(self.block) * self.count as f32 + inside
    }
}

// ---- what is inside a jug ----
//
// A jug carries one kind of loose, dry goods (`types::pours`), and what
// it is carrying rides in the slot's **`damage`** field: the contents'
// block id in the low sixteen bits, how many units in the high sixteen.
// Zero is an empty jug, which is what `Stack::new` already produces and
// what every jug in every save written before this existed already
// says.
//
// ## Why not a fourth field on `Stack`
//
// The same argument `food` makes about a perishable's age -- see the
// "going off" note in `food.rs`, which rejected a per-stack timestamp
// for exactly these costs. A fourth field is a field the wire, both
// container save formats (`logic::containers`) and the profile store
// (`logic::profiles`) would all have to carry and version, because
// bincode writes fields by position with no names: an old file read as
// a new one does not fail, it decodes into nonsense. `damage` is
// already in all four of those and already versioned there (chest
// format v2, profile v4), already splits correctly with a stack,
// already survives a save and already travels with the item through
// `add_worn`, `put_in_slot`, `quick_move` and `tidy` -- every one of
// which treats it as part of *which object this is*. Reusing it costs
// nothing anybody has to remember.
//
// What it costs is stated plainly: **a jug stacks to one**, because one
// number cannot describe four different jugs. `types::stack_limit` says
// so and explains itself there.
//
// The rejected third option was a jug as a *container*, keyed by
// position like a chest is. A chest has a position; a jug in a pack has
// none, so the key would have to be a synthetic id minted on filling,
// stored beside the world, garbage-collected when the jug is destroyed,
// and reconciled when a player logs in carrying one the server's map
// has never heard of. That is a whole second lifetime to get right for
// sixteen units of sand.

/// The most one jug holds, in units of whatever was poured in.
///
/// Sixteen: an eighth of a stack. Enough that filling one is worth the
/// gesture and few enough that a row of jugs is never a cheaper pack
/// than the pack -- a jug takes a whole slot (see `types::stack_limit`)
/// and gives back an eighth of one, so it is somewhere to *keep* a
/// measure of something rather than a way to carry more.
pub const JUG_UNITS: u32 = 16;

/// What has been poured into this jug, or `None` for anything that is
/// not a jug and for an empty one.
///
/// Bounds-checked rather than trusted: a count of zero, or one past
/// `JUG_UNITS`, is read as an empty jug. The number comes off a wire
/// and out of saves written by builds that did not know what this field
/// meant, and the alternative to checking here is thirteen call sites
/// that each have to remember to.
pub fn jug_contents(stack: &Stack) -> Option<(BlockId, u32)> {
    if crate::types::block_kind(stack.block) != crate::types::BLOCK_JUG {
        return None;
    }
    let block = stack.damage as BlockId;
    // A byte for the count, not the sixteen bits that used to be here.
    // Sixteen units never needed more than five, and the top byte of this
    // word is how well the jug was thrown (`quality`) -- read the count
    // without this mask and every judged jug reports holding several
    // thousand of something.
    let count = (stack.damage >> BlockId::BITS) & 0xFF;
    if block == crate::types::BLOCK_AIR || count == 0 || count > JUG_UNITS {
        return None;
    }
    Some((block, count))
}

/// How many more of `block` a jug already holding `inside` will take.
///
/// **The one rule for filling a vessel, wherever the vessel is.** A jug in
/// a pack slot keeps its contents in `damage` and a jug set down on a
/// table keeps them in the container store, one slot at its cell (see
/// `VESSEL_SLOT`); both are asked this, so a jug cannot hold more, or
/// something different, for having been put on a shelf.
///
/// Zero for anything that does not pour (`types::pours`) -- which is also
/// the whole of why **a vessel cannot go inside a vessel**: a jug is a
/// made thing with a shape rather than a handful, so it is not on that
/// list, and neither is a jug of water. No second check for recursion
/// exists to be forgotten; the only door in is this one. Zero for a jug
/// holding some other goods, because one number describes one kind and
/// mixing them would be choosing whose count to delete.
pub fn jug_room(inside: Option<(BlockId, u32)>, block: BlockId) -> u32 {
    if !crate::types::pours(block) {
        return 0;
    }
    match inside {
        None => JUG_UNITS,
        Some((held, count)) if held == block => JUG_UNITS.saturating_sub(count),
        Some(_) => 0,
    }
}

/// Where a set-down jug keeps what is in it: slot zero of the ordinary
/// container store at its cell.
///
/// **In the store, and not in the block.** A cell is one `u16` and the
/// variant bits are already the jug's own; what was in `damage` has
/// nowhere to go in a chunk. So setting a jug down moves its contents into
/// the store, and picking it up moves them back into `damage` -- see
/// `primitive_server::set_down_vessel` and `pick_up_vessel`. Stored as the
/// goods themselves (grain, twelve) rather than as a jug holding grain,
/// because an empty store entry is no entry at all, and a stored jug stack
/// would give every empty jug on every shelf an entry for ever.
pub const VESSEL_SLOT: usize = 0;

/// What a set-down jug's store entry holds, read as a jug's contents.
///
/// `None` for an empty one. Anything the store has in that slot that a
/// jug could not hold -- more than its measure, something that does not
/// pour -- is not reported as contents: a mod or an old save put it
/// there, and the rest of the vessel code must not start believing that
/// a jug holds forty axes.
pub fn vessel_store_contents(store: &Inventory) -> Option<(BlockId, u32)> {
    let stack = store.slots().get(VESSEL_SLOT).copied().flatten()?;
    (crate::types::pours(stack.block) && stack.count > 0 && stack.count <= JUG_UNITS)
        .then_some((stack.block, stack.count))
}

/// Pours what `pack[from]` holds into a set-down jug's store entry, up to
/// what it has room for, and answers how much went in.
///
/// **All that fits, even for a right click.** Loose goods are tipped in,
/// not counted in; taking them *out* a handful at a time is what the half
/// gesture is for -- the same split the jug in hand has, where the only
/// messages are a pour and a take (see `ClientMessage::TakeFromJug`).
/// Nothing leaves the pack that did not land in the jug: the count is
/// worked out first and only that much is taken.
pub fn pour_into_vessel(pack: &mut Inventory, from: usize, store: &mut Inventory) -> u32 {
    let Some(source) = pack.slots().get(from).copied().flatten() else {
        return 0;
    };
    // Something in the slot that is not goods a jug would recognise -- a
    // mod's forty axes -- is not topped up and not swapped: it has to come
    // out first, which is a gesture the player can see.
    let occupied = store.slots().get(VESSEL_SLOT).copied().flatten().is_some();
    let inside = vessel_store_contents(store);
    if occupied && inside.is_none() {
        return 0;
    }
    let moved = jug_room(inside, source.block).min(source.count);
    if moved == 0 {
        return 0;
    }
    let left = store.put_in_slot(VESSEL_SLOT, Stack::new(source.block, moved));
    debug_assert!(left.is_none(), "a jug with room refused what it had room for");
    // Only what actually landed leaves the pack, so even the case the
    // assertion says cannot happen costs nothing rather than a handful.
    let landed = moved - left.map_or(0, |left| left.count);
    pack.take_from(from, landed)
}

/// A jug with `count` units of `block` in it.
///
/// The only place the encoding is written, so no caller does the
/// shifting. `count` is clamped to `JUG_UNITS`; a count of zero, or a
/// block of air, gives a plain empty jug back rather than a jug holding
/// nothing-in-particular.
pub fn filled_jug(block: BlockId, count: u32) -> Stack {
    let count = count.min(JUG_UNITS);
    if block == crate::types::BLOCK_AIR || count == 0 {
        return Stack::new(crate::types::BLOCK_JUG, 1);
    }
    Stack::worn(
        crate::types::BLOCK_JUG,
        1,
        (count << BlockId::BITS) | block as u32,
    )
    // The count goes in one byte and the top byte stays clear for
    // `quality`; `jug_contents` reads it back with the same mask.
}

/// What a player has on, as opposed to what they are carrying.
///
/// ## Why this is not four more slots in `Inventory`
///
/// It nearly was, and the reason it is not comes down to what every
/// other piece of code does with a slot index. `Inventory` has `SLOTS`
/// squares that are all the same kind of square: anything goes in any of
/// them, `quick_move` walks them looking for room, `sort_all` reorders
/// them freely, `add` drops a stack in the first one with space. Every
/// one of those would have had to learn about four squares at the end
/// where none of it applies -- a cuirass does not merge with another
/// cuirass, a sort must not move it, and `add` putting a pickup into the
/// helmet slot is a bug you would only find by wearing a lump of dirt.
///
/// So the equipment is its own small container with its own rules, and
/// the rule is: **one slot per body part, and the slot a garment goes in
/// is a fact about the garment** (see [`crate::equipment::slot_of`]).
/// There is no arranging to do, which is why there is no move gesture
/// here and only a wear and a take.
///
/// ## Wear
///
/// Garments wear out the way tools do, on the same `damage` field of the
/// same `Stack` -- because it is the same thing happening. What differs
/// is when: a tool is charged for a swing it makes, a garment for a blow
/// it stops. A piece that runs out is *gone*, exactly as a tool is: a
/// cuirass in three pieces is not a wearable item, and leaving one in
/// the slot would mean a player is protected by something that has
/// stopped protecting them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Equipment {
    /// Indexed by `equipment::Slot::index`. A `Vec` rather than an array
    /// for the reason `Inventory::slots` is one: it comes off a socket
    /// and out of a save file, and a fixed-size array would make a file
    /// written when there were three slots unreadable rather than
    /// repairable. See `sanitize`.
    worn: Vec<Option<Stack>>,
}

impl Default for Equipment {
    fn default() -> Self {
        Self::new()
    }
}

impl Equipment {
    pub fn new() -> Self {
        Self {
            worn: vec![None; crate::equipment::SLOTS],
        }
    }

    /// Repairs a set that arrived over the wire or out of an old save.
    ///
    /// Three things can be wrong with one, and all three have to be
    /// survivable rather than fatal: the wrong number of slots, a
    /// garment in a slot it does not belong in, and a stack of forty
    /// helmets. The first is a version difference; the other two are a
    /// client saying something impossible, and the answer to that is to
    /// put whatever it was back to nothing rather than to trust it.
    pub fn sanitize(&mut self) {
        self.worn.resize(crate::equipment::SLOTS, None);
        for (index, slot) in self.worn.iter_mut().enumerate() {
            let Some(stack) = slot else { continue };
            let belongs = crate::equipment::slot_of(stack.block)
                .is_some_and(|s| s.index() == index);
            if !belongs || stack.count == 0 {
                *slot = None;
                continue;
            }
            // One to a slot, always: a garment is a thing you have on,
            // and there is no reading of "three tunics worn" that means
            // anything.
            stack.count = 1;
            // Wear against *this* piece's life rather than the kind's, and
            // the top byte kept in either arm: see the pack's `sanitize`
            // for what clearing the whole word costs.
            if stack.life().is_some() {
                if stack.is_broken() {
                    *slot = None;
                }
            } else {
                stack.damage &= !crate::quality::WEAR_MASK;
            }
        }
    }

    /// What is in each slot, in slot order.
    pub fn slots(&self) -> &[Option<Stack>] {
        &self.worn
    }

    /// What is on a particular body part.
    pub fn in_slot(&self, slot: crate::equipment::Slot) -> Option<Stack> {
        self.worn.get(slot.index()).copied().flatten()
    }

    pub fn is_empty(&self) -> bool {
        self.worn.iter().all(|slot| slot.is_none())
    }

    /// Puts a garment on, and hands back whatever it displaced.
    ///
    /// `None` back means the slot was empty; `Some` means the caller now
    /// owns the piece that came off and has to do something with it --
    /// which is the whole reason this returns rather than dropping it.
    /// Anything that is not a garment is refused by being handed
    /// straight back, so a client asking to wear a lump of dirt gets
    /// its lump of dirt.
    pub fn wear(&mut self, stack: Stack) -> Option<Stack> {
        let Some(slot) = crate::equipment::slot_of(stack.block) else {
            return Some(stack);
        };
        let mut worn = stack;
        worn.count = 1;
        let displaced = self.worn[slot.index()].replace(worn);
        // Whatever was over one is not worn and has to go back to the
        // caller with the displaced piece -- but a garment stacks to one
        // (see `blocks::BlockDef::stack`), so this is belt and braces
        // rather than a case anybody reaches.
        debug_assert!(stack.count <= 1, "a garment arrived in a stack");
        displaced
    }

    /// Takes a piece off.
    pub fn take(&mut self, slot: crate::equipment::Slot) -> Option<Stack> {
        self.worn.get_mut(slot.index()).and_then(|s| s.take())
    }

    /// How far this set carries its wearer's smell: the rankest piece on
    /// (`equipment::reek`), not a sum -- a tarred coat is smelled as far as
    /// it is smelled, whatever else is worn with it.
    pub fn reek(&self) -> f32 {
        self.worn.iter().flatten().map(|stack| crate::equipment::reek(stack.block)).fold(1.0, f32::max)
    }

    /// Are snowshoes on the feet? What the physics asks of a drift
    /// (`types::surface_drag_shod`).
    pub fn snowshoes(&self) -> bool {
        self.in_slot(crate::equipment::Slot::Feet)
            .is_some_and(|stack| crate::types::block_kind(stack.block) == crate::types::BLOCK_SNOWSHOES)
    }

    /// What the whole set is worth. See [`crate::equipment::Worn`].
    pub fn worn(&self) -> crate::equipment::Worn {
        let mut pieces = [None; crate::equipment::SLOTS];
        for (index, slot) in self.worn.iter().enumerate() {
            if index >= pieces.len() {
                break;
            }
            pieces[index] = slot.and_then(|stack| {
                let mut garment = crate::equipment::garment(stack.block)?;
                // **Quality turns a blow; it does not keep you warm.** A
                // well-cut jerkin sits where the blow lands, and that is
                // craft. Wool is warm because it is wool, and a fine
                // tunic that also insulated better would make the north
                // a thing you solve at a bench instead of a thing you
                // dress for -- which is the decision `equipment` exists
                // to create. So only `protection` is scaled.
                garment.protection *= crate::quality::protection_scale(stack.quality());
                Some(garment)
            });
        }
        crate::equipment::Worn::total(pieces)
    }

    /// What wearing all this costs to carry.
    ///
    /// Counted into the same total the pack goes through, so armour
    /// makes a fall worse by exactly the mechanism a heavy pack does --
    /// see `crate::load`. A second weight system for worn things would
    /// be two answers to one question.
    pub fn weight(&self) -> f32 {
        // `+ 0.0` for the reason `Inventory::total_weight` gives: an
        // empty float sum in Rust is *negative* zero, and `{:.0}` prints
        // its sign.
        self.worn.iter().flatten().map(|stack| stack.weight()).sum::<f32>() + 0.0
    }

    /// Charges the set for stopping a blow, and says what broke.
    ///
    /// **Spread over the slots that are filled**, rather than charging
    /// the one that was "hit": there is no hit location in this game,
    /// the damage maths is an expected value over `hit_share`, and
    /// picking a slot to damage would mean inventing a die roll that
    /// nothing else uses. What a player sees is a set that wears evenly,
    /// which is also what happens to a set that is worn every day.
    ///
    /// One point of wear per blow per piece, and only for a blow that
    /// something actually stopped -- so being rained on and starving are
    /// free, and a fight is not.
    pub fn take_a_blow(&mut self) -> Vec<(crate::equipment::Slot, Wear)> {
        let mut broken = Vec::new();
        for index in 0..self.worn.len() {
            let Some(slot) = crate::equipment::Slot::from_index(index) else {
                continue;
            };
            let Some(stack) = self.worn[index].as_mut() else {
                continue;
            };
            // **`life` and `wear`, never the raw word** -- this is the
            // site `Stack::wear`'s note is about, and it was the raw word
            // until the audit of 18.09. `damage` carries how well the
            // piece was made in its top byte (`quality::QUALITY_SHIFT`),
            // and every one of the fourteen garments that has a life in
            // it -- all the leather, all the bronze, all the iron, the
            // hood and the cloak -- is judged when it is made, because
            // `quality::takes_quality` reads off exactly this table. So a
            // cuirass somebody forged carried at least sixteen million in
            // that word against a durability of a thousand, and **the
            // first blow that landed destroyed every piece being worn, in
            // silence.** Going through `life` fixes the second half of it
            // too: a finely made piece is supposed to last longer
            // (`quality::durability_scale`), and comparing against the
            // kind's flat durability threw that away.
            let Some(total) = stack.life() else {
                continue;
            };
            stack.damage += 1;
            if stack.wear() >= total {
                self.worn[index] = None;
                broken.push((slot, Wear::Broke));
            } else {
                broken.push((slot, Wear::Worn));
            }
        }
        broken
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

    /// A box in the world: a chest, a corpse, a hearth, a drying rack.
    ///
    /// Its own constructor rather than `new`, because a container's size
    /// stopped being the player's the day the pack was halved -- see
    /// [`CHEST_SLOTS`]. A hearth and a rack use short ranges inside the
    /// same forty squares, exactly as they did when every container in
    /// the game was this long.
    pub fn chest() -> Self {
        Self {
            slots: vec![None; CHEST_SLOTS],
        }
    }

    /// A dead player's body: a chest's forty, and a rucksack's compartment
    /// after them when one was worn -- see [`CORPSE_COMPARTMENT`].
    pub fn body(with_compartment: bool) -> Self {
        Self {
            slots: vec![None; if with_compartment { CORPSE_SLOTS } else { CHEST_SLOTS }],
        }
    }

    /// Whether this container -- asked of a body -- has a rucksack's
    /// compartment. Read off the length, like [`Inventory::backpack_open`]
    /// and for the same reason: a flag beside it is a second copy of one
    /// fact.
    pub fn has_compartment(&self) -> bool {
        self.slots.len() >= CORPSE_SLOTS
    }

    /// Repairs an inventory that arrived over the wire, or out of a save
    /// written by an older build.
    ///
    /// Slot counts change between versions and stack limits get retuned,
    /// so the shape of what arrives is not this type's to assume.
    pub fn sanitize(&mut self) {
        // **This grows and it does not shrink**, and the change from a
        // flat `resize(SLOTS, None)` is the most dangerous line in this
        // file's history.
        //
        // One `Inventory` type is four different containers: a player's
        // pack (`SLOTS`), a pack with a rucksack on it (`MAX_SLOTS`), a
        // chest or a corpse (`CHEST_SLOTS`) and a hearth or a rack,
        // which use their own short ranges. Every one of them goes
        // through here on load and off the wire. While `SLOTS` was forty
        // they all happened to agree, so resizing to `SLOTS` was
        // invisible -- and the moment the pack was halved that same line
        // became "delete the back half of every chest in the world on
        // the next start-up".
        //
        // So: pad a short one up to a player's own squares, which is the
        // old-save repair this was written for; refuse a preposterous
        // one, which is the socket defence; and otherwise leave the
        // length alone, because the container knows its own size and
        // this function does not know which container it is looking at.
        // Reconciling a *player's* pack with whether a rucksack is
        // actually worn is `fit_to_backpack`, which is called where that
        // is known.
        if self.slots.len() < SLOTS {
            self.slots.resize(SLOTS, None);
        } else if self.slots.len() > LARGEST_CONTAINER {
            // The largest thing there is, not a chest: a pack of forty
            // with a rucksack on it is sixty, and clamping to a chest's
            // forty emptied every worn rucksack on load the day the pack
            // grew back.
            self.slots.truncate(LARGEST_CONTAINER);
        }
        for slot in &mut self.slots {
            match slot {
                Some(stack) if stack.count == 0 => *slot = None,
                Some(stack) => {
                    stack.count = stack.count.min(stack_limit(stack.block));
                    // Wear on something that cannot wear out is a number
                    // off a wire or an old save, and it would draw a
                    // damage bar under a lump of coal.
                    //
                    // **A jug is the exception, and forgetting it was a
                    // jug emptied by every round trip.** A jug has no
                    // durability, so the arm below used to clear the
                    // field -- and the field is where its contents live
                    // (see `jug_contents`). Every snapshot off the wire
                    // and every load off disk goes through here, so a
                    // jug of grain arrived as an empty jug. It is
                    // re-encoded rather than merely kept, so a number
                    // `jug_contents` will not read -- a count past
                    // `JUG_UNITS`, an id of air -- is written back as a
                    // plainly empty jug instead of being left in the
                    // field for something later to read differently.
                    //
                    // **Quality is the third meaning of the field and it
                    // survives all three arms**, which is why every one
                    // of them now rebuilds the word instead of assigning
                    // to it. The first cut of this cleared the low bits
                    // with a plain `= 0` and took the top byte with them,
                    // so a fine loaf came back out of a chest unmarked --
                    // a save round trip that quietly un-made the thing.
                    let quality = stack.quality();
                    let low = match stack.life() {
                        Some(total) => stack.wear().min(total),
                        None if crate::types::block_kind(stack.block)
                            == crate::types::BLOCK_JUG =>
                        {
                            match jug_contents(stack) {
                                Some((block, count)) => filled_jug(block, count).wear(),
                                None => 0,
                            }
                        }
                        None => 0,
                    };
                    stack.damage =
                        low | (u32::from(quality.byte()) << crate::quality::QUALITY_SHIFT);
                }
                None => {}
            }
        }
    }

    pub fn slots(&self) -> &[Option<Stack>] {
        &self.slots
    }

    /// Whether a rucksack's squares are here.
    ///
    /// Read off the length rather than kept as a flag, because a flag is
    /// a second copy of the same fact and the one that goes stale is
    /// always the flag. The server sets the length when the rucksack
    /// goes on or comes off; every other reader -- the pack screen, the
    /// weight sum, the death drop -- just sees ten more squares.
    pub fn backpack_open(&self) -> bool {
        self.slots.len() >= MAX_SLOTS
    }

    /// Opens a rucksack's squares. Idempotent.
    ///
    /// Called by the server when something lands in
    /// `equipment::Slot::Back`, and by nothing else: the client is never
    /// the authority on how many squares a player has, or a client could
    /// grant itself a pack.
    pub fn open_backpack(&mut self) {
        if !self.backpack_open() {
            self.slots.resize(MAX_SLOTS, None);
        }
    }

    /// Takes a rucksack's squares away again, and says whether it could.
    ///
    /// **Refused while anything is still in them**, which is the rule
    /// that makes the whole design safe: the alternative is deciding at
    /// this moment where ten stacks go, and every answer is bad -- onto
    /// the ground is a player who takes a pack off indoors and loses it
    /// through the floor, into the pack proper needs room that is not
    /// there, and deleting them is deleting them. Refusing costs the
    /// player one message and an obvious action ("empty it first"), and
    /// it means there is no path in this code where a stack has nowhere
    /// to be.
    /// Makes a player's pack the length the worn set says it should be,
    /// and hands back whatever would not fit.
    ///
    /// **The load path's job, not `sanitize`'s.** `sanitize` sees an
    /// `Inventory` and cannot tell a pack from a chest, so it leaves
    /// lengths alone (see the note there); this is called where the worn
    /// set is in hand and the container is known to be somebody's pack.
    ///
    /// It matters for exactly one save: one written when the pack was
    /// forty squares. Twenty of those squares no longer exist, and what
    /// was in them is moved into whatever is free -- which for almost
    /// every real pack is all of it, because a pack that was genuinely
    /// full of forty different stacks is a pack that was about to be
    /// emptied into a chest anyway. What still does not fit comes back
    /// here rather than being dropped on the floor of this function: the
    /// caller is the one that knows whether there is a world to put it
    /// in.
    #[must_use]
    pub fn fit_to_backpack(&mut self, worn: bool) -> Vec<Stack> {
        let target = if worn { MAX_SLOTS } else { SLOTS };
        if self.slots.len() <= target {
            self.slots.resize(target, None);
            return Vec::new();
        }
        let overflow: Vec<Stack> = self.slots.drain(target..).flatten().collect();
        let mut left = Vec::new();
        for stack in overflow {
            // `add_worn` rather than `add`, so a half-spent axe that has
            // to move does not come out of the move repaired.
            let over = self.add_worn(stack.block, stack.count, stack.damage);
            if over > 0 {
                left.push(Stack::worn(stack.block, over, stack.damage));
            }
        }
        left
    }

    #[must_use]
    pub fn close_backpack(&mut self) -> bool {
        if !self.backpack_open() {
            return true;
        }
        if self.slots[BACKPACK_RANGE].iter().any(|slot| slot.is_some()) {
            return false;
        }
        self.slots.truncate(SLOTS);
        true
    }

    pub fn block_in(&self, slot: usize) -> Option<BlockId> {
        self.slots.get(slot).copied().flatten().map(|s| s.block)
    }

    /// Changes what the thing in a slot *is*, keeping its wear and its
    /// count.
    ///
    /// **For a change of state rather than a change of item**, and
    /// there is exactly one so far: a spear loses the fly agaric
    /// smeared on it when a thrust lands (`types::unpoisoned`). Taking
    /// the spear out and putting a different one back would throw away
    /// the wear on it, which is a repaired weapon in exchange for a
    /// used poison -- and `add` would refuse anyway, because a spear
    /// stacks to one and the slot is full of the spear being changed.
    ///
    /// Refuses a slot that is empty or already holds that block, so a
    /// caller can ask without checking first.
    pub fn retype_slot(&mut self, slot: usize, block: BlockId) -> bool {
        let Some(Some(stack)) = self.slots.get_mut(slot) else {
            return false;
        };
        if stack.block == block {
            return false;
        }
        stack.block = block;
        true
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
    ///
    /// **By kind, not by id.** Food carries its age in the variant
    /// field (see `food::rot_stage`), so a pack with three cuts of
    /// yesterday's meat holds three stacks whose ids are not
    /// `BLOCK_RAW_MEAT` -- and this used to answer "none", which meant
    /// the cooking recipe went grey two and a half minutes after a
    /// kill, at exactly the moment the player most wanted it. Merging
    /// (`add`) still matches the exact id, deliberately: stacks of
    /// different ages are different things and stay in different slots.
    pub fn count(&self, block: BlockId) -> u32 {
        let kind = crate::types::block_kind(block);
        self.slots
            .iter()
            .flatten()
            .filter(|s| crate::types::block_kind(s.block) == kind)
            .map(|s| s.count)
            .sum()
    }

    pub fn total_items(&self) -> u32 {
        self.slots.iter().flatten().map(|s| s.count).sum()
    }

    /// What the whole load weighs, in kilograms.
    pub fn total_weight(&self) -> f32 {
        // **The `+ 0.0` is not decoration.** Rust's `Sum for f32` folds
        // from **negative** zero, deliberately: `-0.0 + x == x` for
        // every `x` including `-0.0`, where `0.0 + -0.0` would be `0.0`
        // and lose the sign. The consequence here is that an *empty*
        // pack weighed `-0.0`, and `{:.0}` prints the sign of a zero --
        // so every empty container in the game told the player it
        // weighed `-0 kg`.
        //
        // Adding positive zero is the fix and is exactly a no-op for
        // every other value: `-0.0 + 0.0` is `+0.0`, and `x + 0.0` is
        // `x` for all finite `x`.
        self.slots.iter().flatten().map(|s| s.weight()).sum::<f32>() + 0.0
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
            // The same block, and room for some of it: merge.
            //
            // No room means nothing happens and the caller is told so --
            // including the case that arrived with tools, where the
            // limit is one (see `blocks::BlockDef::stack`). Dragging one
            // axe onto another is a gesture with no outcome: both slots
            // hold an axe before and after, and swapping them would be a
            // packet and a redraw to change nothing anybody can see.
            Some(target) if target.block == source.block => {
                let limit = stack_limit(target.block);
                let moved = (limit - target.count.min(limit)).min(source.count);
                if moved == 0 {
                    return false;
                }
                // `worn`, not `new`: rebuilding the target with a fresh
                // stack threw its `damage` away, and a fresh stack of
                // anything durable is a *repaired* one. Nothing durable
                // stacks past one so this branch never merges two worn
                // things, but every other rebuild in this file made the
                // same mistake and three of them were reachable.
                self.slots[to] = Some(Stack::worn(
                    target.block,
                    target.count + moved,
                    target.damage,
                ));
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
        let limit = stack_limit(source.block);
        let room = match self.slots[to] {
            None => limit,
            Some(target) if target.block == source.block => {
                limit - target.count.min(limit)
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
            // The wear travels with the item. A tool stacks to one, so
            // "half of it" is the whole of it moved to an empty slot --
            // and building that slot with `Stack::new` handed the player
            // a brand-new axe for a right-click drag.
            slot @ None => *slot = Some(Stack::worn(source.block, taken, source.damage)),
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
            let limit = stack_limit(source.block);
            if target.block != source.block || target.count >= limit {
                continue;
            }
            let moved = (limit - target.count).min(left);
            self.slots[index] = Some(Stack::worn(
                target.block,
                target.count + moved,
                target.damage,
            ));
            left -= moved;
        }
        if left > 0 {
            if let Some(empty) = target_range.find(|&i| self.slots[i].is_none()) {
                // Carrying `source.damage` across: shift-clicking a
                // half-worn pickaxe out of the bar used to land a new one
                // in storage, which is a free repair for one keystroke.
                self.slots[empty] = Some(Stack::worn(source.block, left, source.damage));
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
                let fits = stack.count.min(stack_limit(stack.block));
                *target = Some(Stack::worn(stack.block, fits, stack.damage));
                (stack.count > fits)
                    .then(|| Stack::worn(stack.block, stack.count - fits, stack.damage))
            }
            Some(held) if held.block == stack.block => {
                let room = stack_limit(stack.block).saturating_sub(held.count);
                let moved = room.min(stack.count);
                held.count += moved;
                (moved < stack.count)
                    .then(|| Stack::worn(stack.block, stack.count - moved, stack.damage))
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
    /// **The rucksack is tidied separately, not swept in with the
    /// pile.** One pass over `HOTBAR_SLOTS..len` would fold the pack and
    /// the rucksack into one run of stacks and leave the tail empty --
    /// which is tidy, and also means pressing the sort button empties
    /// the rucksack into the pack, or fills it from the pack, depending
    /// only on how much there was. Either is the player's arrangement
    /// undone by a convenience, and one of them makes the rucksack
    /// impossible to take off (see `close_backpack`) for no reason the
    /// player can see. Two passes over two ranges keep every stack on
    /// the side of the seam it was on.
    pub fn sort_storage(&mut self) -> bool {
        let pack = self.tidy_range(HOTBAR_SLOTS..SLOTS.min(self.slots.len()));
        let sack = if self.backpack_open() {
            self.tidy_range(BACKPACK_RANGE)
        } else {
            false
        };
        pack || sack
    }

    /// The same, over **every** slot.
    ///
    /// What a chest wants: its forty are forty of the same thing, and
    /// leaving the first ten alone -- which is right for a pack, where
    /// they are the belt and the player arranged them -- would tidy
    /// three quarters of a chest and leave a quarter scattered.
    ///
    /// **Two passes on a body with a rucksack's compartment**, for the
    /// reason `sort_storage` gives for a worn one: one pass over all sixty
    /// would pour the rucksack into the body's forty, and the whole point of
    /// the compartment is that a player finds their rucksack's contents
    /// where they left them. On anything forty long the second range is
    /// empty and this is the one pass it always was.
    pub fn sort_all(&mut self) -> bool {
        let body = self.tidy_range(0..CHEST_SLOTS.min(self.slots.len()));
        let compartment = self.tidy_range(CHEST_SLOTS..self.slots.len());
        body || compartment
    }

    /// Folds part-stacks together inside one range of slots.
    ///
    /// A range rather than "from here to the end", because the pack and
    /// a worn rucksack are two piles in one index space and a sort must
    /// not carry anything across the seam -- see `sort_storage`.
    fn tidy_range(&mut self, range: std::ops::Range<usize>) -> bool {
        let first = range.start;
        let end = range.end.min(self.slots.len());
        if end <= first {
            return false;
        }
        let before: Vec<Option<Stack>> = self.slots[first..end].to_vec();

        // One total per (block, wear), laid back out in block order, so
        // the same pack always tidies to the same place.
        //
        // **The wear is part of the key, not a detail to drop.** This
        // used to total by block alone and rebuild with `Stack::new`,
        // which meant sorting a chest of half-spent pickaxes handed them
        // all back as new -- a repair bench you operate by pressing the
        // sort button. Nothing durable stacks past one, so grouping by
        // wear as well never splits a stack that used to merge: for
        // everything that *does* stack the damage is zero (`sanitize`
        // guarantees it) and the grouping is exactly the old one.
        let mut totals: Vec<(BlockId, u32, u32)> = Vec::new();
        for stack in before.iter().flatten() {
            match totals
                .iter_mut()
                .find(|(block, _, damage)| *block == stack.block && *damage == stack.damage)
            {
                Some((_, count, _)) => *count += stack.count,
                None => totals.push((stack.block, stack.count, stack.damage)),
            }
        }
        totals.sort_unstable_by_key(|&(block, _, damage)| (block, damage));

        let mut index = first;
        let mut left_over = 0u32;
        for (block, mut count, damage) in totals {
            while count > 0 {
                if index >= end {
                    left_over += count;
                    break;
                }
                let moved = count.min(stack_limit(block));
                self.slots[index] = Some(Stack::worn(block, moved, damage));
                count -= moved;
                index += 1;
            }
        }

        if left_over > 0 {
            // Cannot happen -- folding part-stacks never needs more
            // slots than it started with -- but the alternative to
            // checking is deleting whatever did not fit, and that is not
            // a bug worth risking for a comparison.
            self.slots[first..end].clone_from_slice(&before);
            return false;
        }
        for slot in &mut self.slots[index..end] {
            *slot = None;
        }
        self.slots[first..end] != before[..]
    }

    /// Puts blocks in, topping up part-filled stacks before claiming a
    /// new slot.
    ///
    /// Returns however many did **not** fit. Callers that can put the
    /// remainder back in the world -- a pickup that overflows, say --
    /// need to know, and silently swallowing it is how items vanish.
    pub fn add(&mut self, block: BlockId, amount: u32) -> u32 {
        self.add_worn(block, amount, 0)
    }

    /// The same, for something that has already been used.
    ///
    /// Every caller that has a whole `Stack` in its hand must come
    /// through here rather than through `add`, because `add` puts a
    /// pristine item in the slot: a worn axe shift-clicked into a chest
    /// came out of the chest as a new one. Wear is a fact about the
    /// object, and moving an object must not change any fact about it.
    pub fn add_worn(&mut self, block: BlockId, amount: u32, damage: u32) -> u32 {
        let mut left = amount;

        // Top up existing stacks first. Filling a new slot while an old
        // one sits one short is how an inventory ends up looking full
        // while holding almost nothing.
        let limit = stack_limit(block);
        for stack in self.slots.iter_mut().flatten() {
            if left == 0 {
                return 0;
            }
            if stack.block != block || stack.count >= limit || stack.damage != damage {
                continue;
            }
            let moved = (limit - stack.count).min(left);
            stack.count += moved;
            left -= moved;
        }

        while left > 0 {
            let Some(empty) = self.slots.iter_mut().find(|s| s.is_none()) else {
                return left;
            };
            let moved = left.min(limit);
            *empty = Some(Stack::worn(block, moved, damage));
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
        // By kind, like `count`: a recipe that could see old meat and
        // not spend it would report ready and then refuse.
        let kind = crate::types::block_kind(block);
        let mut left = amount;
        for index in 0..self.slots.len() {
            if left == 0 {
                break;
            }
            if self.slots[index].map(|s| crate::types::block_kind(s.block)) != Some(kind) {
                continue;
            }
            left -= self.take_from(index, left);
        }
        left == 0
    }

    // ---- a window onto part of the slots ----
    //
    // **A hearth is an inventory with roles in it.** Its ingredients,
    // its fuel and its results live in one slot list (see
    // `crate::hearth` for why it shares the container store with the
    // chests), and every question it asks is about *part* of that list:
    // what is in the input slots, is there room in the output ones. The
    // whole-inventory versions above answer over all forty, which for a
    // hearth is the difference between a furnace and a furnace that eats
    // its own results.
    //
    // Three functions rather than a `Window` type wrapping a slice: the
    // borrow that a mutable window would need makes every caller a
    // two-step dance, and the range is one argument.

    /// How many of `block` are in these slots.
    pub fn count_within(&self, range: std::ops::Range<usize>, block: BlockId) -> u32 {
        // By kind, for the reason `count` is: a hearth loaded with old
        // meat is a hearth loaded with meat.
        let kind = crate::types::block_kind(block);
        self.slots[range.start.min(self.slots.len())..range.end.min(self.slots.len())]
            .iter()
            .flatten()
            .filter(|stack| crate::types::block_kind(stack.block) == kind)
            .map(|stack| stack.count)
            .sum()
    }

    /// Puts blocks into these slots only, and answers how many did not
    /// fit. Tops up part-stacks before claiming an empty slot, exactly
    /// as `add` does.
    pub fn add_within(
        &mut self,
        range: std::ops::Range<usize>,
        block: BlockId,
        amount: u32,
    ) -> u32 {
        self.add_worn_within(range, block, amount, 0)
    }

    /// The same, for something that has already been used -- the window's
    /// `add_worn`.
    ///
    /// **A player's own stack going into a hearth has to come through
    /// here**, and it used to come through `add_within`, which builds a
    /// pristine stack. So a shift-click put a half-spent pickaxe into a
    /// campfire's ingredient slot as a new one, and the shift-click back
    /// out brought the new one home: a repair bench worked by two clicks
    /// at any fire. The same click emptied a jug of grain, whose contents
    /// live in the same field. Merges only onto the same wear, as `add_worn`
    /// does, so two differently worn things never become one.
    pub fn add_worn_within(
        &mut self,
        range: std::ops::Range<usize>,
        block: BlockId,
        amount: u32,
        damage: u32,
    ) -> u32 {
        let end = range.end.min(self.slots.len());
        let start = range.start.min(end);
        let limit = stack_limit(block);
        let mut left = amount;
        for slot in &mut self.slots[start..end] {
            if left == 0 {
                return 0;
            }
            let Some(stack) = slot else { continue };
            if stack.block != block || stack.count >= limit || stack.damage != damage {
                continue;
            }
            let moved = (limit - stack.count).min(left);
            stack.count += moved;
            left -= moved;
        }
        for slot in &mut self.slots[start..end] {
            if left == 0 {
                return 0;
            }
            if slot.is_some() {
                continue;
            }
            let moved = left.min(limit);
            *slot = Some(Stack::worn(block, moved, damage));
            left -= moved;
        }
        left
    }

    /// Takes up to `amount` of `block` out of these slots, and answers
    /// how many it actually got.
    ///
    /// Partial rather than all-or-nothing, because the caller that
    /// matters (`hearth::complete`) has already checked the count and
    /// wants the removal to be exactly what it asked for; a caller that
    /// has not checked would rather have some than a silent no.
    pub fn take_within(
        &mut self,
        range: std::ops::Range<usize>,
        block: BlockId,
        amount: u32,
    ) -> u32 {
        let end = range.end.min(self.slots.len());
        let start = range.start.min(end);
        // By kind, to match `count_within`, or the hearth would count
        // old meat as an ingredient and then fail to consume it.
        let kind = crate::types::block_kind(block);
        let mut left = amount;
        for slot in &mut self.slots[start..end] {
            if left == 0 {
                break;
            }
            let Some(stack) = slot else { continue };
            if crate::types::block_kind(stack.block) != kind {
                continue;
            }
            let taken = stack.count.min(left);
            stack.count -= taken;
            left -= taken;
            if stack.count == 0 {
                *slot = None;
            }
        }
        amount - left
    }

    /// Spends one swing of the tool in a slot.
    ///
    /// Answers what happened: `Wear::Broke` when that was the last of
    /// it, and the slot is empty afterwards. `Wear::None` for a slot
    /// holding something that does not wear out, which is most of them
    /// -- a player digging with their hands or holding a rock.
    ///
    /// **Here rather than on the server** because both sides have to
    /// agree about when a tool is gone: the server spends the swing, and
    /// the client draws what is left of the bar from the same numbers.
    pub fn wear_tool(&mut self, slot: usize) -> Wear {
        let Some(stack) = self.slots.get_mut(slot).and_then(|s| s.as_mut()) else {
            return Wear::None;
        };
        let Some(total) = stack.life() else {
            return Wear::None;
        };
        stack.damage += 1;
        if stack.wear() < total {
            // ...and the edge, which dulls a step at every boundary of it.
            // Here, on the one path every swing already takes, so the
            // server's block break, its hunting blow and a mod's swing all
            // blunt a tool the same way, and the `Worn` answer they already
            // send the pack on carries the new id to the client. See
            // `tools::after_a_swing`.
            stack.block = crate::tools::after_a_swing(stack.block, stack.wear());
            return Wear::Worn;
        }
        // Gone. Not "a broken tool item": a haft with the head off is
        // firewood, and an inventory slot holding a thing that cannot be
        // used is a slot the player has to clear by hand.
        self.slots[slot] = None;
        Wear::Broke
    }

    /// Sets light to a torch in `slot`, and says whether anything
    /// happened.
    ///
    /// A stack of one, always: a lit torch is `ONE` in the block table,
    /// and lighting the whole pile would be a player striking eight
    /// torches with one gesture. So a stack of several is split -- one
    /// leaves the pile alight and the rest stay where they were.
    /// `false` means there was nothing to light or nowhere to put the
    /// rest, and the caller tells the player nothing, because a gesture
    /// that did not happen has nothing to say.
    pub fn light_torch(&mut self, slot: usize, lit: crate::types::BlockId) -> bool {
        let Some(stack) = self.slots.get(slot).and_then(|s| s.as_ref()) else {
            return false;
        };
        if crate::types::block_kind(stack.block) != crate::types::BLOCK_TORCH {
            return false;
        }
        // A wet wad does not take a flame (`wet`).
        if crate::wet::will_not_light(stack.block) {
            return false;
        }
        if stack.count == 1 {
            let stack = self.slots[slot].as_mut().expect("checked above");
            stack.block = lit;
            stack.damage = 0;
            return true;
        }
        // More than one. The lit torch needs a slot of its own, and if
        // there is none the gesture does not happen -- better than
        // silently lighting the pile or silently dropping a torch.
        let Some(free) = self.slots.iter().position(|s| s.is_none()) else {
            return false;
        };
        let stack = self.slots[slot].as_mut().expect("checked above");
        stack.count -= 1;
        self.slots[free] = Some(Stack { block: lit, count: 1, damage: 0 });
        true
    }

    /// Burns a lit torch down, and hands back the stick when the wad of
    /// fibre is gone.
    ///
    /// **The counter is the tool's**, and `steps` is tenths of a second
    /// rather than swings -- see `types::TORCH_LIFE` for why the two
    /// share a field instead of having one each.
    ///
    /// What comes out is *not* nothing, which is the difference from
    /// `wear_tool`. A worn-out axe is a haft with the head off and the
    /// slot is cleared; a burnt-out torch is a stick with a black end,
    /// and the player still has it -- another wad of fibre makes it a
    /// torch again. A torch that vanished when it went out would be a
    /// stick quietly eaten by the dark.
    ///
    /// Here rather than on the server for the reason `wear_tool` is
    /// here: the client draws what is left of the flame from the same
    /// number the server spends.
    pub fn burn_torch(&mut self, slot: usize, steps: u32) -> TorchBurn {
        let Some(stack) = self.slots.get_mut(slot).and_then(|s| s.as_mut()) else {
            return TorchBurn::NoTorch;
        };
        if !crate::types::is_lit_torch(stack.block) {
            return TorchBurn::NoTorch;
        }
        let Some(total) = crate::types::tool_durability(stack.block) else {
            return TorchBurn::NoTorch;
        };
        stack.damage = stack.damage.saturating_add(steps);
        // `wear`, not the raw word, for `Equipment::take_a_blow`'s reason
        // exactly: the top byte is how well the thing was made. A lit
        // torch only ever comes out of `light_torch`, which happens to
        // clear the word, so this has never put one out on its first tick
        // -- but *happens to* is not a rule, and the day something else
        // hands a lit torch a maker's mark is the day every torch in the
        // world gutters the moment it is struck.
        if stack.wear() < total {
            return TorchBurn::Burning;
        }
        // **The wear goes with the fibre.** A spent torch carries no
        // damage: it is a whole stick again, and the next wad burns for
        // its own full forty-five seconds. Leaving the counter where it
        // was would make every re-wadding shorter than the last, which
        // is a rule nobody asked for and nothing would explain.
        stack.block = crate::types::BLOCK_TORCH_SPENT;
        stack.damage = 0;
        TorchBurn::WentOut
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
        let limit = stack_limit(block);
        let mut room: u32 = 0;
        for slot in &self.slots {
            room += match slot {
                Some(stack) if stack.block == block => limit.saturating_sub(stack.count),
                Some(_) => 0,
                None => limit,
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
    let left = to.put_in_slot(to_slot, Stack::worn(source.block, taken, source.damage));
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
    let left = to.add_worn(source.block, taken.count, taken.damage);
    if left == taken.count {
        // Nowhere for any of it: put it back exactly as it was.
        from.put_in_slot(from_slot, taken);
        return false;
    }
    if left > 0 {
        from.put_in_slot(from_slot, Stack::worn(source.block, left, source.damage));
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{BLOCK_DIRT, BLOCK_STONE};

    #[test]
    fn a_vessel_cannot_be_put_inside_itself_or_another_vessel() {
        use crate::types::{jug_of, BLOCK_JUG, BLOCK_SEEDS};
        use crate::body::Water;
        // The recursion guard is the pouring rule and nothing else, so it
        // is asserted where the rule is: an empty jug, a jug of grain and a
        // jug of water all have no room for a jug of any kind.
        for inside in [None, Some((BLOCK_SEEDS, 3))] {
            for vessel in [BLOCK_JUG, jug_of(Water::Fresh), jug_of(Water::Salt)] {
                assert_eq!(
                    jug_room(inside, vessel),
                    0,
                    "{} went into a jug holding {inside:?}",
                    crate::types::block_name(vessel)
                );
            }
        }
        // ...and through the set-down jug's own door as well.
        let mut pack = Inventory::new();
        pack.put_in_slot(0, filled_jug(BLOCK_SEEDS, 5));
        let mut store = Inventory::new();
        assert_eq!(pour_into_vessel(&mut pack, 0, &mut store), 0);
        assert!(store.is_empty(), "a jug was stored inside a jug");
        assert_eq!(jug_contents(&pack.slots()[0].unwrap()), Some((BLOCK_SEEDS, 5)));
    }

    #[test]
    fn a_vessel_takes_loose_goods_up_to_its_measure_and_only_more_of_the_same() {
        use crate::types::{BLOCK_FLINT_FLAKE, BLOCK_RAW_MEAT, BLOCK_SEEDS};
        assert_eq!(jug_room(None, BLOCK_SEEDS), JUG_UNITS);
        assert_eq!(jug_room(Some((BLOCK_SEEDS, 10)), BLOCK_SEEDS), JUG_UNITS - 10);
        assert_eq!(jug_room(Some((BLOCK_SEEDS, JUG_UNITS)), BLOCK_SEEDS), 0);
        assert_eq!(jug_room(Some((BLOCK_SEEDS, 10)), BLOCK_FLINT_FLAKE), 0, "two kinds in one number");
        // Food stays out for the rot clock's sake -- see `types::pours`.
        assert_eq!(jug_room(None, BLOCK_RAW_MEAT), 0, "a jug became a larder that stops time");
    }

    #[test]
    fn pouring_into_a_set_down_jug_leaves_what_does_not_fit_in_the_pack() {
        use crate::types::{BLOCK_FLINT_FLAKE, BLOCK_SEEDS};
        let mut pack = Inventory::new();
        pack.add(BLOCK_SEEDS, JUG_UNITS + 5);
        pack.add(BLOCK_FLINT_FLAKE, 3);
        let mut store = Inventory::new();

        assert_eq!(pour_into_vessel(&mut pack, 0, &mut store), JUG_UNITS);
        assert_eq!(vessel_store_contents(&store), Some((BLOCK_SEEDS, JUG_UNITS)));
        assert_eq!(pack.count(BLOCK_SEEDS), 5, "seeds were poured into the floor");

        // Full: nothing more of the same, and nothing of anything else.
        assert_eq!(pour_into_vessel(&mut pack, 0, &mut store), 0);
        assert_eq!(pour_into_vessel(&mut pack, 1, &mut store), 0);
        assert_eq!(pack.count(BLOCK_FLINT_FLAKE), 3);
    }

    #[test]
    fn a_store_entry_no_jug_could_hold_is_not_read_as_what_a_jug_holds() {
        // A mod, or a save from a build with a bigger measure, can leave a
        // slot a jug never could have filled. It is neither contents nor
        // somewhere to pour more.
        use crate::types::{BLOCK_SEEDS, BLOCK_STONE_AXE};
        let mut store = Inventory::new();
        store.put_in_slot(VESSEL_SLOT, Stack::new(BLOCK_STONE_AXE, 1));
        assert_eq!(vessel_store_contents(&store), None);
        let mut pack = Inventory::new();
        pack.add(BLOCK_SEEDS, 4);
        assert_eq!(pour_into_vessel(&mut pack, 0, &mut store), 0, "seeds went in on top of an axe");
        assert_eq!(pack.count(BLOCK_SEEDS), 4);

        let mut too_much = Inventory::new();
        too_much.put_in_slot(VESSEL_SLOT, Stack::new(BLOCK_SEEDS, JUG_UNITS + 1));
        assert_eq!(vessel_store_contents(&too_much), None);
    }

    #[test]
    fn what_went_into_a_jug_is_what_comes_back_out_of_it() {
        use crate::types::{BLOCK_GRAIN, BLOCK_JUG};
        for units in 1..=JUG_UNITS {
            let jug = filled_jug(BLOCK_GRAIN, units);
            assert_eq!(jug.block, BLOCK_JUG);
            assert_eq!(jug.count, 1, "a jug is one jug however full it is");
            assert_eq!(jug_contents(&jug), Some((BLOCK_GRAIN, units)));
        }
    }

    #[test]
    fn a_jug_holds_no_more_than_it_holds() {
        use crate::types::BLOCK_SAND;
        // Clamped where it is written rather than checked at every call
        // site: a caller that has miscounted gets a full jug, not a jug
        // whose count reads back as something `jug_contents` refuses
        // and therefore as an empty one.
        assert_eq!(
            jug_contents(&filled_jug(BLOCK_SAND, JUG_UNITS * 4)),
            Some((BLOCK_SAND, JUG_UNITS))
        );
        assert_eq!(jug_contents(&filled_jug(BLOCK_SAND, 0)), None);
    }

    #[test]
    fn nothing_but_a_jug_is_read_as_holding_anything() {
        use crate::types::{BLOCK_JUG_WATER, BLOCK_STONE_AXE};
        // A worn axe carries a number in exactly the field a jug's
        // contents live in, and reading it as contents would tell the
        // screen a half-spent axe was full of something.
        assert_eq!(jug_contents(&Stack::worn(BLOCK_STONE_AXE, 1, 40)), None);
        // A full jug of water says what it is in its *id*; the field is
        // spare there and must stay spare.
        assert_eq!(jug_contents(&Stack::new(BLOCK_JUG_WATER, 1)), None);
        assert_eq!(jug_contents(&Stack::new(BLOCK_STONE, 64)), None);
    }

    #[test]
    fn a_jug_of_grain_weighs_the_grain_as_well_as_the_jug() {
        use crate::types::{block_weight, BLOCK_JUG, BLOCK_SAND};
        let empty = Stack::new(BLOCK_JUG, 1).weight();
        let full = filled_jug(BLOCK_SAND, JUG_UNITS).weight();
        assert!(
            (full - empty - block_weight(BLOCK_SAND) * JUG_UNITS as f32).abs() < 1e-4,
            "a jug was a hole in the load: {empty} empty, {full} full"
        );
    }

    #[test]
    fn a_sanitised_jug_still_holds_what_it_held() {
        use crate::types::{BLOCK_ASH, BLOCK_JUG};
        // Every snapshot off the wire and every save off disk goes
        // through `sanitize`, which clears the wear on anything that
        // cannot wear out -- and a jug cannot. Forgetting that emptied
        // every jug in the game once a frame.
        let mut inventory = Inventory::new();
        inventory.put_in_slot(3, filled_jug(BLOCK_ASH, 5));
        inventory.sanitize();
        assert_eq!(inventory.block_in(3), Some(BLOCK_JUG));
        assert_eq!(
            jug_contents(&inventory.slots()[3].unwrap()),
            Some((BLOCK_ASH, 5))
        );

        // ...and a count off the end of the world is written back as a
        // plainly empty jug rather than left in the field. Clamping it
        // up to `JUG_UNITS` was the other option and it is worse: a
        // number nobody wrote is not evidence of sixteen units of ash,
        // and inventing goods out of a corrupt save is a stranger bug
        // than losing them.
        let mut wild = Inventory::new();
        wild.put_in_slot(0, Stack::worn(BLOCK_JUG, 1, (999 << 16) | BLOCK_ASH as u32));
        wild.sanitize();
        assert_eq!(jug_contents(&wild.slots()[0].unwrap()), None);
        // **`wear()` and not `damage`**: the top byte of that word is how
        // well the jug was thrown (`quality`) and was never part of its
        // contents, so clearing the contents must leave it exactly where
        // it is. This assertion read the raw word while `damage` had one
        // meaning; reading it raw now would be asserting that sanitising
        // a jug un-makes it.
        assert_eq!(
            wild.slots()[0].unwrap().wear(),
            0,
            "the nonsense was left in place"
        );
    }

    #[test]
    fn quality_survives_a_save_round_trip() {
        use crate::quality::{Maker, Quality};
        use crate::types::BLOCK_STONE_PICKAXE;
        // A pick somebody made on a good day and has since swung a
        // hundred times: both facts in the one word, both of them out
        // the other side.
        let made = Maker::RESTED.judge(0.8);
        let mut pack = Inventory::new();
        pack.put_in_slot(0, Stack::new(BLOCK_STONE_PICKAXE, 1).with_quality(made));
        for _ in 0..100 {
            pack.wear_tool(0);
        }
        let before = pack.slots()[0].expect("the pick broke");

        // The two round trips an item actually makes: bincode is what
        // the wire, both container stores and the profile store all use.
        let bytes = bincode::serialize(&pack).expect("a pack that will not save");
        let mut back: Inventory = bincode::deserialize(&bytes).expect("a pack that will not load");
        // ...and `sanitize` is what every one of them runs on the way
        // in, which is the step that used to clear the whole word.
        back.sanitize();

        let after = back.slots()[0].expect("the pick did not come back");
        assert_eq!(after.quality(), made, "the save forgot how well it was made");
        assert_eq!(after.wear(), before.wear(), "the save forgot how worn it was");
        assert_eq!(after.life(), before.life(), "the pick came back a different pick");
        assert_ne!(made, Quality::PLAIN, "the test judged nothing");
    }

    #[test]
    fn two_pieces_of_different_quality_do_not_become_one_stack() {
        use crate::quality::Quality;
        use crate::types::BLOCK_DRIED_MEAT;
        // The same rule food's ages live under, and the same reason: two
        // half-stacks made on different days are different things, and
        // merging them would be choosing which maker to forget.
        let fine = Quality::from_fraction(0.9);
        let poor = Quality::from_fraction(0.1);
        let mut pack = Inventory::new();
        for quality in [fine, poor] {
            let stack = Stack::new(BLOCK_DRIED_MEAT, 4).with_quality(quality);
            assert_eq!(pack.add_worn(stack.block, stack.count, stack.damage), 0);
        }
        let filled: Vec<_> = pack.slots().iter().flatten().collect();
        assert_eq!(filled.len(), 2, "the fine meat and the poor meat became one stack");
        // ...and the same quality does merge, or every loaf would be its
        // own square.
        let plain = Stack::new(BLOCK_DRIED_MEAT, 4).with_quality(fine);
        assert_eq!(pack.add_worn(plain.block, plain.count, plain.damage), 0);
        assert_eq!(pack.slots().iter().flatten().count(), 2, "two identical loaves took two squares");
    }

    #[test]
    fn a_better_tool_lasts_longer_than_a_worse_one() {
        use crate::quality::Quality;
        use crate::types::BLOCK_STONE_PICKAXE;
        let swings = |quality: Quality| {
            let mut pack = Inventory::new();
            pack.put_in_slot(0, Stack::new(BLOCK_STONE_PICKAXE, 1).with_quality(quality));
            let mut count = 0;
            while pack.wear_tool(0) == Wear::Worn {
                count += 1;
                assert!(count < 100_000, "a pick that never wears out");
            }
            count
        };
        let fine = swings(Quality::from_fraction(1.0));
        let poor = swings(Quality::from_fraction(0.0));
        let unjudged = swings(Quality::PLAIN);
        assert!(fine > poor * 2, "a fine pick took {fine} swings and a poor one {poor}");
        // ...and an old pick out of an old chest is the pick it always
        // was: every multiplier in `quality` is one at the middle.
        assert!(poor < unjudged && unjudged < fine, "an unjudged pick is not the middle one");
    }

    #[test]
    fn two_jugs_of_different_things_never_become_one() {
        use crate::types::{BLOCK_GRAIN, BLOCK_SAND};
        // The reason a jug stacks to one. Both are `BLOCK_JUG`, so
        // every merge in this file matches them by block -- and a merge
        // that succeeded would leave one slot, one `damage`, and one of
        // the two sets of contents gone.
        let mut inventory = Inventory::new();
        inventory.put_in_slot(0, filled_jug(BLOCK_GRAIN, 4));
        inventory.put_in_slot(1, filled_jug(BLOCK_SAND, 9));
        inventory.move_or_merge(0, 1);
        let mut seen: Vec<_> = inventory
            .slots()
            .iter()
            .flatten()
            .filter_map(jug_contents)
            .collect();
        seen.sort_unstable();
        assert_eq!(seen, vec![(BLOCK_SAND, 9), (BLOCK_GRAIN, 4)]);

        // ...and sorting, which folds part-stacks together, must not
        // fold them either.
        inventory.sort_all();
        let mut after: Vec<_> = inventory
            .slots()
            .iter()
            .flatten()
            .filter_map(jug_contents)
            .collect();
        after.sort_unstable();
        assert_eq!(after, seen);
    }

    // ---- the halved pack, and the rucksack that gives half of it back ----

    #[test]
    fn the_pack_is_all_belt_plus_storage_and_a_rucksack_is_half_of_it() {
        // See the note on `STORAGE_ROWS` for why forty. The relations are
        // what this pins, not the number: the belt is still the front of
        // the pack, and the pack is still a whole number of rows.
        let pack = Inventory::new();
        assert_eq!(pack.slots().len(), SLOTS);
        assert_eq!(SLOTS, 40, "the pack changed size without this test being read");
        assert_eq!(SLOTS % HOTBAR_SLOTS, 0, "the pack is not a whole number of rows");
        // In a `const` block: it is a relation between two constants,
        // and the build is where such a relation should fail. Clippy
        // says so too.
        const { assert!(SLOTS > HOTBAR_SLOTS, "there is no storage behind the belt") };
        assert_eq!(BACKPACK_SLOTS, SLOTS / 2, "a rucksack is half a pack");
    }

    #[test]
    fn a_halved_pack_still_stacks_and_swaps_exactly_as_it_did() {
        // The whole of what halving must not change. A stack that
        // overflows claims the next square, a move onto the same block
        // merges, a move onto a different one swaps, and the last square
        // is as usable as the first.
        let mut pack = Inventory::new();
        assert_eq!(pack.add(BLOCK_STONE, MAX_STACK + 3), 0);
        assert_eq!(pack.count_in(0), MAX_STACK);
        assert_eq!(pack.count_in(1), 3);

        let last = SLOTS - 1;
        pack.put_in_slot(last, Stack::new(BLOCK_DIRT, 2));
        assert!(pack.move_or_merge(1, last), "a move onto a different block did nothing");
        assert_eq!(pack.block_in(last), Some(BLOCK_STONE), "the swap did not happen");
        assert_eq!(pack.block_in(1), Some(BLOCK_DIRT));

        pack.put_in_slot(2, Stack::new(BLOCK_STONE, 4));
        assert!(pack.move_or_merge(2, last), "a move onto the same block did nothing");
        assert_eq!(pack.count_in(last), 7, "the two stacks did not merge");
        assert_eq!(pack.count_in(2), 0);

        // ...and nothing has appeared past the end of it.
        assert_eq!(pack.slots().len(), SLOTS);
        assert!(!pack.backpack_open());
    }

    #[test]
    fn the_rucksacks_squares_are_refused_until_a_rucksack_is_on() {
        // The rule the whole design rests on: the squares do not exist,
        // so every path that could touch them -- a move, a split, a
        // swap, a direct put -- fails its own bounds check. Nothing has
        // to remember to ask whether a rucksack is worn.
        let mut pack = Inventory::new();
        pack.add(BLOCK_STONE, 8);
        let first = BACKPACK_RANGE.start;
        assert!(pack.slots().get(first).is_none(), "a square that should not be there");
        assert!(!pack.move_or_merge(0, first), "a stone moved into a rucksack nobody is wearing");
        assert!(!pack.split_into(0, first), "half a stone went into thin air");
        pack.swap(0, first);
        assert_eq!(pack.block_in(0), Some(BLOCK_STONE), "the swap threw the stone away");
        assert_eq!(pack.put_in_slot(first, Stack::new(BLOCK_DIRT, 1)).map(|s| s.count), Some(1),
            "a put past the end swallowed the stack instead of handing it back");
        assert_eq!(pack.count(BLOCK_STONE), 8);

        // ...and with one on, the same square takes the same stone.
        pack.open_backpack();
        assert!(pack.backpack_open());
        assert_eq!(pack.slots().len(), MAX_SLOTS);
        assert!(pack.move_or_merge(0, first), "a worn rucksack still refused a stack");
        assert_eq!(pack.block_in(first), Some(BLOCK_STONE));
    }

    #[test]
    fn a_rucksack_with_anything_in_it_cannot_be_taken_off() {
        // Every other answer loses a stack -- see `close_backpack`, which
        // argues the three that were rejected.
        let mut pack = Inventory::new();
        pack.open_backpack();
        pack.put_in_slot(BACKPACK_RANGE.start, Stack::new(BLOCK_STONE, 1));
        assert!(!pack.close_backpack(), "a full rucksack came off");
        assert_eq!(pack.slots().len(), MAX_SLOTS, "it shortened the pack anyway");
        assert_eq!(pack.block_in(BACKPACK_RANGE.start), Some(BLOCK_STONE));

        pack.take_slot(BACKPACK_RANGE.start);
        assert!(pack.close_backpack(), "an empty rucksack would not come off");
        assert_eq!(pack.slots().len(), SLOTS);
        // ...and taking off what is already off is not an error.
        assert!(pack.close_backpack());
    }

    #[test]
    fn tidying_never_carries_a_stack_across_the_rucksacks_seam() {
        // One pass over everything above the belt would empty the
        // rucksack into the pack, or fill it from the pack, depending
        // only on how much there was -- and the first of those makes the
        // rucksack impossible to take off for no reason a player can
        // see. See `sort_storage`.
        let mut pack = Inventory::new();
        pack.open_backpack();
        pack.put_in_slot(HOTBAR_SLOTS, Stack::new(BLOCK_STONE, 2));
        pack.put_in_slot(HOTBAR_SLOTS + 4, Stack::new(BLOCK_STONE, 3));
        pack.put_in_slot(BACKPACK_RANGE.start + 2, Stack::new(BLOCK_DIRT, 1));
        assert!(pack.sort_storage());
        assert_eq!(pack.count_in(HOTBAR_SLOTS), 5, "the pack's own part-stacks did not fold");
        assert_eq!(
            pack.slots()[SLOTS..].iter().flatten().count(),
            1,
            "the sort moved something across the seam"
        );
        assert_eq!(pack.block_in(BACKPACK_RANGE.start), Some(BLOCK_DIRT), "the rucksack did not fold");
    }

    #[test]
    fn a_pack_off_the_wire_keeps_a_rucksacks_squares_and_a_chests_forty() {
        // `sanitize` runs over every container in the game -- a pack, a
        // chest, a hearth, a rack -- and the flat `resize(SLOTS)` it used
        // to do became "delete the back half of every chest in the
        // world" the day the pack was halved.
        let mut worn = Inventory::new();
        worn.open_backpack();
        worn.put_in_slot(MAX_SLOTS - 1, Stack::new(BLOCK_STONE, 1));
        worn.sanitize();
        assert_eq!(worn.slots().len(), MAX_SLOTS, "a rucksack was emptied by a round trip");
        assert_eq!(worn.block_in(MAX_SLOTS - 1), Some(BLOCK_STONE));

        let mut chest = Inventory::chest();
        chest.put_in_slot(CHEST_SLOTS - 1, Stack::new(BLOCK_DIRT, 1));
        chest.sanitize();
        assert_eq!(chest.slots().len(), CHEST_SLOTS, "a chest shrank to a pack");
        assert_eq!(chest.block_in(CHEST_SLOTS - 1), Some(BLOCK_DIRT));

        // A short one out of an older save is padded, which is what the
        // resize was written for in the first place.
        let mut old: Inventory = bincode::deserialize(&bincode::serialize(&vec![None::<Stack>; 5]).unwrap())
            .map(|slots: Vec<Option<Stack>>| {
                let mut i = Inventory::new();
                for (slot, stack) in slots.into_iter().enumerate() {
                    if let Some(stack) = stack {
                        i.put_in_slot(slot, stack);
                    }
                }
                i
            })
            .expect("a list of empty slots is a readable inventory");
        old.sanitize();
        assert_eq!(old.slots().len(), SLOTS);
    }

    /// **The body's rucksack is kept apart by the tidy button too.** One
    /// pass over sixty squares folds the compartment's stone into the
    /// body's, and then the rucksack a player walks back for is empty and
    /// its contents are somewhere in the forty.
    #[test]
    fn tidying_a_body_leaves_its_rucksack_on_its_own_side_of_the_seam() {
        let mut body = Inventory::body(true);
        assert!(body.has_compartment());
        assert!(!Inventory::chest().has_compartment());
        body.put_in_slot(3, Stack::new(BLOCK_STONE, 5));
        body.put_in_slot(CORPSE_COMPARTMENT.start + 7, Stack::new(BLOCK_STONE, 2));
        body.sort_all();
        assert_eq!(body.slots().len(), CORPSE_SLOTS);
        let compartment: u32 = CORPSE_COMPARTMENT.map(|s| body.count_in(s)).sum();
        assert_eq!(compartment, 2, "the rucksack was tipped into the body");

        // ...and a sanitize on load keeps all sixty.
        body.sanitize();
        assert_eq!(body.slots().len(), CORPSE_SLOTS, "a load emptied a dead player's rucksack");
    }

    #[test]
    fn a_forty_square_save_is_folded_into_the_pack_it_now_has() {
        // The one save shape that needs `fit_to_backpack`: one written
        // when a player could carry forty squares. What fits is kept and
        // what does not comes back to the caller rather than being
        // dropped here.
        let mut old = Inventory::chest();
        old.put_in_slot(CHEST_SLOTS - 1, Stack::new(BLOCK_STONE, 4));
        let lost = old.fit_to_backpack(false);
        assert!(lost.is_empty(), "a nearly empty pack lost something");
        assert_eq!(old.slots().len(), SLOTS);
        assert_eq!(old.count(BLOCK_STONE), 4, "the stack past the end went nowhere");

        // ...and with a rucksack on, there is room for ten more of them.
        let mut worn = Inventory::chest();
        worn.put_in_slot(CHEST_SLOTS - 1, Stack::new(BLOCK_DIRT, 1));
        assert!(worn.fit_to_backpack(true).is_empty());
        assert_eq!(worn.slots().len(), MAX_SLOTS);
    }

    #[test]
    fn a_rucksack_is_worn_on_the_back_and_is_not_a_garment() {
        use crate::equipment::{garment, is_wearable, slot_of, Slot};
        use crate::types::BLOCK_RUCKSACK;
        assert_eq!(slot_of(BLOCK_RUCKSACK), Some(Slot::Back));
        assert!(is_wearable(BLOCK_RUCKSACK));
        // ...and it stops nothing, keeps nothing in and sheds no rain,
        // which is what keeps it out of `body`'s heat balance and out of
        // the combat maths.
        assert!(garment(BLOCK_RUCKSACK).is_none(), "a bag got into the garment table");
        let mut worn = Equipment::new();
        assert!(worn.wear(Stack::new(BLOCK_RUCKSACK, 1)).is_none());
        assert_eq!(worn.in_slot(Slot::Back).map(|s| s.block), Some(BLOCK_RUCKSACK));
        assert_eq!(worn.worn().pieces, 0, "a rucksack counted as a piece of a set");
        assert_eq!(worn.worn().insulation, 0.0);
        assert_eq!(worn.worn().protection, 0.0);
    }

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

    // ---- tools do not stack ----

    /// Every way a slot can be filled, checked against a tool.
    ///
    /// Thirteen call sites used to say `MAX_STACK` and every one of them
    /// was a way to end up with a slot holding a hundred and twenty-eight
    /// axes. A test per path is the only thing that keeps the next one
    /// honest.
    #[test]
    fn a_tool_never_stacks_however_it_arrives_in_a_slot() {
        use crate::types::{stack_limit, BLOCK_STONE_AXE, BLOCK_STONE};
        assert_eq!(stack_limit(BLOCK_STONE_AXE), 1);
        assert_eq!(stack_limit(BLOCK_STONE), MAX_STACK);

        // ...picked up.
        let mut inventory = Inventory::new();
        assert_eq!(inventory.add(BLOCK_STONE_AXE, 5), 0, "five axes did not fit at all");
        for slot in inventory.slots().iter().flatten() {
            assert_eq!(slot.count, 1, "a slot holds more than one axe");
        }
        assert_eq!(inventory.count(BLOCK_STONE_AXE), 5);

        // ...put straight into a slot, as a chest screen does.
        let mut inventory = Inventory::new();
        let left = inventory.put_in_slot(0, Stack::new(BLOCK_STONE_AXE, 4));
        assert_eq!(inventory.count_in(0), 1);
        assert_eq!(left.map(|s| s.count), Some(3), "the rest was swallowed");

        // ...dragged onto another one, which is a gesture with nothing
        // to do: an axe on an axe leaves an axe in each slot.
        let mut inventory = Inventory::new();
        inventory.slots[0] = Some(Stack::new(BLOCK_STONE_AXE, 1));
        inventory.slots[1] = Some(Stack::new(BLOCK_STONE_AXE, 1));
        assert!(!inventory.move_or_merge(0, 1));
        assert_eq!(inventory.count_in(1), 1, "two axes merged into one slot");
        assert_eq!(inventory.count_in(0), 1);

        // ...and dragged onto a *different* block, which still swaps.
        let mut inventory = Inventory::new();
        inventory.slots[0] = Some(Stack::new(BLOCK_STONE_AXE, 1));
        inventory.slots[1] = Some(Stack::new(BLOCK_STONE, 4));
        assert!(inventory.move_or_merge(0, 1));
        assert_eq!(inventory.block_in(1), Some(BLOCK_STONE_AXE));

        // ...shift-clicked across the pack.
        let mut inventory = Inventory::new();
        inventory.slots[0] = Some(Stack::new(BLOCK_STONE_AXE, 1));
        inventory.slots[HOTBAR_SLOTS] = Some(Stack::new(BLOCK_STONE_AXE, 1));
        inventory.quick_move(0);
        assert!(
            inventory.slots().iter().flatten().all(|s| s.count == 1),
            "a quick move stacked two tools"
        );

        // ...and tidied.
        let mut inventory = Inventory::new();
        for slot in HOTBAR_SLOTS..HOTBAR_SLOTS + 3 {
            inventory.slots[slot] = Some(Stack::new(BLOCK_STONE_AXE, 1));
        }
        inventory.sort_storage();
        assert_eq!(inventory.count(BLOCK_STONE_AXE), 3, "tidying lost a tool");
        assert!(
            inventory.slots().iter().flatten().all(|s| s.count == 1),
            "tidying stacked three axes into one slot"
        );
    }

    #[test]
    fn a_tool_wears_out_and_then_is_gone() {
        use crate::types::{tool_durability, BLOCK_STONE_AXE, BLOCK_STONE};
        let total = tool_durability(BLOCK_STONE_AXE).expect("an axe wears out");
        let mut inventory = Inventory::new();
        inventory.add(BLOCK_STONE_AXE, 1);

        for swing in 1..total {
            assert_eq!(inventory.wear_tool(0), Wear::Worn, "swing {swing}");
        }
        let nearly = inventory.slots()[0].expect("still an axe");
        assert!(nearly.condition() < 0.05, "a nearly dead axe read as {}", nearly.condition());

        assert_eq!(inventory.wear_tool(0), Wear::Broke, "the last swing");
        assert!(inventory.block_in(0).is_none(), "a broken tool stayed in the slot");

        // ...and nothing else in the game wears at all.
        let mut rocks = Inventory::new();
        rocks.add(BLOCK_STONE, 4);
        assert_eq!(rocks.wear_tool(0), Wear::None);
        assert_eq!(rocks.count(BLOCK_STONE), 4);
        assert_eq!(rocks.wear_tool(37), Wear::None, "a slot off the end is not a panic");
    }

    #[test]
    fn a_wad_of_fibre_burns_for_forty_five_seconds_and_not_a_tick_longer() {
        // The number the player was promised, checked against the
        // counter that actually spends it. These are two different
        // units -- seconds and hundredths -- and the whole reason
        // `TORCH_LIFE` exists is that they must not drift apart.
        use crate::types::{BLOCK_TORCH_LIT, BLOCK_TORCH_SPENT, TORCH_LIFE, TORCH_SECONDS};
        let mut pack = Inventory::new();
        pack.add(BLOCK_TORCH_LIT, 1);

        // A twenty-hertz server: five hundredths a tick, which is what
        // the tick loop hands in.
        let per_tick = 5;
        let ticks = (TORCH_LIFE / per_tick) as usize;
        for tick in 0..ticks - 1 {
            assert_eq!(
                pack.burn_torch(0, per_tick),
                TorchBurn::Burning,
                "the torch went out on tick {tick} of {ticks}"
            );
        }
        assert_eq!(pack.burn_torch(0, per_tick), TorchBurn::WentOut);
        assert_eq!(
            ticks as f32 / 20.0,
            TORCH_SECONDS,
            "the counter and the promise are different lengths of time"
        );
        // ...and it burns for that long whatever the tick rate is,
        // which is the other half of counting in hundredths of a second
        // rather than in ticks.
        let mut slow = Inventory::new();
        slow.add(BLOCK_TORCH_LIT, 1);
        let per_slow_tick = 20; // five hertz
        for _ in 0..(TORCH_LIFE / per_slow_tick) - 1 {
            assert_eq!(slow.burn_torch(0, per_slow_tick), TorchBurn::Burning);
        }
        assert_eq!(slow.burn_torch(0, per_slow_tick), TorchBurn::WentOut);
        assert_eq!(slow.block_in(0), Some(BLOCK_TORCH_SPENT));
    }

    #[test]
    fn a_torch_that_goes_out_leaves_the_stick_rather_than_nothing() {
        // The difference from `wear_tool`, and it is the whole reason
        // `burn_torch` is its own function: an axe that wears through is
        // gone, and a torch that burns out is a stick a player still has
        // and can re-wad. A slot cleared here would be a stick quietly
        // eaten by the dark.
        use crate::types::{BLOCK_TORCH_LIT, BLOCK_TORCH_SPENT, TORCH_LIFE};
        let mut pack = Inventory::new();
        pack.add(BLOCK_TORCH_LIT, 1);
        assert_eq!(pack.burn_torch(0, TORCH_LIFE), TorchBurn::WentOut);
        assert_eq!(pack.block_in(0), Some(BLOCK_TORCH_SPENT));
        assert_eq!(pack.count_in(0), 1);
        // And the wear went with the fibre: the next wad is a full one.
        assert_eq!(
            pack.slots[0].as_ref().map(|s| s.damage),
            Some(0),
            "a spent torch remembers a burn it no longer has"
        );
    }

    #[test]
    fn lighting_a_pile_of_torches_lights_exactly_one_of_them() {
        // A lit torch is `ONE` in the block table, so a stack of them
        // cannot exist -- and a gesture that lit the whole pile would be
        // a player striking eight torches at once. One leaves the pile,
        // the rest stay where they were.
        use crate::types::{BLOCK_TORCH, BLOCK_TORCH_LIT};
        let mut pack = Inventory::new();
        pack.add(BLOCK_TORCH, 5);
        assert!(pack.light_torch(0, BLOCK_TORCH_LIT));
        assert_eq!(pack.count(BLOCK_TORCH), 4);
        assert_eq!(pack.count(BLOCK_TORCH_LIT), 1);
    }

    #[test]
    fn a_torch_is_not_lit_when_there_is_nowhere_to_put_it() {
        // The refusal is deliberate and it is the safe way round. With
        // the pack full, the choice is between lighting the whole pile
        // and dropping a torch on the floor; doing nothing at all is
        // the only one of the three that cannot lose anything.
        use crate::types::{BLOCK_STONE, BLOCK_TORCH, BLOCK_TORCH_LIT};
        let mut pack = Inventory::new();
        pack.add(BLOCK_TORCH, 5);
        for slot in 1..pack.slots.len() {
            pack.slots[slot] = Some(Stack { block: BLOCK_STONE, count: 1, damage: 0 });
        }
        assert!(!pack.light_torch(0, BLOCK_TORCH_LIT));
        assert_eq!(pack.count(BLOCK_TORCH), 5, "a torch was lost lighting one");
        assert_eq!(pack.count(BLOCK_TORCH_LIT), 0);
    }

    #[test]
    fn nothing_but_a_torch_burns_and_nothing_but_a_ready_one_lights() {
        // Both gestures are asked of every slot the player selects, so
        // both have to be silent about everything else in the game.
        use crate::types::{BLOCK_STONE_AXE, BLOCK_STONE, BLOCK_TORCH_LIT, BLOCK_TORCH_SPENT};
        for block in [BLOCK_STONE, BLOCK_STONE_AXE, BLOCK_TORCH_SPENT] {
            let mut pack = Inventory::new();
            pack.add(block, 1);
            assert_eq!(
                pack.burn_torch(0, 100),
                TorchBurn::NoTorch,
                "{} burned down",
                crate::types::block_name(block)
            );
            assert!(
                !pack.light_torch(0, BLOCK_TORCH_LIT),
                "{} caught fire",
                crate::types::block_name(block)
            );
            assert_eq!(pack.block_in(0), Some(block));
        }
    }

    #[test]
    fn a_fresh_tool_is_whole_and_wear_is_a_fraction_of_it() {
        use crate::types::BLOCK_IRON_PICKAXE;
        let fresh = Stack::new(BLOCK_IRON_PICKAXE, 1);
        assert_eq!(fresh.condition(), 1.0);
        assert!(!fresh.is_broken());
        let total = crate::types::tool_durability(BLOCK_IRON_PICKAXE).unwrap();
        let half = Stack::worn(BLOCK_IRON_PICKAXE, 1, total / 2);
        assert!((half.condition() - 0.5).abs() < 0.02, "{}", half.condition());
        assert!(Stack::worn(BLOCK_IRON_PICKAXE, 1, total).is_broken());
        // A block that is not a tool is always whole, whatever number it
        // is carrying: nothing reads wear off one.
        assert_eq!(Stack::worn(crate::types::BLOCK_STONE, 1, 99).condition(), 1.0);
    }

    #[test]
    fn wear_off_a_wire_is_clamped_rather_than_believed() {
        use crate::types::{tool_durability, BLOCK_STONE_AXE, BLOCK_STONE};
        let mut inventory = Inventory::new();
        inventory.slots[0] = Some(Stack::worn(BLOCK_STONE_AXE, 1, 9_999));
        inventory.slots[1] = Some(Stack::worn(BLOCK_STONE, 4, 7));
        inventory.sanitize();
        assert_eq!(
            inventory.slots()[0].unwrap().damage,
            tool_durability(BLOCK_STONE_AXE).unwrap()
        );
        assert_eq!(inventory.slots()[1].unwrap().damage, 0, "a rock was worn out");
    }

    #[test]
    fn a_stacked_tool_out_of_an_old_save_is_repaired() {
        // Saves written before tools stopped stacking have slots with
        // eight axes in them. `sanitize` is where everything off a disk
        // or a socket is brought back inside the rules -- and it has to
        // clamp to the *block's* limit, or the old slot survives every
        // load for ever.
        use crate::types::BLOCK_STONE_AXE;
        let mut inventory = Inventory::new();
        inventory.slots[0] = Some(Stack::new(BLOCK_STONE_AXE, 8));
        inventory.sanitize();
        assert_eq!(inventory.count_in(0), 1);
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
mod weight_format_tests {
    use super::*;

    #[test]
    fn an_empty_pack_weighs_zero_and_prints_as_zero() {
        // **"-0 kg" was on screen**, on the pack screen and the chest
        // screen, and the format string has no minus in it -- so the
        // number really was a negative zero. `f32` has one, `{:.0}`
        // prints its sign, and every empty container in the game was
        // reporting a negative weight to the player.
        let empty = Inventory::new();
        assert_eq!(format!("{:.0}", empty.total_weight()), "0");
        assert!(empty.total_weight().is_sign_positive());
    }

    #[test]
    fn a_rounded_down_weight_never_prints_a_minus() {
        // The other half: anything light enough to round to zero has to
        // print "0" rather than "-0", and something *does* weigh less
        // than half a kilo -- a flake, a seed, a fibre.
        let mut pack = Inventory::new();
        pack.add(crate::types::BLOCK_FIBER, 1);
        let text = format!("{:.0}", pack.total_weight());
        assert!(!text.starts_with('-'), "a light pack printed {text}");
    }
}

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


#[cfg(test)]
mod wear_travels_with_the_item {
    //! Every gesture that relocates a stack used to rebuild it with
    //! `Stack::new`, which zeroes `damage`. A tool stacks to one, so a
    //! move is always a rebuild -- and every one of these was a repair
    //! bench the player could operate with a drag.

    use super::*;
    use crate::types::BLOCK_STONE_AXE;

    fn worn_axe() -> Stack {
        Stack::worn(BLOCK_STONE_AXE, 1, 5)
    }

    fn pack_with_a_worn_axe(slot: usize) -> Inventory {
        let mut pack = Inventory::new();
        pack.put_in_slot(slot, worn_axe());
        assert_eq!(pack.slots()[slot].map(|s| s.damage), Some(5));
        pack
    }

    #[test]
    fn dragging_a_worn_tool_to_an_empty_slot_does_not_repair_it() {
        // The last square rather than a written-down 20, which is what
        // it was until the pack was halved: a test that names a slot
        // number is a test that fails for the wrong reason the next time
        // `STORAGE_ROWS` moves.
        let mut pack = pack_with_a_worn_axe(0);
        let last = SLOTS - 1;
        assert!(pack.move_or_merge(0, last));
        assert_eq!(pack.slots()[last].map(|s| s.damage), Some(5));
    }

    #[test]
    fn splitting_a_worn_tool_off_does_not_repair_it() {
        let mut pack = pack_with_a_worn_axe(0);
        let last = SLOTS - 1;
        assert!(pack.split_into(0, last));
        assert_eq!(pack.slots()[last].map(|s| s.damage), Some(5));
    }

    #[test]
    fn shift_clicking_a_worn_tool_off_the_bar_does_not_repair_it() {
        let mut pack = pack_with_a_worn_axe(0);
        assert!(pack.quick_move(0));
        let landed = pack.slots().iter().flatten().find(|s| s.block == BLOCK_STONE_AXE);
        assert_eq!(landed.map(|s| s.damage), Some(5));
    }

    #[test]
    fn sorting_a_chest_of_worn_tools_does_not_repair_them() {
        let mut chest = Inventory::new();
        chest.put_in_slot(15, worn_axe());
        chest.put_in_slot(17, Stack::worn(BLOCK_STONE_AXE, 1, 9));
        assert!(chest.sort_all());
        let mut wear: Vec<u32> = chest.slots().iter().flatten().map(|s| s.damage).collect();
        wear.sort_unstable();
        assert_eq!(wear, vec![5, 9], "the sort handed back new tools");
    }

    #[test]
    fn putting_a_worn_tool_in_a_chest_does_not_repair_it() {
        let mut pack = pack_with_a_worn_axe(0);
        let mut chest = Inventory::new();
        assert!(move_between(&mut pack, 0, &mut chest, 0));
        assert_eq!(chest.slots()[0].map(|s| s.damage), Some(5));

        // And back again, by every route across the two.
        assert!(quick_move_between(&mut chest, 0, &mut pack));
        let back = pack.slots().iter().flatten().find(|s| s.block == BLOCK_STONE_AXE);
        assert_eq!(back.map(|s| s.damage), Some(5));
    }

    #[test]
    fn splitting_a_worn_tool_into_a_chest_does_not_repair_it() {
        let mut pack = pack_with_a_worn_axe(0);
        let mut chest = Inventory::new();
        assert!(split_between(&mut pack, 0, &mut chest, 3));
        assert_eq!(chest.slots()[3].map(|s| s.damage), Some(5));
    }
}


#[cfg(test)]
mod armour_wear_tests {
    use super::*;
    use crate::quality::Quality;
    use crate::types::{BLOCK_IRON_CUIRASS, BLOCK_LEATHER_TUNIC};

    /// The bug this is here for: `damage` carries the maker's mark in its
    /// top byte, `take_a_blow` compared that whole word against a
    /// durability of a few hundred, and a cuirass somebody had spent two
    /// days of iron on was gone the first time anything hit them. Every
    /// garment in the game that has a life in it is judged when it is
    /// made, so this was every set of armour anybody ever forged.
    #[test]
    fn a_piece_of_armour_somebody_made_survives_the_first_blow_that_lands() {
        for fraction in [0.0, 0.5, 1.0] {
            let mut kit = Equipment::default();
            kit.wear(Stack::new(BLOCK_IRON_CUIRASS, 1).with_quality(Quality::from_fraction(fraction)));
            let after = kit.take_a_blow();
            assert_eq!(
                after.iter().map(|&(_, wear)| wear).collect::<Vec<_>>(),
                vec![Wear::Worn],
                "a cuirass judged at {fraction} came apart on the first blow"
            );
            assert!(
                kit.in_slot(crate::equipment::Slot::Chest).is_some(),
                "the slot is empty after one blow"
            );
        }
    }

    /// ...and it still wears out, at the end of the life quality gave it.
    /// The other half of the same mistake: comparing against the *kind's*
    /// durability rather than the piece's threw away
    /// `quality::durability_scale`, so a fine tunic and a poor one lasted
    /// exactly as long.
    #[test]
    fn a_finely_made_tunic_takes_more_blows_than_a_poor_one_and_both_wear_out() {
        let blows_until_gone = |fraction: f32| {
            let mut kit = Equipment::default();
            kit.wear(
                Stack::new(BLOCK_LEATHER_TUNIC, 1).with_quality(Quality::from_fraction(fraction)),
            );
            let mut blows = 0u32;
            loop {
                blows += 1;
                let after = kit.take_a_blow();
                assert!(blows < 10_000, "a tunic that never wore out");
                if after.iter().any(|&(_, wear)| wear == Wear::Broke) {
                    return blows;
                }
            }
        };
        let poor = blows_until_gone(0.0);
        let fine = blows_until_gone(1.0);
        assert!(fine > poor, "a fine tunic ({fine}) lasted no longer than a poor one ({poor})");
        // ...and the plain one, which is what every piece in every save
        // written before quality existed reads as, is between them.
        let plain = {
            let mut kit = Equipment::default();
            kit.wear(Stack::new(BLOCK_LEATHER_TUNIC, 1));
            let mut blows = 0;
            loop {
                blows += 1;
                if kit.take_a_blow().iter().any(|&(_, wear)| wear == Wear::Broke) {
                    break blows;
                }
            }
        };
        assert!(poor < plain && plain < fine, "poor {poor}, plain {plain}, fine {fine}");
    }

    /// A lit torch is the other place the raw word was compared against a
    /// durability. Nothing marks one today -- `light_torch` clears the
    /// word -- so this states the property rather than reproducing a
    /// failure: a torch burns for its whole life whatever is in the top
    /// byte.
    #[test]
    fn a_torch_burns_for_its_whole_life_whatever_the_maker_did() {
        let lit = crate::types::BLOCK_TORCH_LIT;
        let life = crate::types::tool_durability(lit).expect("a torch with no life in it");
        let mut pack = Inventory::new();
        pack.add_worn(lit, 1, Stack::new(lit, 1).with_quality(Quality::FINEST).damage);
        assert_eq!(pack.burn_torch(0, 1), TorchBurn::Burning, "a fine torch guttered at once");
        assert_eq!(pack.burn_torch(0, life), TorchBurn::WentOut);
    }
}
