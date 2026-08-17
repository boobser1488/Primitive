//! The rules: everything the server is authoritative about.
//!
//! | module      | what it decides                                      |
//! |-------------|------------------------------------------------------|
//! | `world`     | what block is where, generation, saving and loading   |
//! | `survival`  | health, falls, drowning, suffocation, respawn         |
//! | `items`     | dropped stacks, their motion, and who picks them up   |
//! | `containers`| what is inside the chests, and how it is saved        |
//! | `falling`   | sand that has lost its support                        |
//! | `water`     | where the water goes when it has somewhere to go      |
//! | `simulation`| the shape every cell-watching mechanic is written to  |
//! | `anticheat` | whether a client's claimed movement is possible       |
//! | `commands`  | the console and chat commands                         |
//! | `profiles`  | what a player had when they last left                 |
//! | `plugins`   | scripted hooks into all of the above                  |
//!
//! This layer is the reason the client can be wrong without it
//! mattering. A client says where it *thinks* it is and what it *wants*
//! to do; what actually happens is decided here, and the answer is sent
//! back. The rules that both sides need to agree on -- hardness, drops,
//! weight, what may grow on what -- live in `primitive_shared` and are
//! read from there rather than being written twice.

pub mod anticheat;
pub mod commands;
pub mod containers;
pub mod falling;
pub mod items;
pub mod plugins;
pub mod profiles;
pub mod simulation;
pub mod survival;
pub mod water;
pub mod world;
