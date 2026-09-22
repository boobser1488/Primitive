//! Game logic: the world as the client understands it, and what the
//! player does to it.
//!
//! | module          | what it decides                                  |
//! |-----------------|--------------------------------------------------|
//! | `chunk_manager` | which chunks are loaded, wanted, or gone          |
//! | `physics`       | where the player's body ends up                   |
//! | `menu_scene`    | the patch of world the main menu is drawn over    |
//! | `mining`        | how long a block takes to break, and what is aimed at |
//! | `inventory`     | what the hotbar has in it (the server owns the truth) |
//! | `entities`      | dropped items and falling blocks, between snapshots |
//! | `fishing`       | the float on the water, and what to say instead of a cast |
//! | `hand`          | the player's own arm, and what it is holding      |
//! | `player_model`  | what another player looks like, and how they move |
//! | `stamina`       | whether a sprint is available                     |
//! | `bearing`       | where north is, off the sky or a compass needle   |
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

pub mod animal_model;
pub mod bearing;
pub mod bbmodel;
pub mod models;
mod model_notes;
pub mod obj_export;
pub mod chunk_manager;
pub mod entities;
pub mod fishing;
pub mod hand;
pub mod inventory;
pub mod map;
pub mod menu_scene;
pub mod mining;
#[cfg(test)]
mod model_overlap;
pub mod physics;
pub mod player_model;
pub mod posture;
pub mod raft_model;
pub mod riding;
pub mod horseback;
pub mod shake;
pub mod stamina;
pub mod worlds;
