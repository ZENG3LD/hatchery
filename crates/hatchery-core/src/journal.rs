//! Node journal: persistent append-only log + C2 gap recovery.
//!
//! Core never talks to nodes. When RESUME asks for a seq older than the buffer,
//! C2 fetches missing events from the node's journal over the uplink
//! (`JournalReq` / `JournalReply`).

use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use hatchery_c2_protocol::{JournalReplyBody, JournalWireEntry};

use crate::buffer::BufferedEvent;

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
    Io(String),
    Corrupt,
    Other(String),
}

impl std::fmt::Display for JournalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NodeUnknown => write!(f, "node unknown"),
            Self::NodeUnavailable => write!(f, "node unavailable"),
            Self::Io(msg) => write!(f, "journal io: {msg}"),
            Self::Corrupt => write!(f, "journal corrupt"),
            Self::Other(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for JournalError {}

/// In-memory journal for tests.
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

/// On-disk journal format (per node, one file per channel or a single multiplexed file).
///
/// Record: `u32 channel_id ‖ u64 seq ‖ u32 len ‖ [len] payload` (little-endian).
/// Append-only; readers scan from the start (or from a cached offset index).
#[derive(Clone)]
pub struct FileJournal {
    path: PathBuf,
    state: Arc<Mutex<FileJournalState>>,
}

struct FileJournalState {
    /// Next seq to assign per channel.
    next_seq: BTreeMap<u32, u64>,
}

impl FileJournal {
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, JournalError> {
        let path = path.into();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| JournalError::Io(e.to_string()))?;
        }
        let mut next_seq = BTreeMap::new();
        if path.exists() {
            for entry in scan_file(&path)? {
                let next = next_seq.entry(entry.channel_id).or_insert(1);
                *next = (*next).max(entry.seq + 1);
            }
        } else {
            File::create(&path).map_err(|e| JournalError::Io(e.to_string()))?;
        }
        Ok(Self {
            path,
            state: Arc::new(Mutex::new(FileJournalState { next_seq })),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Append a payload; returns the assigned seq.
    pub fn append(&self, channel_id: u32, payload: &[u8]) -> Result<u64, JournalError> {
        if payload.len() > 1024 * 1024 {
            return Err(JournalError::Other("payload too large".into()));
        }
        let mut guard = self.state.lock().expect("journal lock");
        let seq = {
            let entry = guard.next_seq.entry(channel_id).or_insert(1);
            let seq = *entry;
            *entry = seq + 1;
            seq
        };
        let mut file = OpenOptions::new()
            .append(true)
            .open(&self.path)
            .map_err(|e| JournalError::Io(e.to_string()))?;
        write_record(&mut file, channel_id, seq, payload)?;
        file.flush().map_err(|e| JournalError::Io(e.to_string()))?;
        Ok(seq)
    }

    pub fn replay_after(
        &self,
        channel_id: u32,
        after: u64,
        limit: usize,
    ) -> Result<Vec<JournalEntry>, JournalError> {
        let mut out = Vec::new();
        for entry in scan_file(&self.path)? {
            if entry.channel_id == channel_id && entry.seq > after {
                out.push(entry);
                if out.len() >= limit {
                    break;
                }
            }
        }
        Ok(out)
    }

    /// Build a wire reply body for an uplink JournalReq.
    pub fn reply_body(
        &self,
        channel_id: u32,
        after: u64,
        limit: usize,
    ) -> Result<JournalReplyBody, JournalError> {
        let entries = self.replay_after(channel_id, after, limit)?;
        Ok(JournalReplyBody {
            after,
            entries: entries
                .into_iter()
                .map(|e| JournalWireEntry {
                    seq: e.seq,
                    payload: e.payload,
                })
                .collect(),
        })
    }
}

/// Adapter: FileJournal as NodeJournal for a single node id (local / test).
pub struct LocalFileJournal {
    node_id: String,
    journal: FileJournal,
}

impl LocalFileJournal {
    pub fn new(node_id: impl Into<String>, journal: FileJournal) -> Self {
        Self {
            node_id: node_id.into(),
            journal,
        }
    }

    pub fn journal(&self) -> &FileJournal {
        &self.journal
    }
}

impl NodeJournal for LocalFileJournal {
    fn replay_after(
        &self,
        node_id: &str,
        channel_id: u32,
        after: u64,
        limit: usize,
    ) -> Result<Vec<JournalEntry>, JournalError> {
        if node_id != self.node_id {
            return Err(JournalError::NodeUnknown);
        }
        self.journal.replay_after(channel_id, after, limit)
    }
}

fn write_record(file: &mut File, channel_id: u32, seq: u64, payload: &[u8]) -> Result<(), JournalError> {
    file.write_all(&channel_id.to_le_bytes())
        .map_err(|e| JournalError::Io(e.to_string()))?;
    file.write_all(&seq.to_le_bytes())
        .map_err(|e| JournalError::Io(e.to_string()))?;
    file.write_all(&(payload.len() as u32).to_le_bytes())
        .map_err(|e| JournalError::Io(e.to_string()))?;
    file.write_all(payload)
        .map_err(|e| JournalError::Io(e.to_string()))?;
    Ok(())
}

fn scan_file(path: &Path) -> Result<Vec<JournalEntry>, JournalError> {
    let mut file = File::open(path).map_err(|e| JournalError::Io(e.to_string()))?;
    let mut out = Vec::new();
    loop {
        let mut hdr = [0u8; 4 + 8 + 4];
        match file.read_exact(&mut hdr) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(JournalError::Io(e.to_string())),
        }
        let channel_id = u32::from_le_bytes([hdr[0], hdr[1], hdr[2], hdr[3]]);
        let seq = u64::from_le_bytes([
            hdr[4], hdr[5], hdr[6], hdr[7], hdr[8], hdr[9], hdr[10], hdr[11],
        ]);
        let len = u32::from_le_bytes([hdr[12], hdr[13], hdr[14], hdr[15]]) as usize;
        if len > 1024 * 1024 {
            return Err(JournalError::Corrupt);
        }
        let mut payload = vec![0u8; len];
        file.read_exact(&mut payload)
            .map_err(|_| JournalError::Corrupt)?;
        out.push(JournalEntry {
            seq,
            channel_id,
            payload,
        });
    }
    // Ensure we did not stop mid-record with leftover bytes that are not EOF-aligned.
    let pos = file.stream_position().map_err(|e| JournalError::Io(e.to_string()))?;
    let len = file.seek(SeekFrom::End(0)).map_err(|e| JournalError::Io(e.to_string()))?;
    if pos != len && pos + (4 + 8 + 4) as u64 <= len {
        // Trailing incomplete record — treat as corrupt rather than silent truncate.
        return Err(JournalError::Corrupt);
    }
    Ok(out)
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

    #[test]
    fn file_journal_persists_across_reopen() {
        let dir = std::env::temp_dir().join(format!("fj-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("node.journal");
        {
            let j = FileJournal::open(&path).unwrap();
            assert_eq!(j.append(1, b"a").unwrap(), 1);
            assert_eq!(j.append(1, b"b").unwrap(), 2);
            assert_eq!(j.append(2, b"x").unwrap(), 1);
        }
        let j = FileJournal::open(&path).unwrap();
        let got = j.replay_after(1, 0, 10).unwrap();
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].payload, b"a");
        assert_eq!(got[1].payload, b"b");
        assert_eq!(j.append(1, b"c").unwrap(), 3);
        let body = j.reply_body(1, 1, 10).unwrap();
        assert_eq!(body.entries.len(), 2);
        assert_eq!(body.entries[0].seq, 2);
        assert_eq!(body.entries[1].seq, 3);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn local_file_journal_trait() {
        let dir = std::env::temp_dir().join(format!("lfj-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("j");
        let fj = FileJournal::open(&path).unwrap();
        fj.append(1, b"p").unwrap();
        let local = LocalFileJournal::new("node-a", fj);
        let got = NodeJournal::replay_after(&local, "node-a", 1, 0, 10).unwrap();
        assert_eq!(got[0].payload, b"p");
        assert!(NodeJournal::replay_after(&local, "other", 1, 0, 10).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
