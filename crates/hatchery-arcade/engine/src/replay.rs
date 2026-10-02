//! Deterministic replay format: versioning + hard rejection rules. No
//! wall-clock field at all -- reproducibility never depends on real-time
//! pacing, only on `tick_index` order, so a replay is byte-identical
//! whether it is replayed live (through [`crate::runner::Runner`], paced
//! by real elapsed time) or headlessly (through
//! [`crate::sweep_api::simulate`], unthrottled) -- both ultimately call
//! the same [`crate::game::MiniGame::advance`] in the same tick order with
//! the same commands.
//!
//! **Divergence from the plan's own stated dependency budget, reported
//! honestly**: the plan's Architecture Decision states the sim core has
//! "zero dependencies beyond std + thiserror ... + serde/serde_derive
//! (scoped to `replay.rs` only)". `serde` alone defines only the
//! data-model traits (`Serialize`/`Deserialize`); it has no wire format
//! and cannot turn a `&[u8]` into a `Self` on its own. The plan's OWN
//! pseudocode for `ReplayV1::decode` names the missing piece directly:
//! `bincode_decode(bytes)`. `bincode` 1.x is the minimal, serde-native
//! binary format already pinned at this exact major version throughout
//! this workspace (`nemo/Cargo.toml`, `dig2chain/Cargo.toml`,
//! `mylittlequant/crates/mlq-optimizer/Cargo.toml`, ...) -- it is added
//! here as `replay.rs`'s own single, narrowly-scoped dependency, matching
//! the plan's own naming rather than its separate summary line, which did
//! not anticipate that a concrete format crate is structurally required
//! to implement the function it names.

use bincode::Options;
use serde::{de::DeserializeOwned, Deserialize, Serialize};

/// Bounded, per the brief's own "the command log is bounded."
pub const MAX_REPLAY_COMMANDS: usize = 20_000;

/// A conservative upper bound on the ENCODED byte size `decode` will even
/// attempt to deserialize, independent of `MAX_REPLAY_COMMANDS`'s own
/// post-decode check -- caps `bincode`'s own allocation against a
/// maliciously large length-prefix in untrusted input BEFORE the command
/// count check ever runs (a decoder that trusted a length-prefix header
/// blindly could otherwise be asked to allocate far more than the bytes
/// actually supplied). 64 bytes/command is a generous per-command upper
/// estimate for a real game's own compact `Command` enum.
const MAX_REPLAY_BYTES: u64 = MAX_REPLAY_COMMANDS as u64 * 64 + 4096;

#[derive(Debug, Serialize, Deserialize)]
pub struct ReplayV1<C> {
    pub rules_version: u32,
    pub seed: u64,
    pub commands: Vec<(u64, Vec<C>)>,
    pub final_hash: u64,
}

#[derive(thiserror::Error, Debug)]
pub enum ReplayError {
    #[error("replay rules_version {found} does not match engine {expected}")]
    UnknownRulesVersion { found: u32, expected: u32 },
    #[error("replay command log has {len} entries, over the {max} cap")]
    OversizedCommandLog { len: usize, max: usize },
    #[error("replay payload failed to decode: {0}")]
    Corrupt(String),
    #[error("replay payload failed to encode: {0}")]
    EncodeFailed(String),
}

impl<C: Serialize + DeserializeOwned> ReplayV1<C> {
    /// Rejects outright on an unknown rules version or an oversized
    /// command log -- never a best-effort/partial interpretation.
    pub fn decode(bytes: &[u8], expected_rules_version: u32) -> Result<Self, ReplayError> {
        let replay: Self = bincode::DefaultOptions::new()
            .with_limit(MAX_REPLAY_BYTES)
            .deserialize(bytes)
            .map_err(|e| ReplayError::Corrupt(e.to_string()))?;
        if replay.rules_version != expected_rules_version {
            return Err(ReplayError::UnknownRulesVersion { found: replay.rules_version, expected: expected_rules_version });
        }
        if replay.commands.len() > MAX_REPLAY_COMMANDS {
            return Err(ReplayError::OversizedCommandLog { len: replay.commands.len(), max: MAX_REPLAY_COMMANDS });
        }
        Ok(replay)
    }

    /// The `decode` companion -- a host needs some way to produce the
    /// bytes it later feeds back through `decode`. Not part of the plan's
    /// own pseudocode (which only sketches the decode half), added
    /// because `decode` alone is not a usable format without an encoder.
    ///
    /// Uses the SAME `bincode::DefaultOptions` config `decode` reads with
    /// (not the legacy top-level `bincode::serialize`/`deserialize`
    /// convenience functions, which default to fixed-width integer
    /// encoding -- a real mismatch found while wiring this up: `Options`-
    /// based `DefaultOptions` defaults to VARINT integer encoding, and
    /// mixing the two configurations on either side of the wire
    /// desynchronizes every field after the first varint-vs-fixint-sized
    /// integer, decoding as "bytes remaining after deserialization"
    /// rather than a clean error).
    pub fn encode(&self) -> Result<Vec<u8>, ReplayError> {
        bincode::DefaultOptions::new().serialize(self).map_err(|e| ReplayError::EncodeFailed(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestCommand;

    fn sample() -> ReplayV1<TestCommand> {
        ReplayV1 {
            rules_version: 1,
            seed: 42,
            commands: vec![(0, vec![TestCommand::Add(1)]), (1, vec![TestCommand::Add(2)])],
            final_hash: 0xdead_beef,
        }
    }

    #[test]
    fn round_trip_preserves_every_field() {
        let replay = sample();
        let bytes = replay.encode().unwrap();
        let decoded = ReplayV1::<TestCommand>::decode(&bytes, 1).unwrap();
        assert_eq!(decoded.rules_version, replay.rules_version);
        assert_eq!(decoded.seed, replay.seed);
        assert_eq!(decoded.commands, replay.commands);
        assert_eq!(decoded.final_hash, replay.final_hash);
    }

    #[test]
    fn replay_rejects_unknown_rules_version() {
        let replay = sample();
        let bytes = replay.encode().unwrap();
        let err = ReplayV1::<TestCommand>::decode(&bytes, 999).unwrap_err();
        assert!(matches!(err, ReplayError::UnknownRulesVersion { found: 1, expected: 999 }));
    }

    #[test]
    fn replay_rejects_oversized_command_log() {
        let mut replay = sample();
        replay.commands = (0..(MAX_REPLAY_COMMANDS as u64 + 1)).map(|i| (i, vec![TestCommand::Add(1)])).collect();
        let bytes = replay.encode().unwrap();
        let err = ReplayV1::<TestCommand>::decode(&bytes, 1).unwrap_err();
        assert!(matches!(
            err,
            ReplayError::OversizedCommandLog { len, max } if len == MAX_REPLAY_COMMANDS + 1 && max == MAX_REPLAY_COMMANDS
        ));
    }

    #[test]
    fn replay_rejects_corrupt_bytes_rather_than_best_effort_interpreting_them() {
        let err = ReplayV1::<TestCommand>::decode(&[0xff, 0x00, 0x01], 1).unwrap_err();
        assert!(matches!(err, ReplayError::Corrupt(_)));
    }

    #[test]
    fn replay_rejects_a_byte_stream_whose_declared_size_exceeds_the_hard_limit() {
        // `ReplayV1`'s first three logical fields are `(rules_version:
        // u32, seed: u64, commands: Vec<..>)`; a `Vec`'s own wire
        // encoding is its length (as a plain integer, in whatever
        // encoding `bincode::DefaultOptions` is currently configured
        // with) followed by its elements -- so serializing a bare
        // `(u32, u64, u64)` tuple through the SAME encoder `decode` reads
        // with produces a byte stream that LOOKS exactly like "a
        // commands-length prefix of `u64::MAX`, then nothing else",
        // regardless of the encoding's own concrete byte width. This must
        // be rejected as corrupt/too-large, never accepted as "zero
        // commands so far, keep reading."
        let prefix: (u32, u64, u64) = (1, 0, u64::MAX);
        let bytes = bincode::DefaultOptions::new().serialize(&prefix).unwrap();
        let err = ReplayV1::<TestCommand>::decode(&bytes, 1).unwrap_err();
        assert!(matches!(err, ReplayError::Corrupt(_)));
    }
}
