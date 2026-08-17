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
//! | `protocol`  | every message, and the version they are checked against |
//! | `net`       | length-prefixed framing over a stream                |
//! | `inventory` | slots, stacks and what fits where                    |
//! | `crafting`  | the recipe table, read by the menu and by the server |
//! | `fluid`     | how deep water is, and where it will flow when it can |
//! | `geometry`  | intersection tests both sides need to agree about    |
//! | `load`      | what carrying a heavy pack costs                     |
//! | `combat`    | what a punch reaches, costs and is allowed to do     |

pub mod blocks;
pub mod combat;
pub mod crafting;
pub mod fluid;
pub mod geometry;
pub mod inventory;
pub mod lighting;
pub mod load;
pub mod net;
pub mod protocol;
pub mod types;
pub mod worldgen;
