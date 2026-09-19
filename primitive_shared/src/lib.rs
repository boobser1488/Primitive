//! The rules both sides have to agree on.
//!
//! The workspace is four layers plus this one. The client is split into
//! `engine` (the GPU), `net` (the socket), `ui` (what the player reads
//! and presses) and `logic` (the world as the client understands it);
//! the server is split into `net` and `logic`. Each of those directories
//! has a `mod.rs` saying what belongs in it.
//!
//! This crate is what sits underneath all of them, and it exists to
//! answer one class of question exactly once. How long does stone take
//! to break? What does a tuft of grass drop? How much does a stack
//! weigh, what may grow on what, and what does the terrain look like at
//! this seed? Both sides need every one of those answers, and any of
//! them implemented twice will eventually be implemented differently --
//! at which point a player mining normally trips the server's
//! anti-cheat, or the client draws a world the server does not have.
//!
//! | module      | what it settles                                     |
//! |-------------|-----------------------------------------------------|
//! | `types`     | what a block is, and everything that follows from it |
//! | `worldgen`  | the terrain, biomes and climate a seed produces      |
//! | `lighting`  | how light travels, and the map it fills in           |
//! | `packed`    | how a chunk and its light are kept in memory, a section at a time |
//! | `protocol`  | every message, and the version they are checked against |
//! | `net`       | length-prefixed framing over a stream                |
//! | `inventory` | slots, stacks and what fits where                    |
//! | `crafting`  | the recipe table, read by the menu and by the server |
//! | `fluid`     | how deep water is, and where it will flow when it can |
//! | `geometry`  | intersection tests both sides need to agree about    |
//! | `dig`       | a rock taken apart a slice at a time: the bite, and where it is kept |
//! | `load`      | what carrying a heavy pack costs                     |
//! | `combat`    | what a punch reaches, costs and is allowed to do     |
//! | `showcase`  | the test world: a flat field with one of everything on it |
//! | `hearth`    | what is inside a fire: its slots, and what it cooks   |
//! | `pit`       | fires in the ground: pit kiln, charcoal pit, firepit  |
//! | `wildfire`  | fire that spreads, smoke in a room, soot, the standing torch |
//! | `wood`      | which blocks are one tree's timber, and what stands in for what |
//! | `body`      | warmth and water: the two meters the world drives     |
//! | `bees`      | wild hives: the honey in them and what a raid is stung for |
//! | `equipment` | what a person is wearing and what it is worth         |
//! | `minigame`  | the anvil's blows and the wheel's pulls: timing, and what a run is worth |

pub mod animals;
pub mod bees;
pub mod build;
pub mod clay;
pub mod blocks;
pub mod body;
pub mod branch;
pub mod combat;
pub mod comfort;
pub mod crafting;
pub mod dig;
pub mod discovery;
pub mod dripstone;
pub mod spikes;
pub mod equipment;
pub mod fishing;
pub mod fluid;
pub mod food;
pub mod geometry;
pub mod ground;
pub mod haunt;
pub mod husbandry;
pub mod hearth;
pub mod injury;
pub mod inventory;
pub mod lighting;
pub mod load;
pub mod minigame;
pub mod moon;
pub mod net;
pub mod packed;
pub mod palm;
pub mod pit;
/// The ladder from bare hands to steel, walked and totalled. Test-only: it is
/// a measurement of the recipe table, not a part of the game.
#[cfg(test)]
mod progression;
pub mod protocol;
pub mod quality;
pub mod rack;
pub mod raft;
pub mod season;
pub mod shelter;
pub mod stall;
pub mod showcase;
pub mod tools;
pub mod types;
pub mod vermin;
pub mod weather;
pub mod weathering;
pub mod wet;
pub mod wildfire;
pub mod wood;
pub mod worldgen;
pub mod youth;

