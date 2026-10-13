//! Node journal view used by C2 to recover gaps outside the replay buffer.
//!
//! Core never talks to nodes. When RESUME asks for a seq older than the buffer,
//! C2 fetches the missing events from the node's journal itself.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use super::buffer::BufferedEvent;

/// Opaque journal entry as the node stores it for retransmission.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JournalEntry {
    pub seq: u64,
    pub channel_id: u32,
    pub payload: Vec<u8>,
}

/// C2-side handle to ask a connected node for a journal slice.
pub trait NodeJournal: Send + Sync {
    /// Return entries with `seq > after` up to `limit`, oldest first.
    fn replay_after(
        &self,
        node_id: &str,
        channel_id: u32,
        after: u64,
        limit: usize,
    ) -> Result<Vec<JournalEntry>, JournalError>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JournalError {
    NodeUnknown,
    NodeUnavailable,
    Other(String),
}

impl std::fmt::Display for JournalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NodeUnknown => write!(f, "node unknown"),
            Self::NodeUnavailable => write!(f, "node unavailable"),
            Self::Other(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for JournalError {}

/// In-memory journal for tests and local harnesses. Production nodes expose theirs over uplink.
#[derive(Clone, Default)]
pub struct MemoryJournal {
    inner: Arc<Mutex<BTreeMap<String, Vec<JournalEntry>>>>,
}

impl MemoryJournal {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn append(&self, node_id: &str, channel_id: u32, seq: u64, payload: Vec<u8>) {
        let mut guard = self.inner.lock().expect("journal lock");
        guard
            .entry(node_id.to_owned())
            .or_default()
            .push(JournalEntry {
                seq,
                channel_id,
                payload,
            });
    }
}

impl NodeJournal for MemoryJournal {
    fn replay_after(
        &self,
        node_id: &str,
        channel_id: u32,
        after: u64,
        limit: usize,
    ) -> Result<Vec<JournalEntry>, JournalError> {
        let guard = self.inner.lock().expect("journal lock");
        let Some(entries) = guard.get(node_id) else {
            return Err(JournalError::NodeUnknown);
        };
        Ok(entries
            .iter()
            .filter(|e| e.channel_id == channel_id && e.seq > after)
            .take(limit)
            .cloned()
            .collect())
    }
}

/// Recover events for a RESUME that falls before the in-memory buffer.
pub fn recover_gap(
    journal: &dyn NodeJournal,
    node_id: &str,
    channel_id: u32,
    after: u64,
    until_exclusive: Option<u64>,
    limit: usize,
) -> Result<Vec<BufferedEvent>, JournalError> {
    let entries = journal.replay_after(node_id, channel_id, after, limit)?;
    Ok(entries
        .into_iter()
        .filter(|e| until_exclusive.map(|u| e.seq < u).unwrap_or(true))
        .map(|e| BufferedEvent {
            seq: e.seq,
            node_id: node_id.to_owned(),
            channel_id: e.channel_id,
            payload: e.payload,
            enqueued_at: std::time::Instant::now(),
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_journal_replays_slice() {
        let j = MemoryJournal::new();
        for seq in 1..=5 {
            j.append("n1", 1, seq, vec![seq as u8]);
        }
        let got = j.replay_after("n1", 1, 2, 10).unwrap();
        assert_eq!(got.iter().map(|e| e.seq).collect::<Vec<_>>(), vec![3, 4, 5]);
    }

    #[test]
    fn recover_gap_stops_before_buffer_oldest() {
        let j = MemoryJournal::new();
        for seq in 1..=10 {
            j.append("n1", 1, seq, vec![seq as u8]);
        }
        let recovered = recover_gap(&j, "n1", 1, 1, Some(5), 100).unwrap();
        assert_eq!(
            recovered.iter().map(|e| e.seq).collect::<Vec<_>>(),
            vec![2, 3, 4]
        );
    }
}
