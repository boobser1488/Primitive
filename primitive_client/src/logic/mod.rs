//! Game logic: the world as the client understands it, and what the
//! player does to it.
//!
//! | module          | what it decides                                  |
//! |-----------------|--------------------------------------------------|
//! | `chunk_manager` | which chunks are loaded, wanted, or gone          |
//! | `physics`       | where the player's body ends up                   |
//! | `mining`        | how long a block takes to break, and what is aimed at |
//! | `inventory`     | what the hotbar has in it (the server owns the truth) |
//! | `entities`      | dropped items and falling blocks, between snapshots |
//! | `hand`          | the player's own arm, and what it is holding      |
//! | `stamina`       | whether a sprint is available                     |
//! | `shake`         | the camera's own motion -- bob, sway, recoil      |
//! | `worlds`        | the singleplayer saves on disk                    |
//!
//! ## None of this is authoritative
//!
//! The server decides what is true. What lives here is the client's
//! working copy: enough to move smoothly between snapshots, to show the
//! result of an action before it is confirmed, and to answer questions
//! (is this block solid? how far can I reach?) without a round trip.
//! Anything the server contradicts is overwritten, which is why the
//! rules themselves -- hardness, drops, weight, what may grow where --
//! live in `primitive_shared` and are read by both sides rather than
//! being implemented twice.

pub mod chunk_manager;
pub mod entities;
pub mod hand;
pub mod inventory;
pub mod mining;
pub mod physics;
pub mod shake;
pub mod stamina;
pub mod worlds;
