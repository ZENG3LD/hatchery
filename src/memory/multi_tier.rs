//! Multi-tier memory implementation.
//!
//! Four-tier memory system:
//! - Short-term: Per-agent temporary data with TTL
//! - Long-term: Persistent data across sessions
//! - Entity: Files, functions, concepts with relationships
//! - Contextual: Per-task scoped data

use super::Memory;
use crate::core::types::AgentId;
use anyhow::{Context, Result};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Configuration for MultiTierMemory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiTierConfig {
    /// Time-to-live for short-term memory entries.
    pub short_term_ttl: Duration,
    /// Interval for automatic eviction checks.
    pub eviction_interval: Duration,
    /// Access count threshold for promoting short-term to long-term.
    pub promotion_threshold: u32,
}

impl Default for MultiTierConfig {
    fn default() -> Self {
        MultiTierConfig {
            short_term_ttl: Duration::from_secs(3600), // 1 hour
            eviction_interval: Duration::from_secs(300), // 5 minutes
            promotion_threshold: 5,
        }
    }
}

/// Type of entity stored in entity memory.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum EntityType {
    File,
    Function,
    Concept,
    Module,
    Variable,
}

/// Entity memory entry with relationships.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityMemory {
    pub entity_type: EntityType,
    pub references: Vec<String>,
    pub attributes: HashMap<String, Value>,
}

/// Memory entry with metadata and access tracking.
#[derive(Debug, Clone)]
struct MemoryEntry {
    value: Value,
    source: String,
    created_at: Instant,
    updated_at: Instant,
    last_accessed: Instant,
    access_count: u32,
}

/// Serializable memory entry for snapshots.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SerializableEntry {
    value: Value,
    source: String,
    created_secs: u64,
    updated_secs: u64,
    accessed_secs: u64,
    access_count: u32,
}

/// Multi-tier memory with automatic tier promotion.
pub struct MultiTierMemory {
    config: MultiTierConfig,
    // Agent-specific short-term memory: agent_key -> key -> entry
    short_term: Arc<RwLock<HashMap<String, HashMap<String, MemoryEntry>>>>,
    // Persistent long-term memory
    long_term: Arc<RwLock<HashMap<String, MemoryEntry>>>,
    // Entity memory: files, functions, concepts
    entity: Arc<RwLock<HashMap<String, EntityMemory>>>,
    // Task-scoped contextual memory: task_id -> key -> entry
    contextual: Arc<RwLock<HashMap<String, HashMap<String, MemoryEntry>>>>,
    start_time: Instant,
    last_eviction: Arc<RwLock<Instant>>,
}

impl MultiTierMemory {
    /// Create a new MultiTierMemory with default configuration.
    pub fn new() -> Self {
        Self::with_config(MultiTierConfig::default())
    }

    /// Create a new MultiTierMemory with custom configuration.
    pub fn with_config(config: MultiTierConfig) -> Self {
        let now = Instant::now();
        MultiTierMemory {
            config,
            short_term: Arc::new(RwLock::new(HashMap::new())),
            long_term: Arc::new(RwLock::new(HashMap::new())),
            entity: Arc::new(RwLock::new(HashMap::new())),
            contextual: Arc::new(RwLock::new(HashMap::new())),
            start_time: now,
            last_eviction: Arc::new(RwLock::new(now)),
        }
    }

    /// Get statistics for each tier.
    pub fn tier_stats(&self) -> (usize, usize, usize, usize) {
        let short_count: usize = self.short_term.read().values().map(|m| m.len()).sum();
        let long_count = self.long_term.read().len();
        let entity_count = self.entity.read().len();
        let ctx_count: usize = self.contextual.read().values().map(|m| m.len()).sum();
        (short_count, long_count, entity_count, ctx_count)
    }

    /// Add an entity to entity memory.
    pub fn add_entity(
        &mut self,
        key: String,
        entity_type: EntityType,
        attributes: HashMap<String, Value>,
    ) -> Result<()> {
        let mut entity_mem = self.entity.write();
        entity_mem.insert(
            key,
            EntityMemory {
                entity_type,
                references: Vec::new(),
                attributes,
            },
        );
        Ok(())
    }

    /// Link two entities with a reference.
    pub fn link_entities(&mut self, from: &str, to: &str) -> Result<()> {
        let mut entity_mem = self.entity.write();
        if let Some(entity) = entity_mem.get_mut(from) {
            if !entity.references.contains(&to.to_string()) {
                entity.references.push(to.to_string());
            }
        }
        Ok(())
    }

    /// Get entity by key.
    pub fn get_entity(&self, key: &str) -> Option<EntityMemory> {
        self.entity.read().get(key).cloned()
    }

    /// Parse key prefix to determine tier.
    fn parse_tier(key: &str) -> (&str, &str) {
        if let Some(colon_pos) = key.find(':') {
            let prefix = &key[..colon_pos];
            let rest = &key[colon_pos + 1..];
            (prefix, rest)
        } else {
            ("long", key) // Default to long-term if no prefix
        }
    }

    /// Convert AgentId to string.
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

    /// Check if entry should be promoted from short-term to long-term.
    fn should_promote(&self, entry: &MemoryEntry) -> bool {
        entry.access_count >= self.config.promotion_threshold
    }

    /// Promote an entry from short-term to long-term.
    fn promote_to_long_term(&self, key: String, entry: MemoryEntry) {
        let mut long_term = self.long_term.write();
        long_term.insert(key.clone(), entry);
        eprintln!("[MultiTierMemory] Promoted key to long-term: {}", key);
    }
}

impl Default for MultiTierMemory {
    fn default() -> Self {
        Self::new()
    }
}

impl Memory for MultiTierMemory {
    fn insert(&mut self, key: String, value: Value, source: AgentId) -> Result<()> {
        let source_str = Self::agent_id_to_string(&source);
        let now = Instant::now();

        let (tier, rest_key) = Self::parse_tier(&key);

        match tier {
            "short" => {
                // Format: "short:<agent_key>:<key>"
                let parts: Vec<&str> = rest_key.splitn(2, ':').collect();
                if parts.len() != 2 {
                    anyhow::bail!("Invalid short-term key format: {}", key);
                }
                let agent_key = parts[0];
                let actual_key = parts[1];

                let mut short_term = self.short_term.write();
                let agent_mem = short_term.entry(agent_key.to_string()).or_insert_with(HashMap::new);

                agent_mem.insert(
                    actual_key.to_string(),
                    MemoryEntry {
                        value,
                        source: source_str,
                        created_at: now,
                        updated_at: now,
                        last_accessed: now,
                        access_count: 0,
                    },
                );
            }
            "long" => {
                let mut long_term = self.long_term.write();
                long_term.insert(
                    rest_key.to_string(),
                    MemoryEntry {
                        value,
                        source: source_str,
                        created_at: now,
                        updated_at: now,
                        last_accessed: now,
                        access_count: 0,
                    },
                );
            }
            "entity" => {
                // Format: "entity:<entity_key>"
                // Value should contain entity_type and attributes
                if let Some(obj) = value.as_object() {
                    let entity_type = obj
                        .get("entity_type")
                        .and_then(|v| serde_json::from_value(v.clone()).ok())
                        .unwrap_or(EntityType::Concept);

                    let attributes = obj
                        .get("attributes")
                        .and_then(|v| serde_json::from_value(v.clone()).ok())
                        .unwrap_or_default();

                    let mut entity_mem = self.entity.write();
                    entity_mem.insert(
                        rest_key.to_string(),
                        EntityMemory {
                            entity_type,
                            references: Vec::new(),
                            attributes,
                        },
                    );
                }
            }
            "ctx" => {
                // Format: "ctx:<task_id>:<key>"
                let parts: Vec<&str> = rest_key.splitn(2, ':').collect();
                if parts.len() != 2 {
                    anyhow::bail!("Invalid contextual key format: {}", key);
                }
                let task_id = parts[0];
                let actual_key = parts[1];

                let mut contextual = self.contextual.write();
                let task_mem = contextual.entry(task_id.to_string()).or_insert_with(HashMap::new);

                task_mem.insert(
                    actual_key.to_string(),
                    MemoryEntry {
                        value,
                        source: source_str,
                        created_at: now,
                        updated_at: now,
                        last_accessed: now,
                        access_count: 0,
                    },
                );
            }
            _ => {
                // Unknown tier, default to long-term
                let mut long_term = self.long_term.write();
                long_term.insert(
                    key,
                    MemoryEntry {
                        value,
                        source: source_str,
                        created_at: now,
                        updated_at: now,
                        last_accessed: now,
                        access_count: 0,
                    },
                );
            }
        }

        Ok(())
    }

    fn get(&self, key: &str) -> Option<Value> {
        let (tier, rest_key) = Self::parse_tier(key);

        match tier {
            "short" => {
                let parts: Vec<&str> = rest_key.splitn(2, ':').collect();
                if parts.len() != 2 {
                    return None;
                }
                let agent_key = parts[0];
                let actual_key = parts[1];

                let mut short_term = self.short_term.write();
                if let Some(agent_mem) = short_term.get_mut(agent_key) {
                    if let Some(entry) = agent_mem.get_mut(actual_key) {
                        entry.last_accessed = Instant::now();
                        entry.access_count += 1;

                        let value = entry.value.clone();
                        let should_promote = self.should_promote(entry);

                        // Check for promotion
                        if should_promote {
                            let promoted_entry = entry.clone();
                            let promoted_key = format!("long:{}", actual_key);
                            drop(short_term); // Release lock before promotion
                            self.promote_to_long_term(promoted_key, promoted_entry);
                            return Some(value);
                        }

                        return Some(value);
                    }
                }
                None
            }
            "long" => {
                let mut long_term = self.long_term.write();
                long_term.get_mut(rest_key).map(|entry| {
                    entry.last_accessed = Instant::now();
                    entry.access_count += 1;
                    entry.value.clone()
                })
            }
            "entity" => {
                let entity_mem = self.entity.read();
                entity_mem.get(rest_key).map(|entity| {
                    serde_json::to_value(entity).unwrap_or(Value::Null)
                })
            }
            "ctx" => {
                let parts: Vec<&str> = rest_key.splitn(2, ':').collect();
                if parts.len() != 2 {
                    return None;
                }
                let task_id = parts[0];
                let actual_key = parts[1];

                let mut contextual = self.contextual.write();
                if let Some(task_mem) = contextual.get_mut(task_id) {
                    task_mem.get_mut(actual_key).map(|entry| {
                        entry.last_accessed = Instant::now();
                        entry.access_count += 1;
                        entry.value.clone()
                    })
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn query(&self, pattern: &str) -> Result<Vec<(String, Value)>> {
        let mut results = Vec::new();

        // Search contextual tier
        let contextual = self.contextual.read();
        for (task_id, task_mem) in contextual.iter() {
            for (key, entry) in task_mem.iter() {
                let full_key = format!("ctx:{}:{}", task_id, key);
                if full_key.contains(pattern) {
                    results.push((full_key, entry.value.clone()));
                }
            }
        }

        // Search short-term tier
        let short_term = self.short_term.read();
        for (agent_key, agent_mem) in short_term.iter() {
            for (key, entry) in agent_mem.iter() {
                let full_key = format!("short:{}:{}", agent_key, key);
                if full_key.contains(pattern) || key.contains(pattern) {
                    results.push((full_key, entry.value.clone()));
                }
            }
        }

        // Search entity tier
        let entity_mem = self.entity.read();
        for (key, entity) in entity_mem.iter() {
            if key.contains(pattern) {
                let full_key = format!("entity:{}", key);
                if let Ok(value) = serde_json::to_value(entity) {
                    results.push((full_key, value));
                }
            }
        }

        // Search long-term tier
        let long_term = self.long_term.read();
        for (key, entry) in long_term.iter() {
            let full_key = format!("long:{}", key);
            if full_key.contains(pattern) || key.contains(pattern) {
                results.push((full_key, entry.value.clone()));
            }
        }

        Ok(results)
    }

    fn evict(&mut self) -> Result<usize> {
        let now = Instant::now();
        let mut evicted = 0;

        // Evict expired short-term entries
        let mut short_term = self.short_term.write();
        for agent_mem in short_term.values_mut() {
            agent_mem.retain(|_, entry| {
                let age = now.duration_since(entry.created_at);
                let keep = age < self.config.short_term_ttl;
                if !keep {
                    evicted += 1;
                }
                keep
            });
        }

        // Remove empty agent buckets
        short_term.retain(|_, agent_mem| !agent_mem.is_empty());

        *self.last_eviction.write() = now;

        Ok(evicted)
    }

    fn snapshot(&self) -> Result<Value> {
        let mut snapshot = serde_json::Map::new();

        // Snapshot short-term
        let short_term = self.short_term.read();
        let short_snapshot: HashMap<String, HashMap<String, SerializableEntry>> = short_term
            .iter()
            .map(|(agent_key, agent_mem)| {
                let serializable_mem: HashMap<String, SerializableEntry> = agent_mem
                    .iter()
                    .map(|(key, entry)| {
                        (
                            key.clone(),
                            SerializableEntry {
                                value: entry.value.clone(),
                                source: entry.source.clone(),
                                created_secs: entry.created_at.duration_since(self.start_time).as_secs(),
                                updated_secs: entry.updated_at.duration_since(self.start_time).as_secs(),
                                accessed_secs: entry.last_accessed.duration_since(self.start_time).as_secs(),
                                access_count: entry.access_count,
                            },
                        )
                    })
                    .collect();
                (agent_key.clone(), serializable_mem)
            })
            .collect();
        snapshot.insert("short_term".to_string(), serde_json::to_value(short_snapshot)?);

        // Snapshot long-term
        let long_term = self.long_term.read();
        let long_snapshot: HashMap<String, SerializableEntry> = long_term
            .iter()
            .map(|(key, entry)| {
                (
                    key.clone(),
                    SerializableEntry {
                        value: entry.value.clone(),
                        source: entry.source.clone(),
                        created_secs: entry.created_at.duration_since(self.start_time).as_secs(),
                        updated_secs: entry.updated_at.duration_since(self.start_time).as_secs(),
                        accessed_secs: entry.last_accessed.duration_since(self.start_time).as_secs(),
                        access_count: entry.access_count,
                    },
                )
            })
            .collect();
        snapshot.insert("long_term".to_string(), serde_json::to_value(long_snapshot)?);

        // Snapshot entity
        let entity = self.entity.read();
        snapshot.insert("entity".to_string(), serde_json::to_value(&*entity)?);

        // Snapshot contextual
        let contextual = self.contextual.read();
        let ctx_snapshot: HashMap<String, HashMap<String, SerializableEntry>> = contextual
            .iter()
            .map(|(task_id, task_mem)| {
                let serializable_mem: HashMap<String, SerializableEntry> = task_mem
                    .iter()
                    .map(|(key, entry)| {
                        (
                            key.clone(),
                            SerializableEntry {
                                value: entry.value.clone(),
                                source: entry.source.clone(),
                                created_secs: entry.created_at.duration_since(self.start_time).as_secs(),
                                updated_secs: entry.updated_at.duration_since(self.start_time).as_secs(),
                                accessed_secs: entry.last_accessed.duration_since(self.start_time).as_secs(),
                                access_count: entry.access_count,
                            },
                        )
                    })
                    .collect();
                (task_id.clone(), serializable_mem)
            })
            .collect();
        snapshot.insert("contextual".to_string(), serde_json::to_value(ctx_snapshot)?);

        Ok(Value::Object(snapshot))
    }

    fn restore(&mut self, snapshot: Value) -> Result<()> {
        let obj = snapshot.as_object().context("Snapshot must be an object")?;

        // Restore short-term
        if let Some(short_val) = obj.get("short_term") {
            let short_snapshot: HashMap<String, HashMap<String, SerializableEntry>> =
                serde_json::from_value(short_val.clone())?;
            let mut short_term = self.short_term.write();
            short_term.clear();

            for (agent_key, agent_snapshot) in short_snapshot {
                let agent_mem: HashMap<String, MemoryEntry> = agent_snapshot
                    .into_iter()
                    .map(|(key, entry)| {
                        (
                            key,
                            MemoryEntry {
                                value: entry.value,
                                source: entry.source,
                                created_at: self.start_time + Duration::from_secs(entry.created_secs),
                                updated_at: self.start_time + Duration::from_secs(entry.updated_secs),
                                last_accessed: self.start_time + Duration::from_secs(entry.accessed_secs),
                                access_count: entry.access_count,
                            },
                        )
                    })
                    .collect();
                short_term.insert(agent_key, agent_mem);
            }
        }

        // Restore long-term
        if let Some(long_val) = obj.get("long_term") {
            let long_snapshot: HashMap<String, SerializableEntry> =
                serde_json::from_value(long_val.clone())?;
            let mut long_term = self.long_term.write();
            long_term.clear();

            for (key, entry) in long_snapshot {
                long_term.insert(
                    key,
                    MemoryEntry {
                        value: entry.value,
                        source: entry.source,
                        created_at: self.start_time + Duration::from_secs(entry.created_secs),
                        updated_at: self.start_time + Duration::from_secs(entry.updated_secs),
                        last_accessed: self.start_time + Duration::from_secs(entry.accessed_secs),
                        access_count: entry.access_count,
                    },
                );
            }
        }

        // Restore entity
        if let Some(entity_val) = obj.get("entity") {
            let entity_snapshot: HashMap<String, EntityMemory> =
                serde_json::from_value(entity_val.clone())?;
            let mut entity = self.entity.write();
            *entity = entity_snapshot;
        }

        // Restore contextual
        if let Some(ctx_val) = obj.get("contextual") {
            let ctx_snapshot: HashMap<String, HashMap<String, SerializableEntry>> =
                serde_json::from_value(ctx_val.clone())?;
            let mut contextual = self.contextual.write();
            contextual.clear();

            for (task_id, task_snapshot) in ctx_snapshot {
                let task_mem: HashMap<String, MemoryEntry> = task_snapshot
                    .into_iter()
                    .map(|(key, entry)| {
                        (
                            key,
                            MemoryEntry {
                                value: entry.value,
                                source: entry.source,
                                created_at: self.start_time + Duration::from_secs(entry.created_secs),
                                updated_at: self.start_time + Duration::from_secs(entry.updated_secs),
                                last_accessed: self.start_time + Duration::from_secs(entry.accessed_secs),
                                access_count: entry.access_count,
                            },
                        )
                    })
                    .collect();
                contextual.insert(task_id, task_mem);
            }
        }

        Ok(())
    }
}
