//! Shared state memory implementation.
//!
//! Simple key-value store with thread-safe access, versioning, and timestamps.

use super::Memory;
use crate::core::types::AgentId;
use anyhow::{Context, Result};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Configuration for SharedStateMemory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedStateConfig {
    /// Interval for automatic state snapshots (if enabled).
    pub autosave_interval: Duration,
}

impl Default for SharedStateConfig {
    fn default() -> Self {
        SharedStateConfig {
            autosave_interval: Duration::from_secs(300), // 5 minutes
        }
    }
}

/// A single entry in the shared state memory.
#[derive(Debug, Clone)]
struct MemoryEntry {
    /// The actual value stored.
    value: Value,
    /// Which agent last updated this entry.
    source: String,
    /// When the entry was first created.
    created_at: Instant,
    /// When the entry was last updated.
    updated_at: Instant,
    /// Version counter (incremented on each update).
    version: u64,
}

/// Serializable snapshot entry (converts Instant to seconds since start).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SnapshotEntry {
    value: Value,
    source: String,
    created_secs: u64,
    updated_secs: u64,
    version: u64,
}

/// Shared state memory implementation with versioning and metadata.
pub struct SharedStateMemory {
    config: SharedStateConfig,
    state: Arc<RwLock<HashMap<String, MemoryEntry>>>,
    start_time: Instant,
}

impl SharedStateMemory {
    /// Create a new SharedStateMemory with default configuration.
    pub fn new() -> Self {
        Self::with_config(SharedStateConfig::default())
    }

    /// Create a new SharedStateMemory with custom configuration.
    pub fn with_config(config: SharedStateConfig) -> Self {
        SharedStateMemory {
            config,
            state: Arc::new(RwLock::new(HashMap::new())),
            start_time: Instant::now(),
        }
    }

    /// Get the number of entries in memory.
    pub fn len(&self) -> usize {
        self.state.read().len()
    }

    /// Check if memory is empty.
    pub fn is_empty(&self) -> bool {
        self.state.read().is_empty()
    }

    /// Get all keys currently stored.
    pub fn keys(&self) -> Vec<String> {
        self.state.read().keys().cloned().collect()
    }

    /// Get version of a specific key.
    pub fn get_version(&self, key: &str) -> Option<u64> {
        self.state.read().get(key).map(|entry| entry.version)
    }

    /// Get metadata for a key (source, timestamps, version).
    pub fn get_metadata(&self, key: &str) -> Option<(String, Instant, Instant, u64)> {
        self.state.read().get(key).map(|entry| {
            (
                entry.source.clone(),
                entry.created_at,
                entry.updated_at,
                entry.version,
            )
        })
    }

    /// Convert AgentId to string representation for storage.
    fn agent_id_to_string(agent: &AgentId) -> String {
        match agent {
            AgentId::Nydus(id) => format!("Nydus({})", id.0),
            AgentId::Queen(id) => format!("Queen({})", id.0),
            AgentId::Overlord(id) => format!("Overlord({})", id.0),
            AgentId::Overmind(id) => format!("Overmind({})", id.0),
            AgentId::Validator => "Validator".to_string(),
            AgentId::Operator => "Operator".to_string(),
        }
    }
}

impl Default for SharedStateMemory {
    fn default() -> Self {
        Self::new()
    }
}

impl Memory for SharedStateMemory {
    fn insert(&mut self, key: String, value: Value, source: AgentId) -> Result<()> {
        let source_str = Self::agent_id_to_string(&source);
        let mut state = self.state.write();
        let now = Instant::now();

        if let Some(entry) = state.get_mut(&key) {
            // Update existing entry
            entry.value = value;
            entry.source = source_str;
            entry.updated_at = now;
            entry.version += 1;
        } else {
            // Create new entry
            state.insert(
                key.clone(),
                MemoryEntry {
                    value,
                    source: source_str,
                    created_at: now,
                    updated_at: now,
                    version: 1,
                },
            );
        }

        Ok(())
    }

    fn get(&self, key: &str) -> Option<Value> {
        self.state.read().get(key).map(|entry| entry.value.clone())
    }

    fn query(&self, pattern: &str) -> Result<Vec<(String, Value)>> {
        let state = self.state.read();
        let mut results = Vec::new();

        // Simple glob-like pattern matching (supports * wildcard)
        if pattern.contains('*') {
            let parts: Vec<&str> = pattern.split('*').collect();
            let prefix = parts.first().unwrap_or(&"");
            let suffix = parts.last().unwrap_or(&"");

            for (key, entry) in state.iter() {
                let matches = if parts.len() == 1 {
                    // No wildcard
                    key == pattern
                } else if parts.len() == 2 {
                    // Single wildcard
                    if prefix.is_empty() {
                        key.ends_with(suffix)
                    } else if suffix.is_empty() {
                        key.starts_with(prefix)
                    } else {
                        key.starts_with(prefix) && key.ends_with(suffix)
                    }
                } else {
                    // Multiple wildcards - check all parts in order
                    let mut pos = 0;
                    let mut all_match = true;
                    for (i, part) in parts.iter().enumerate() {
                        if part.is_empty() {
                            continue;
                        }
                        if i == 0 {
                            if !key.starts_with(part) {
                                all_match = false;
                                break;
                            }
                            pos = part.len();
                        } else if let Some(idx) = key[pos..].find(part) {
                            pos += idx + part.len();
                        } else {
                            all_match = false;
                            break;
                        }
                    }
                    all_match
                };

                if matches {
                    results.push((key.clone(), entry.value.clone()));
                }
            }
        } else {
            // Exact match
            if let Some(entry) = state.get(pattern) {
                results.push((pattern.to_string(), entry.value.clone()));
            }
        }

        Ok(results)
    }

    fn evict(&mut self) -> Result<usize> {
        // Shared state has infinite TTL - no eviction needed
        Ok(0)
    }

    fn snapshot(&self) -> Result<Value> {
        let state = self.state.read();
        let mut snapshot_map = HashMap::new();

        for (key, entry) in state.iter() {
            let snapshot_entry = SnapshotEntry {
                value: entry.value.clone(),
                source: entry.source.clone(),
                created_secs: entry.created_at.duration_since(self.start_time).as_secs(),
                updated_secs: entry.updated_at.duration_since(self.start_time).as_secs(),
                version: entry.version,
            };
            snapshot_map.insert(key.clone(), snapshot_entry);
        }

        serde_json::to_value(&snapshot_map).context("Failed to serialize snapshot")
    }

    fn restore(&mut self, snapshot: Value) -> Result<()> {
        let snapshot_map: HashMap<String, SnapshotEntry> =
            serde_json::from_value(snapshot).context("Failed to deserialize snapshot")?;

        let mut state = self.state.write();
        state.clear();

        for (key, snapshot_entry) in snapshot_map {
            let created_at = self.start_time + Duration::from_secs(snapshot_entry.created_secs);
            let updated_at = self.start_time + Duration::from_secs(snapshot_entry.updated_secs);

            state.insert(
                key,
                MemoryEntry {
                    value: snapshot_entry.value,
                    source: snapshot_entry.source,
                    created_at,
                    updated_at,
                    version: snapshot_entry.version,
                },
            );
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::types::{NydusId, QueenId};

    #[test]
    fn test_insert_and_get() {
        let mut mem = SharedStateMemory::new();
        let agent = AgentId::Queen(QueenId("Q0".to_string()));

        mem.insert("key1".to_string(), Value::String("value1".to_string()), agent)
            .unwrap();

        assert_eq!(mem.get("key1"), Some(Value::String("value1".to_string())));
        assert_eq!(mem.len(), 1);
    }

    #[test]
    fn test_versioning() {
        let mut mem = SharedStateMemory::new();
        let agent = AgentId::Queen(QueenId("Q0".to_string()));

        mem.insert("key1".to_string(), Value::String("v1".to_string()), agent.clone())
            .unwrap();
        assert_eq!(mem.get_version("key1"), Some(1));

        mem.insert("key1".to_string(), Value::String("v2".to_string()), agent)
            .unwrap();
        assert_eq!(mem.get_version("key1"), Some(2));
    }

    #[test]
    fn test_query_wildcard() {
        let mut mem = SharedStateMemory::new();
        let agent = AgentId::Nydus(NydusId("N0".to_string()));

        mem.insert("task:1".to_string(), Value::String("t1".to_string()), agent.clone())
            .unwrap();
        mem.insert("task:2".to_string(), Value::String("t2".to_string()), agent.clone())
            .unwrap();
        mem.insert("status:1".to_string(), Value::String("s1".to_string()), agent)
            .unwrap();

        let results = mem.query("task:*").unwrap();
        assert_eq!(results.len(), 2);

        let results = mem.query("*:1").unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_snapshot_restore() {
        let mut mem = SharedStateMemory::new();
        let agent = AgentId::Validator;

        mem.insert("k1".to_string(), Value::String("v1".to_string()), agent.clone())
            .unwrap();
        mem.insert("k2".to_string(), Value::Number(42.into()), agent)
            .unwrap();

        let snapshot = mem.snapshot().unwrap();
        assert!(snapshot.is_object());

        let mut mem2 = SharedStateMemory::new();
        mem2.restore(snapshot).unwrap();

        assert_eq!(mem2.get("k1"), Some(Value::String("v1".to_string())));
        assert_eq!(mem2.get("k2"), Some(Value::Number(42.into())));
    }
}
