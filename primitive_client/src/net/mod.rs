//! The network layer: the connection to the server, and the state that
//! arrives over it.
//!
//! Everything here is about the wire. `network` owns the socket, the
//! background reader and writer tasks, and the queues between them and
//! the frame loop; `remote_players` holds the other players the server
//! tells us about and interpolates them between snapshots, because
//! snapshots arrive at the tick rate and frames do not.
//!
//! The rest of the client never touches a socket. It reads messages out
//! of `network`'s queue and pushes messages into it, which is what makes
//! singleplayer -- the same server in the same process, on the loopback
//! interface -- indistinguishable from a remote one everywhere above
//! this line.

pub mod network;
pub mod remote_players;
