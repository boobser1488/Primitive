//! The network layer: sockets, framing, and who is on the other end.
//!
//! `connection` owns a client's three tasks -- a reader, a writer and a
//! chunk pump -- and translates between the wire protocol and the rest
//! of the server. `players` is the registry: every connected player, the
//! chunks each of them is subscribed to, and the reverse index that lets
//! a block edit find exactly the players who can see it without walking
//! the whole list.
//!
//! Nothing here decides anything about the world. A message arrives,
//! it is validated for shape and rate, and then it is handed to
//! [`crate::logic`] -- which is where it is decided whether the thing
//! being asked for is allowed to happen.

pub mod connection;
pub mod players;
