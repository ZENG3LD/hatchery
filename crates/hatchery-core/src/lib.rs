//! Shared hatchery types and utilities.
//!
//! Node, edge (C2) and core (server) all depend on this crate; it depends on
//! none of them. Anything two of those components need lives here, so the node
//! never has to depend on the edge or the server.

pub mod buffer;
pub mod journal;

/// Restream wire: frames, channels, journal request/reply bodies.
pub mod wire {
    pub use hatchery_c2_protocol::*;
}

/// Nested restream crypto: outer edge layer, inner Noise layer, key types.
pub mod crypto {
    pub use hatchery_c2_protocol::nested::*;
}

pub use buffer::{BufferLimits, BufferedEvent, EnqueueOutcome, ReplayBuffer, StreamKey};
pub use crypto::{PublicKeyBytes, RestreamerKeys};
pub use journal::{
    recover_gap, FileJournal, JournalEntry, JournalError, LocalFileJournal, MemoryJournal,
    NodeJournal,
};
