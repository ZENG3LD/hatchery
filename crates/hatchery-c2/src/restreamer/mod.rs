//! C2 byte restreamer (A2).
//!
//! Edge is not part of the server: it accepts uplink from nodes, holds a bounded
//! replay buffer, and lets core drain a long-lived stream. C2 never pushes to
//! core and never parses g4a / m4a payloads.

mod buffer;
mod crypto;
mod journal;
mod runtime;

pub use buffer::{BufferLimits, BufferedEvent, EnqueueOutcome, ReplayBuffer, StreamKey};
pub use crypto::{
    c2_cannot_complete_as_hq, c2_cannot_open_inner, c2_strip_outer, core_open_inner,
    noise_session_seal, CryptoError, HqRecipientSecret, NestedSeal, NoiseInitiator, NoiseResponder,
    NoiseTransport, PublicKeyBytes, RestreamerKeys, NOISE_PATTERN,
};
pub use journal::{recover_gap, JournalEntry, JournalError, MemoryJournal, NodeJournal};
pub use runtime::{
    serve, DrainClient, DrainSession, Restreamer, RestreamerConfig, RestreamerError,
};

#[cfg(test)]
mod isolation_tests;
