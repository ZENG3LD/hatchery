//! File-based knowledge store for inter-Queen communication.
//! Uses JSONL (append-only) with file locking for concurrent access.

use std::path::{Path, PathBuf};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Serialize, Deserialize};

/// A single knowledge entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeEntry {
    pub queen_id: String,
    pub task_id: String,
    pub key: String,
    pub value: serde_json::Value,
    pub timestamp: DateTime<Utc>,
}

/// File-based knowledge store using JSONL format.
pub struct KnowledgeFile {
    path: PathBuf,
}

impl KnowledgeFile {
    /// Create or open a knowledge file at the given path.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        Self { path }
    }

    /// Create a knowledge file in the standard location.
    pub fn in_working_dir(working_dir: &Path) -> Self {
        Self::new(working_dir.join(".hatchery").join("knowledge.jsonl"))
    }

    /// Read all entries from the file.
    pub fn read(&self) -> Result<Vec<KnowledgeEntry>> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }
        let file = File::open(&self.path)?;
        let reader = BufReader::new(file);
        let mut entries = Vec::new();
        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() { continue; }
            match serde_json::from_str::<KnowledgeEntry>(&line) {
                Ok(entry) => entries.push(entry),
                Err(e) => eprintln!("[KnowledgeFile] Skipping malformed entry: {}", e),
            }
        }
        Ok(entries)
    }

    /// Append an entry to the file (with exclusive file lock on Windows via OpenOptions).
    pub fn write_entry(&self, entry: &KnowledgeEntry) -> Result<()> {
        // Ensure parent dir exists
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        let line = serde_json::to_string(entry)?;
        writeln!(file, "{}", line)?;
        Ok(())
    }

    /// Get entries for a specific queen.
    pub fn entries_for(&self, queen_id: &str) -> Result<Vec<KnowledgeEntry>> {
        Ok(self.read()?.into_iter().filter(|e| e.queen_id == queen_id).collect())
    }

    /// Get entries since a given timestamp.
    pub fn entries_since(&self, since: DateTime<Utc>) -> Result<Vec<KnowledgeEntry>> {
        Ok(self.read()?.into_iter().filter(|e| e.timestamp >= since).collect())
    }

    /// Get the last N entries (most recent).
    pub fn recent(&self, n: usize) -> Result<Vec<KnowledgeEntry>> {
        let entries = self.read()?;
        let start = entries.len().saturating_sub(n);
        Ok(entries[start..].to_vec())
    }

    /// Path to the knowledge file.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_knowledge_file_read_write() {
        let dir = tempdir().unwrap();
        let kf = KnowledgeFile::new(dir.path().join("test.jsonl"));

        let entry = KnowledgeEntry {
            queen_id: "Q0".to_string(),
            task_id: "T1".to_string(),
            key: "result".to_string(),
            value: serde_json::json!({"status": "completed"}),
            timestamp: Utc::now(),
        };

        kf.write_entry(&entry).unwrap();
        let entries = kf.read().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].queen_id, "Q0");
        assert_eq!(entries[0].task_id, "T1");
    }

    #[test]
    fn test_knowledge_file_entries_for() {
        let dir = tempdir().unwrap();
        let kf = KnowledgeFile::new(dir.path().join("test.jsonl"));

        let entry1 = KnowledgeEntry {
            queen_id: "Q0".to_string(),
            task_id: "T1".to_string(),
            key: "k1".to_string(),
            value: serde_json::json!("v1"),
            timestamp: Utc::now(),
        };

        let entry2 = KnowledgeEntry {
            queen_id: "Q1".to_string(),
            task_id: "T2".to_string(),
            key: "k2".to_string(),
            value: serde_json::json!("v2"),
            timestamp: Utc::now(),
        };

        kf.write_entry(&entry1).unwrap();
        kf.write_entry(&entry2).unwrap();

        let q0_entries = kf.entries_for("Q0").unwrap();
        assert_eq!(q0_entries.len(), 1);
        assert_eq!(q0_entries[0].queen_id, "Q0");
    }

    #[test]
    fn test_knowledge_file_entries_since() {
        let dir = tempdir().unwrap();
        let kf = KnowledgeFile::new(dir.path().join("test.jsonl"));

        let now = Utc::now();
        let past = now - chrono::Duration::hours(1);

        let entry1 = KnowledgeEntry {
            queen_id: "Q0".to_string(),
            task_id: "T1".to_string(),
            key: "k1".to_string(),
            value: serde_json::json!("v1"),
            timestamp: past,
        };

        let entry2 = KnowledgeEntry {
            queen_id: "Q1".to_string(),
            task_id: "T2".to_string(),
            key: "k2".to_string(),
            value: serde_json::json!("v2"),
            timestamp: now,
        };

        kf.write_entry(&entry1).unwrap();
        kf.write_entry(&entry2).unwrap();

        let recent_entries = kf.entries_since(past + chrono::Duration::minutes(30)).unwrap();
        assert_eq!(recent_entries.len(), 1);
        assert_eq!(recent_entries[0].task_id, "T2");
    }

    #[test]
    fn test_knowledge_file_recent() {
        let dir = tempdir().unwrap();
        let kf = KnowledgeFile::new(dir.path().join("test.jsonl"));

        for i in 0..5 {
            let entry = KnowledgeEntry {
                queen_id: format!("Q{}", i),
                task_id: format!("T{}", i),
                key: "key".to_string(),
                value: serde_json::json!(i),
                timestamp: Utc::now(),
            };
            kf.write_entry(&entry).unwrap();
        }

        let recent = kf.recent(3).unwrap();
        assert_eq!(recent.len(), 3);
        assert_eq!(recent[0].queen_id, "Q2");
        assert_eq!(recent[1].queen_id, "Q3");
        assert_eq!(recent[2].queen_id, "Q4");
    }

    #[test]
    fn test_knowledge_file_empty() {
        let dir = tempdir().unwrap();
        let kf = KnowledgeFile::new(dir.path().join("nonexistent.jsonl"));

        let entries = kf.read().unwrap();
        assert_eq!(entries.len(), 0);
    }
}
