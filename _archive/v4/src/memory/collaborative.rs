//! Collaborative memory implementation.
//!
//! Dual-tier memory with access control: private per-agent storage and shared
//! team space with fine-grained read/write permissions.

use super::Memory;
use crate::core::types::AgentId;
use anyhow::{Context, Result};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

/// Configuration for CollaborativeMemory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollaborativeConfig {
    /// Default access control for new entries.
    pub default_access: AccessControl,
}

impl Default for CollaborativeConfig {
    fn default() -> Self {
        CollaborativeConfig {
            default_access: AccessControl::Private,
        }
    }
}

/// Access control level for memory entries.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AccessControl {
    /// Only the owner can read and write.
    Private,
    /// Shared with specific agents (controlled by readers/writers lists).
    Shared,
    /// Anyone can read, only owner and writers can write.
    Public,
}

/// An entry with access control metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct AccessEntry {
    value: Value,
    owner: String,
    access_level: AccessControl,
    readers: Vec<String>,
    writers: Vec<String>,
}

/// Collaborative memory with private and shared tiers.
pub struct CollaborativeMemory {
    config: CollaborativeConfig,
    // Private memory: agent -> key -> entry
    private_memory: Arc<RwLock<HashMap<String, HashMap<String, AccessEntry>>>>,
    // Shared memory: key -> entry
    shared_memory: Arc<RwLock<HashMap<String, AccessEntry>>>,
    // Current agent for access checks
    current_agent: Arc<RwLock<Option<String>>>,
}

impl CollaborativeMemory {
    /// Create a new CollaborativeMemory with default configuration.
    pub fn new() -> Self {
        Self::with_config(CollaborativeConfig::default())
    }

    /// Create a new CollaborativeMemory with custom configuration.
    pub fn with_config(config: CollaborativeConfig) -> Self {
        CollaborativeMemory {
            config,
            private_memory: Arc::new(RwLock::new(HashMap::new())),
            shared_memory: Arc::new(RwLock::new(HashMap::new())),
            current_agent: Arc::new(RwLock::new(None)),
        }
    }

    /// Set the current agent context for access control.
    pub fn set_current_agent(&mut self, agent: AgentId) {
        let agent_key = Self::agent_id_to_key(&agent);
        *self.current_agent.write() = Some(agent_key);
    }

    /// Clear the current agent context.
    pub fn clear_current_agent(&mut self) {
        *self.current_agent.write() = None;
    }

    /// Share a private entry to the shared tier.
    pub fn share(&mut self, key: &str, access_level: AccessControl) -> Result<()> {
        let current = self.current_agent.read().clone();
        let agent_key = current.context("No current agent set")?;

        let mut private_memory = self.private_memory.write();
        let agent_mem = private_memory
            .get_mut(&agent_key)
            .context("Agent private memory not found")?;

        let entry = agent_mem
            .remove(key)
            .context("Entry not found in private memory")?;

        let mut shared_memory = self.shared_memory.write();
        let mut shared_entry = entry;
        shared_entry.access_level = access_level;

        shared_memory.insert(key.to_string(), shared_entry);

        eprintln!("[CollaborativeMemory] Shared key to team: {}", key);
        Ok(())
    }

    /// Grant read access to another agent.
    pub fn grant_read_access(&mut self, key: &str, agent: AgentId) -> Result<()> {
        let agent_key = Self::agent_id_to_key(&agent);
        let mut shared_memory = self.shared_memory.write();

        let entry = shared_memory
            .get_mut(key)
            .context("Entry not found in shared memory")?;

        if !entry.readers.contains(&agent_key) {
            entry.readers.push(agent_key.clone());
        }

        eprintln!("[CollaborativeMemory] Granted read access to {} for key: {}", agent_key, key);
        Ok(())
    }

    /// Grant write access to another agent.
    pub fn grant_write_access(&mut self, key: &str, agent: AgentId) -> Result<()> {
        let agent_key = Self::agent_id_to_key(&agent);
        let mut shared_memory = self.shared_memory.write();

        let entry = shared_memory
            .get_mut(key)
            .context("Entry not found in shared memory")?;

        if !entry.writers.contains(&agent_key) {
            entry.writers.push(agent_key.clone());
        }
        // Writers automatically get read access
        if !entry.readers.contains(&agent_key) {
            entry.readers.push(agent_key.clone());
        }

        eprintln!("[CollaborativeMemory] Granted write access to {} for key: {}", agent_key, key);
        Ok(())
    }

    /// Revoke read access from an agent.
    pub fn revoke_read_access(&mut self, key: &str, agent: AgentId) -> Result<()> {
        let agent_key = Self::agent_id_to_key(&agent);
        let mut shared_memory = self.shared_memory.write();

        let entry = shared_memory
            .get_mut(key)
            .context("Entry not found in shared memory")?;

        entry.readers.retain(|a| a != &agent_key);

        eprintln!("[CollaborativeMemory] Revoked read access from {} for key: {}", agent_key, key);
        Ok(())
    }

    /// Revoke write access from an agent.
    pub fn revoke_write_access(&mut self, key: &str, agent: AgentId) -> Result<()> {
        let agent_key = Self::agent_id_to_key(&agent);
        let mut shared_memory = self.shared_memory.write();

        let entry = shared_memory
            .get_mut(key)
            .context("Entry not found in shared memory")?;

        entry.writers.retain(|a| a != &agent_key);

        eprintln!("[CollaborativeMemory] Revoked write access from {} for key: {}", agent_key, key);
        Ok(())
    }

    /// Check if current agent has read access to an entry.
    fn has_read_access(&self, entry: &AccessEntry, current_agent: &str) -> bool {
        match entry.access_level {
            AccessControl::Private => entry.owner == current_agent,
            AccessControl::Shared => {
                entry.owner == current_agent || entry.readers.contains(&current_agent.to_string())
            }
            AccessControl::Public => true,
        }
    }

    /// Check if current agent has write access to an entry.
    fn has_write_access(&self, entry: &AccessEntry, current_agent: &str) -> bool {
        match entry.access_level {
            AccessControl::Private => entry.owner == current_agent,
            AccessControl::Shared => {
                entry.owner == current_agent || entry.writers.contains(&current_agent.to_string())
            }
            AccessControl::Public => {
                entry.owner == current_agent || entry.writers.contains(&current_agent.to_string())
            }
        }
    }

    /// Convert AgentId to key.
    fn agent_id_to_key(agent: &AgentId) -> String {
        match agent {
            AgentId::Nydus(id) => format!("nydus:{}", id.0),
            AgentId::Queen(id) => format!("queen:{}", id.0),
            AgentId::Overlord(id) => format!("overlord:{}", id.0),
            AgentId::Overmind(id) => format!("overmind:{}", id.0),
            AgentId::Validator => "validator".to_string(),
            AgentId::Operator => "operator".to_string(),
        }
    }

    /// Parse access control from value.
    fn parse_access_control(value: &Value) -> AccessControl {
        if let Some(obj) = value.as_object() {
            if let Some(access_val) = obj.get("access_level") {
                return serde_json::from_value(access_val.clone())
                    .unwrap_or(AccessControl::Private);
            }
        }
        AccessControl::Private
    }
}

impl Default for CollaborativeMemory {
    fn default() -> Self {
        Self::new()
    }
}

impl Memory for CollaborativeMemory {
    fn insert(&mut self, key: String, value: Value, source: AgentId) -> Result<()> {
        let owner = Self::agent_id_to_key(&source);
        let access_level = Self::parse_access_control(&value);

        let current = self.current_agent.read().clone();

        let entry = AccessEntry {
            value: value.clone(),
            owner: owner.clone(),
            access_level: access_level.clone(),
            readers: Vec::new(),
            writers: Vec::new(),
        };

        match access_level {
            AccessControl::Private => {
                // Store in private memory
                let mut private_memory = self.private_memory.write();
                let agent_mem = private_memory.entry(owner).or_insert_with(HashMap::new);
                agent_mem.insert(key, entry);
            }
            AccessControl::Shared | AccessControl::Public => {
                // Store in shared memory
                let mut shared_memory = self.shared_memory.write();

                // Check write permissions if entry exists
                if let Some(existing) = shared_memory.get(&key) {
                    let current_agent = current.as_ref().unwrap_or(&owner);
                    if !self.has_write_access(existing, current_agent) {
                        anyhow::bail!("Write access denied for key: {}", key);
                    }
                }

                shared_memory.insert(key, entry);
            }
        }

        Ok(())
    }

    fn get(&self, key: &str) -> Option<Value> {
        let current = self.current_agent.read();
        let current_agent = current.as_ref()?;

        // Check shared memory first
        let shared_memory = self.shared_memory.read();
        if let Some(entry) = shared_memory.get(key) {
            if self.has_read_access(entry, current_agent) {
                return Some(entry.value.clone());
            }
        }

        // Check private memory
        let private_memory = self.private_memory.read();
        if let Some(agent_mem) = private_memory.get(current_agent) {
            if let Some(entry) = agent_mem.get(key) {
                return Some(entry.value.clone());
            }
        }

        None
    }

    fn query(&self, pattern: &str) -> Result<Vec<(String, Value)>> {
        let current = self.current_agent.read();
        let current_agent = current.as_ref().context("No current agent set")?;
        let mut results = Vec::new();

        // Search shared memory
        let shared_memory = self.shared_memory.read();
        for (key, entry) in shared_memory.iter() {
            if key.contains(pattern) && self.has_read_access(entry, current_agent) {
                results.push((key.clone(), entry.value.clone()));
            }
        }

        // Search private memory
        let private_memory = self.private_memory.read();
        if let Some(agent_mem) = private_memory.get(current_agent) {
            for (key, entry) in agent_mem.iter() {
                if key.contains(pattern) {
                    results.push((key.clone(), entry.value.clone()));
                }
            }
        }

        Ok(results)
    }

    fn evict(&mut self) -> Result<usize> {
        // No automatic eviction - access-controlled memory persists
        Ok(0)
    }

    fn snapshot(&self) -> Result<Value> {
        let mut snapshot = serde_json::Map::new();

        let private_memory = self.private_memory.read();
        snapshot.insert("private".to_string(), serde_json::to_value(&*private_memory)?);

        let shared_memory = self.shared_memory.read();
        snapshot.insert("shared".to_string(), serde_json::to_value(&*shared_memory)?);

        Ok(Value::Object(snapshot))
    }

    fn restore(&mut self, snapshot: Value) -> Result<()> {
        let obj = snapshot.as_object().context("Snapshot must be an object")?;

        if let Some(private_val) = obj.get("private") {
            let private_restored: HashMap<String, HashMap<String, AccessEntry>> =
                serde_json::from_value(private_val.clone())?;
            let mut private_memory = self.private_memory.write();
            *private_memory = private_restored;
        }

        if let Some(shared_val) = obj.get("shared") {
            let shared_restored: HashMap<String, AccessEntry> =
                serde_json::from_value(shared_val.clone())?;
            let mut shared_memory = self.shared_memory.write();
            *shared_memory = shared_restored;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::types::QueenId;

    #[test]
    fn test_private_memory() {
        let mut mem = CollaborativeMemory::new();
        let agent = AgentId::Queen(QueenId("Q0".to_string()));

        mem.set_current_agent(agent.clone());

        let value = serde_json::json!({"access_level": "Private", "data": "secret"});
        mem.insert("private_key".to_string(), value, agent).unwrap();

        let retrieved = mem.get("private_key");
        assert!(retrieved.is_some());
    }

    #[test]
    fn test_shared_memory() {
        let mut mem = CollaborativeMemory::new();
        let agent1 = AgentId::Queen(QueenId("Q0".to_string()));
        let agent2 = AgentId::Queen(QueenId("Q1".to_string()));

        mem.set_current_agent(agent1.clone());

        let value = serde_json::json!({"access_level": "Shared", "data": "team_data"});
        mem.insert("shared_key".to_string(), value, agent1).unwrap();

        // Grant read access to agent2
        mem.grant_read_access("shared_key", agent2.clone()).unwrap();

        // Switch to agent2
        mem.set_current_agent(agent2);
        let retrieved = mem.get("shared_key");
        assert!(retrieved.is_some());
    }

    #[test]
    fn test_access_control() {
        let mut mem = CollaborativeMemory::new();
        let agent1 = AgentId::Queen(QueenId("Q0".to_string()));
        let agent2 = AgentId::Queen(QueenId("Q1".to_string()));

        mem.set_current_agent(agent1.clone());

        let value = serde_json::json!({"access_level": "Private", "data": "secret"});
        mem.insert("private_key".to_string(), value, agent1).unwrap();

        // Switch to agent2 - should not see private data
        mem.set_current_agent(agent2);
        assert_eq!(mem.get("private_key"), None);
    }

    #[test]
    fn test_public_access() {
        let mut mem = CollaborativeMemory::new();
        let agent1 = AgentId::Queen(QueenId("Q0".to_string()));
        let agent2 = AgentId::Queen(QueenId("Q1".to_string()));

        mem.set_current_agent(agent1.clone());

        let value = serde_json::json!({"access_level": "Public", "data": "public_data"});
        mem.insert("public_key".to_string(), value, agent1).unwrap();

        // Anyone can read public data
        mem.set_current_agent(agent2);
        let retrieved = mem.get("public_key");
        assert!(retrieved.is_some());
    }
}
