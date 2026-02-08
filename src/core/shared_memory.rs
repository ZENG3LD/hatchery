//! SharedMemory knowledge store for Hatchery V2.
//!
//! Provides a concurrent, persistent key-value store for agent coordination:
//! - Arc-based shared state with parking_lot::RwLock for efficient concurrent access
//! - Knowledge entries with TTL, visibility control, and metadata
//! - Task result storage for tracking completed work
//! - JSON file persistence for crash recovery
//! - Automatic expiration with evict_expired()

use crate::core::types::{AgentId, SwarmHostId, Visibility};
use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

/// A knowledge entry stored in shared memory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeEntry {
    pub key: String,
    pub value: serde_json::Value,
    pub author: AgentId,
    pub timestamp: DateTime<Utc>,
    pub visibility: Visibility,
    pub ttl: Option<Duration>,
}

impl KnowledgeEntry {
    /// Check if this entry has expired based on its TTL.
    pub fn is_expired(&self) -> bool {
        if let Some(ttl) = self.ttl {
            let now = Utc::now();
            let expiry = self.timestamp + chrono::Duration::from_std(ttl).unwrap_or_default();
            now > expiry
        } else {
            false
        }
    }
}

/// Metadata about the memory store.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryMetadata {
    pub created_at: DateTime<Utc>,
    pub last_updated: DateTime<Utc>,
    pub swarm_id: SwarmHostId,
}

/// Internal state of shared memory, protected by RwLock.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryState {
    pub version: u64,
    pub knowledge: HashMap<String, KnowledgeEntry>,
    pub task_results: HashMap<String, serde_json::Value>,
    pub metadata: MemoryMetadata,
}

/// Thread-safe shared memory for swarm coordination.
///
/// Uses parking_lot::RwLock for concurrent access with reader preference.
/// Supports JSON persistence for crash recovery.
pub struct SharedMemory {
    state: Arc<RwLock<MemoryState>>,
    persist_path: Option<PathBuf>,
}

impl SharedMemory {
    /// Create a new in-memory SharedMemory (no persistence).
    pub fn new(swarm_id: SwarmHostId) -> Self {
        let now = Utc::now();
        let state = MemoryState {
            version: 0,
            knowledge: HashMap::new(),
            task_results: HashMap::new(),
            metadata: MemoryMetadata {
                created_at: now,
                last_updated: now,
                swarm_id,
            },
        };

        SharedMemory {
            state: Arc::new(RwLock::new(state)),
            persist_path: None,
        }
    }

    /// Create a new SharedMemory with JSON file persistence.
    pub fn with_persistence(swarm_id: SwarmHostId, persist_path: PathBuf) -> Self {
        let mut memory = Self::new(swarm_id);
        memory.persist_path = Some(persist_path);
        memory
    }

    /// Insert a knowledge entry with default visibility and no TTL.
    pub fn insert(&self, key: String, value: serde_json::Value, author: AgentId) {
        self.insert_with_options(key, value, author, Visibility::default_internal(), None);
    }

    /// Insert a knowledge entry with custom visibility and TTL.
    pub fn insert_with_options(
        &self,
        key: String,
        value: serde_json::Value,
        author: AgentId,
        visibility: Visibility,
        ttl: Option<Duration>,
    ) {
        let entry = KnowledgeEntry {
            key: key.clone(),
            value,
            author,
            timestamp: Utc::now(),
            visibility,
            ttl,
        };

        let mut state = self.state.write();
        state.knowledge.insert(key, entry);
        state.version += 1;
        state.metadata.last_updated = Utc::now();
    }

    /// Get a knowledge entry's value by key.
    pub fn get(&self, key: &str) -> Option<serde_json::Value> {
        let state = self.state.read();
        state.knowledge.get(key).map(|entry| entry.value.clone())
    }

    /// Get the full KnowledgeEntry (including metadata).
    pub fn get_entry(&self, key: &str) -> Option<KnowledgeEntry> {
        let state = self.state.read();
        state.knowledge.get(key).cloned()
    }

    /// Query entries matching a key pattern (substring match).
    pub fn query(&self, pattern: &str) -> Vec<KnowledgeEntry> {
        let state = self.state.read();
        state
            .knowledge
            .iter()
            .filter(|(key, _)| key.contains(pattern))
            .map(|(_, entry)| entry.clone())
            .collect()
    }

    /// Remove expired entries (those past their TTL).
    /// Returns the count of removed entries.
    pub fn evict_expired(&self) -> usize {
        let mut state = self.state.write();
        let initial_count = state.knowledge.len();

        state.knowledge.retain(|_, entry| !entry.is_expired());

        let removed = initial_count - state.knowledge.len();
        if removed > 0 {
            state.version += 1;
            state.metadata.last_updated = Utc::now();
        }

        removed
    }

    /// Store a task result.
    pub fn store_task_result(&self, task_id: &str, result: serde_json::Value) {
        let mut state = self.state.write();
        state.task_results.insert(task_id.to_string(), result);
        state.version += 1;
        state.metadata.last_updated = Utc::now();
    }

    /// Get a task result.
    pub fn get_task_result(&self, task_id: &str) -> Option<serde_json::Value> {
        let state = self.state.read();
        state.task_results.get(task_id).cloned()
    }

    /// Get all task results.
    pub fn all_task_results(&self) -> HashMap<String, serde_json::Value> {
        let state = self.state.read();
        state.task_results.clone()
    }

    /// Current version (incremented on every write).
    pub fn version(&self) -> u64 {
        let state = self.state.read();
        state.version
    }

    /// Get a snapshot of the full state (cloned).
    pub fn snapshot(&self) -> MemoryState {
        let state = self.state.read();
        state.clone()
    }

    /// Get an Arc clone of the internal state (for sharing across threads).
    pub fn shared_state(&self) -> Arc<RwLock<MemoryState>> {
        Arc::clone(&self.state)
    }

    /// Number of knowledge entries.
    pub fn len(&self) -> usize {
        let state = self.state.read();
        state.knowledge.len()
    }

    /// Whether the store is empty.
    pub fn is_empty(&self) -> bool {
        let state = self.state.read();
        state.knowledge.is_empty()
    }

    /// Save state to the persistence path (if configured).
    pub fn save(&self) -> Result<()> {
        let path = self
            .persist_path
            .as_ref()
            .ok_or_else(|| anyhow!("No persistence path configured"))?;

        let state = self.state.read();
        let json = serde_json::to_string_pretty(&*state)
            .context("Failed to serialize SharedMemory state")?;

        // Create parent directory if it doesn't exist
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .context("Failed to create parent directory for persistence file")?;
        }

        std::fs::write(path, json).context("Failed to write SharedMemory state to file")?;

        Ok(())
    }

    /// Load state from a JSON file.
    pub fn load(persist_path: PathBuf) -> Result<Self> {
        let json = std::fs::read_to_string(&persist_path)
            .context("Failed to read SharedMemory state file")?;

        let state: MemoryState =
            serde_json::from_str(&json).context("Failed to deserialize SharedMemory state")?;

        Ok(SharedMemory {
            state: Arc::new(RwLock::new(state)),
            persist_path: Some(persist_path),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::types::QueenId;
    use std::thread;
    use std::time::Duration as StdDuration;

    fn test_agent() -> AgentId {
        AgentId::Queen(QueenId("Q0".to_string()))
    }

    fn test_swarm_id() -> SwarmHostId {
        SwarmHostId("test-swarm".to_string())
    }

    #[test]
    fn test_insert_and_get() {
        let mem = SharedMemory::new(test_swarm_id());
        let value = serde_json::json!({"data": "test"});

        mem.insert("key1".to_string(), value.clone(), test_agent());

        let retrieved = mem.get("key1").expect("Key should exist");
        assert_eq!(retrieved, value);

        assert_eq!(mem.len(), 1);
        assert!(!mem.is_empty());
    }

    #[test]
    fn test_insert_with_ttl_and_evict_expired() {
        let mem = SharedMemory::new(test_swarm_id());

        // Insert entry with 1ms TTL (will expire immediately)
        mem.insert_with_options(
            "short-lived".to_string(),
            serde_json::json!("data"),
            test_agent(),
            Visibility::default_internal(),
            Some(StdDuration::from_millis(1)),
        );

        // Insert entry with 1 hour TTL (won't expire)
        mem.insert_with_options(
            "long-lived".to_string(),
            serde_json::json!("data"),
            test_agent(),
            Visibility::default_internal(),
            Some(StdDuration::from_secs(3600)),
        );

        // Insert entry with no TTL (won't expire)
        mem.insert("permanent".to_string(), serde_json::json!("data"), test_agent());

        assert_eq!(mem.len(), 3);

        // Wait for short-lived entry to expire
        thread::sleep(StdDuration::from_millis(10));

        let removed = mem.evict_expired();
        assert_eq!(removed, 1, "Should remove exactly 1 expired entry");
        assert_eq!(mem.len(), 2, "Should have 2 entries remaining");

        // Verify the right entries remain
        assert!(mem.get("short-lived").is_none());
        assert!(mem.get("long-lived").is_some());
        assert!(mem.get("permanent").is_some());
    }

    #[test]
    fn test_query_by_pattern() {
        let mem = SharedMemory::new(test_swarm_id());

        mem.insert("user:alice".to_string(), serde_json::json!("data1"), test_agent());
        mem.insert("user:bob".to_string(), serde_json::json!("data2"), test_agent());
        mem.insert("config:timeout".to_string(), serde_json::json!(30), test_agent());

        let user_entries = mem.query("user:");
        assert_eq!(user_entries.len(), 2);

        let config_entries = mem.query("config:");
        assert_eq!(config_entries.len(), 1);

        let alice_entries = mem.query("alice");
        assert_eq!(alice_entries.len(), 1);
    }

    #[test]
    fn test_store_and_get_task_results() {
        let mem = SharedMemory::new(test_swarm_id());

        let result1 = serde_json::json!({"status": "completed", "output": "success"});
        let result2 = serde_json::json!({"status": "failed", "error": "timeout"});

        mem.store_task_result("task-1", result1.clone());
        mem.store_task_result("task-2", result2.clone());

        assert_eq!(mem.get_task_result("task-1"), Some(result1.clone()));
        assert_eq!(mem.get_task_result("task-2"), Some(result2.clone()));
        assert_eq!(mem.get_task_result("task-3"), None);

        let all_results = mem.all_task_results();
        assert_eq!(all_results.len(), 2);
        assert_eq!(all_results.get("task-1"), Some(&result1));
        assert_eq!(all_results.get("task-2"), Some(&result2));
    }

    #[test]
    fn test_version_increments_on_writes() {
        let mem = SharedMemory::new(test_swarm_id());

        assert_eq!(mem.version(), 0);

        mem.insert("key1".to_string(), serde_json::json!("value1"), test_agent());
        assert_eq!(mem.version(), 1);

        mem.insert("key2".to_string(), serde_json::json!("value2"), test_agent());
        assert_eq!(mem.version(), 2);

        mem.store_task_result("task-1", serde_json::json!("result"));
        assert_eq!(mem.version(), 3);

        // Version should not increment when nothing is evicted
        let removed = mem.evict_expired();
        assert_eq!(removed, 0);
        assert_eq!(mem.version(), 3);
    }

    #[test]
    fn test_save_and_load_persistence() {
        let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
        let persist_path = temp_dir.path().join("memory.json");

        // Create memory with data
        let mem1 = SharedMemory::with_persistence(test_swarm_id(), persist_path.clone());
        mem1.insert("key1".to_string(), serde_json::json!("value1"), test_agent());
        mem1.insert("key2".to_string(), serde_json::json!(42), test_agent());
        mem1.store_task_result("task-1", serde_json::json!({"status": "done"}));

        // Save to disk
        mem1.save().expect("Save should succeed");

        // Load from disk
        let mem2 = SharedMemory::load(persist_path).expect("Load should succeed");

        // Verify data is preserved
        assert_eq!(mem2.get("key1"), Some(serde_json::json!("value1")));
        assert_eq!(mem2.get("key2"), Some(serde_json::json!(42)));
        assert_eq!(
            mem2.get_task_result("task-1"),
            Some(serde_json::json!({"status": "done"}))
        );
        assert_eq!(mem2.len(), 2);
        assert_eq!(mem2.version(), mem1.version());
    }

    #[test]
    fn test_get_entry_returns_full_entry() {
        let mem = SharedMemory::new(test_swarm_id());
        let agent = test_agent();
        let visibility = Visibility {
            agent_visible: true,
            coordinator_visible: false,
            user_visible: true,
        };

        mem.insert_with_options(
            "key1".to_string(),
            serde_json::json!("value1"),
            agent.clone(),
            visibility.clone(),
            Some(StdDuration::from_secs(60)),
        );

        let entry = mem.get_entry("key1").expect("Entry should exist");

        assert_eq!(entry.key, "key1");
        assert_eq!(entry.value, serde_json::json!("value1"));
        assert!(matches!(entry.author, AgentId::Queen(_)));
        assert_eq!(entry.visibility.agent_visible, visibility.agent_visible);
        assert_eq!(entry.visibility.coordinator_visible, visibility.coordinator_visible);
        assert_eq!(entry.visibility.user_visible, visibility.user_visible);
        assert!(entry.ttl.is_some());
    }

    #[test]
    fn test_evict_removes_only_expired() {
        let mem = SharedMemory::new(test_swarm_id());

        // Add 3 entries: 2 expired, 1 valid
        mem.insert_with_options(
            "expired1".to_string(),
            serde_json::json!("data"),
            test_agent(),
            Visibility::default_internal(),
            Some(StdDuration::from_millis(1)),
        );

        mem.insert_with_options(
            "expired2".to_string(),
            serde_json::json!("data"),
            test_agent(),
            Visibility::default_internal(),
            Some(StdDuration::from_millis(1)),
        );

        mem.insert_with_options(
            "valid".to_string(),
            serde_json::json!("data"),
            test_agent(),
            Visibility::default_internal(),
            Some(StdDuration::from_secs(3600)),
        );

        thread::sleep(StdDuration::from_millis(10));

        let initial_version = mem.version();
        let removed = mem.evict_expired();

        assert_eq!(removed, 2, "Should remove 2 expired entries");
        assert_eq!(mem.len(), 1, "Should have 1 entry remaining");
        assert!(mem.get("valid").is_some());
        assert!(mem.get("expired1").is_none());
        assert!(mem.get("expired2").is_none());
        assert_eq!(mem.version(), initial_version + 1, "Version should increment");
    }
}
