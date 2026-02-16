//! Isolated memory implementation.
//!
//! Per-agent isolated namespaced storage. Each agent has its own memory space
//! and cannot access other agents' data.

use super::Memory;
use crate::core::types::AgentId;
use anyhow::{Context, Result};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

/// Configuration for IsolatedMemory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IsolatedConfig {
    /// Base directory for isolated storage (optional, for persistence).
    pub base_dir: Option<PathBuf>,
}

impl Default for IsolatedConfig {
    fn default() -> Self {
        IsolatedConfig { base_dir: None }
    }
}

/// Isolated memory with per-agent namespaces.
pub struct IsolatedMemory {
    config: IsolatedConfig,
    // Agent namespace -> key -> value
    storage: Arc<RwLock<HashMap<String, HashMap<String, Value>>>>,
    // Current agent context for access control
    current_agent: Arc<RwLock<Option<String>>>,
}

impl IsolatedMemory {
    /// Create a new IsolatedMemory with default configuration.
    pub fn new() -> Self {
        Self::with_config(IsolatedConfig::default())
    }

    /// Create a new IsolatedMemory with custom configuration.
    pub fn with_config(config: IsolatedConfig) -> Self {
        IsolatedMemory {
            config,
            storage: Arc::new(RwLock::new(HashMap::new())),
            current_agent: Arc::new(RwLock::new(None)),
        }
    }

    /// Set the current agent context for subsequent operations.
    /// This determines which namespace can be accessed.
    pub fn set_current_agent(&mut self, agent: AgentId) {
        let namespace = Self::agent_id_to_namespace(&agent);
        *self.current_agent.write() = Some(namespace);
    }

    /// Clear the current agent context.
    pub fn clear_current_agent(&mut self) {
        *self.current_agent.write() = None;
    }

    /// Get the number of namespaces.
    pub fn namespace_count(&self) -> usize {
        self.storage.read().len()
    }

    /// Get the number of entries in a specific namespace.
    pub fn namespace_size(&self, namespace: &str) -> usize {
        self.storage
            .read()
            .get(namespace)
            .map(|ns| ns.len())
            .unwrap_or(0)
    }

    /// List all namespaces.
    pub fn list_namespaces(&self) -> Vec<String> {
        self.storage.read().keys().cloned().collect()
    }

    /// Get all keys in the current agent's namespace.
    pub fn keys_in_current_namespace(&self) -> Vec<String> {
        let current = self.current_agent.read();
        if let Some(namespace) = current.as_ref() {
            self.storage
                .read()
                .get(namespace)
                .map(|ns| ns.keys().cloned().collect())
                .unwrap_or_default()
        } else {
            Vec::new()
        }
    }

    /// Convert AgentId to namespace string.
    fn agent_id_to_namespace(agent: &AgentId) -> String {
        match agent {
            AgentId::Nydus(id) => format!("nydus_{}", id.0),
            AgentId::Queen(id) => format!("queen_{}", id.0),
            AgentId::Overlord(id) => format!("overlord_{}", id.0),
            AgentId::Overmind(id) => format!("overmind_{}", id.0),
            AgentId::Validator => "validator".to_string(),
            AgentId::Operator => "operator".to_string(),
        }
    }

    /// Check if current agent has access to a namespace.
    fn has_access(&self, namespace: &str) -> bool {
        let current = self.current_agent.read();
        if let Some(current_ns) = current.as_ref() {
            current_ns == namespace
        } else {
            // If no current agent set, allow access (for initial setup)
            true
        }
    }
}

impl Default for IsolatedMemory {
    fn default() -> Self {
        Self::new()
    }
}

impl Memory for IsolatedMemory {
    fn insert(&mut self, key: String, value: Value, source: AgentId) -> Result<()> {
        let namespace = Self::agent_id_to_namespace(&source);

        // Verify access
        if !self.has_access(&namespace) {
            anyhow::bail!("Access denied: cannot write to namespace {}", namespace);
        }

        let mut storage = self.storage.write();
        let agent_storage = storage.entry(namespace).or_insert_with(HashMap::new);
        agent_storage.insert(key, value);

        Ok(())
    }

    fn get(&self, key: &str) -> Option<Value> {
        let current = self.current_agent.read();
        let namespace = current.as_ref()?;

        let storage = self.storage.read();
        storage.get(namespace)?.get(key).cloned()
    }

    fn query(&self, pattern: &str) -> Result<Vec<(String, Value)>> {
        let current = self.current_agent.read();
        let namespace = current.as_ref().context("No current agent set for query")?;

        let storage = self.storage.read();
        let mut results = Vec::new();

        if let Some(agent_storage) = storage.get(namespace) {
            for (key, value) in agent_storage.iter() {
                if key.contains(pattern) {
                    results.push((key.clone(), value.clone()));
                }
            }
        }

        Ok(results)
    }

    fn evict(&mut self) -> Result<usize> {
        // No automatic eviction - isolated memory persists until explicitly cleared
        Ok(0)
    }

    fn snapshot(&self) -> Result<Value> {
        let storage = self.storage.read();
        serde_json::to_value(&*storage).context("Failed to serialize isolated memory snapshot")
    }

    fn restore(&mut self, snapshot: Value) -> Result<()> {
        let restored: HashMap<String, HashMap<String, Value>> =
            serde_json::from_value(snapshot).context("Failed to deserialize isolated memory snapshot")?;

        let mut storage = self.storage.write();
        *storage = restored;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::types::QueenId;

    #[test]
    fn test_isolated_namespaces() {
        let mut mem = IsolatedMemory::new();
        let agent1 = AgentId::Queen(QueenId("Q0".to_string()));
        let agent2 = AgentId::Queen(QueenId("Q1".to_string()));

        // Set context to agent1
        mem.set_current_agent(agent1.clone());

        mem.insert("key1".to_string(), Value::String("value1".to_string()), agent1.clone())
            .unwrap();

        // Switch to agent2
        mem.set_current_agent(agent2.clone());

        // Should not see agent1's data
        assert_eq!(mem.get("key1"), None);

        mem.insert("key2".to_string(), Value::String("value2".to_string()), agent2)
            .unwrap();

        // Switch back to agent1
        mem.set_current_agent(agent1);
        assert_eq!(mem.get("key1"), Some(Value::String("value1".to_string())));
        assert_eq!(mem.get("key2"), None); // Cannot see agent2's data
    }

    #[test]
    fn test_namespace_count() {
        let mut mem = IsolatedMemory::new();
        let agent1 = AgentId::Queen(QueenId("Q0".to_string()));
        let agent2 = AgentId::Queen(QueenId("Q1".to_string()));

        mem.set_current_agent(agent1.clone());
        mem.insert("k1".to_string(), Value::Null, agent1).unwrap();

        mem.set_current_agent(agent2.clone());
        mem.insert("k2".to_string(), Value::Null, agent2).unwrap();

        assert_eq!(mem.namespace_count(), 2);
    }

    #[test]
    fn test_access_control() {
        let mut mem = IsolatedMemory::new();
        let agent1 = AgentId::Queen(QueenId("Q0".to_string()));
        let agent2 = AgentId::Queen(QueenId("Q1".to_string()));

        // Set context to agent1
        mem.set_current_agent(agent1.clone());

        // Try to write to agent2's namespace - should fail
        let result = mem.insert("key".to_string(), Value::Null, agent2);
        assert!(result.is_err());
    }

    #[test]
    fn test_snapshot_restore() {
        let mut mem = IsolatedMemory::new();
        let agent = AgentId::Operator;

        mem.set_current_agent(agent.clone());
        mem.insert("k1".to_string(), Value::String("v1".to_string()), agent.clone())
            .unwrap();

        let snapshot = mem.snapshot().unwrap();

        let mut mem2 = IsolatedMemory::new();
        mem2.restore(snapshot).unwrap();
        mem2.set_current_agent(agent);

        assert_eq!(mem2.get("k1"), Some(Value::String("v1".to_string())));
    }
}
