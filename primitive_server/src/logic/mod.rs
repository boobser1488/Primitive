//! The rules: everything the server is authoritative about.
//!
//! | module      | what it decides                                      |
//! |-------------|------------------------------------------------------|
//! | `world`     | what block is where, generation, saving and loading   |
//! | `chunkgen`  | the queue and thread pool that terrain is made on     |
//! | `climate`   | how cold it is where a particular player is standing  |
//! | `shelters`  | which room a player is in, and the warmth it holds    |
//! | `drying`    | hides on racks, and the weather they cure in          |
//! | `survival`  | health, falls, drowning, suffocation, respawn         |
//! | `items`     | dropped stacks, their motion, and who picks them up   |
//! | `containers`| what is inside the chests, and how it is saved        |
//! | `stalls`    | who owns each barter stall, and what it asks          |
//! | `falling`   | sand that has lost its support, and rock somebody dug out from under |
//! | `collapse`  | what a falling block does to whoever is under it      |
//! | `felling`   | cutting the base of a tree brings the tree down       |
//! | `water`     | where the water goes when it has somewhere to go      |
//! | `simulation`| the shape every cell-watching mechanic is written to  |
//! | `anticheat` | whether a client's claimed movement is possible       |
//! | `commands`  | the console and chat commands                         |
//! | `rot`       | food going off, in packs, chests and on the ground    |
//! | `carrion`   | a kill nobody butchered, going the same way           |
//! | `pits`      | pit kilns and charcoal pits: contents and the hour    |
//! | `wildfire`  | wood catching from a fire, smoke in a room, soot, torches |
//! | `profiles`  | what a player had when they last left                 |
//! | `vermin`    | rats: where a player lives, and what gets into the stores |
//! | `plugins`   | scripted hooks into all of the above                  |
//! | `mods`      | native libraries, and the stable API they talk to     |
//!
//! This layer is the reason the client can be wrong without it
//! mattering. A client says where it *thinks* it is and what it *wants*
//! to do; what actually happens is decided here, and the answer is sent
//! back. The rules that both sides need to agree on -- hardness, drops,
//! weight, what may grow on what -- live in `primitive_shared` and are
//! read from there rather than being written twice.

pub mod animals;
#[cfg(feature = "mods")]
pub(crate) mod api_impl;
pub mod anticheat;
pub mod carrion;
pub mod chunkgen;
pub mod climate;
pub mod collapse;
pub mod commands;
pub mod drying;
pub mod peat;
pub mod containers;
pub mod falling;
pub mod felling;
pub mod fire;
pub mod fishing;
pub mod growth;
pub mod items;
#[cfg(feature = "mods")]
#[cfg(feature = "mods")]
pub mod mods;
pub mod pits;
pub mod plugins;
pub mod profiles;
pub mod rafts;
pub mod rng;
pub mod shelters;
pub mod stalls;
pub mod rot;
pub mod simulation;
pub mod smelting;
pub mod snowfall;
pub mod survival;
pub mod vermin;
pub mod water;
pub mod weather;
pub mod weathering;
pub mod wildfire;
pub mod world;

/// The one property no single module can state on its own.
///
/// Three simulations here put entities into one snapshot, and the
/// client keys its table on the id alone. Each of them counts its own
/// entities, so the property is *between* them and there is nowhere
/// else it can be written down.
#[cfg(test)]
mod entity_id_tests {
    use primitive_shared::animals::Species;
    use primitive_shared::protocol::EntityId;
    use primitive_shared::types::{BLOCK_SAND, BLOCK_STONE};
    use std::collections::HashSet;
    use std::time::Instant;

    #[test]
    fn a_falling_block_an_animal_and_a_dropped_stack_never_share_an_id() {
        // Reproduction, and it needs nothing but the first entity each
        // simulation ever makes: all three used to be number one. What
        // the player saw was sand that fell "instantly, or after a
        // pause with no animation, or not at all" -- the block was in
        // the air the whole way down and the client was drawing
        // whatever else owned that number instead. See
        // `protocol::EntitySource`.
        let world = super::falling::tests::TestWorld::default();
        world.put(0, 0, 0, BLOCK_STONE);
        world.put(0, 20, 0, BLOCK_SAND);
        let mut falling = super::falling::FallingBlocks::new();
        falling.on_block_changed(0, 20, 0);
        falling.step(&world, 1.0 / 20.0);

        let mut animals = super::animals::Animals::seeded(7);
        animals.spawn(Species::Deer, (0.0, 20.0, 0.0)).expect("a deer");

        let mut items = super::items::Items::new();
        items.spawn(
            BLOCK_SAND,
            1,
            (0.0, 20.0, 0.0),
            (0.0, 0.0, 0.0),
            None,
            Instant::now(),
        );

        let mut seen: HashSet<EntityId> = HashSet::new();
        let states: Vec<_> = falling
            .entities()
            .iter()
            .map(|e| e.state())
            .chain(items.states())
            .chain(animals.states())
            .collect();
        assert_eq!(states.len(), 3, "the fixture should have one of each");
        for state in states {
            assert!(
                seen.insert(state.id),
                "two entities are replicated as {}: {:?}",
                state.id,
                state.kind
            );
        }
    }
}
