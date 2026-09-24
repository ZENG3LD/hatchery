//! Conversation memory implementation.
//!
//! Tracks conversation history per agent with automatic summarization when history
//! exceeds configured thresholds.

use super::Memory;
use crate::core::types::AgentId;
use anyhow::{Context, Result};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

/// Configuration for ConversationMemory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationConfig {
    /// Number of messages per agent before triggering summarization.
    pub summarization_threshold: usize,
    /// Maximum number of messages to keep per agent (after summarization).
    pub max_history_per_agent: usize,
}

impl Default for ConversationConfig {
    fn default() -> Self {
        ConversationConfig {
            summarization_threshold: 50,
            max_history_per_agent: 100,
        }
    }
}

/// A single conversation entry.
#[derive(Debug, Clone)]
struct ConversationEntry {
    /// Role of the speaker (e.g., "user", "assistant", "system").
    role: String,
    /// Message content.
    content: String,
    /// When this message was created.
    timestamp: Instant,
    /// Whether this is a summarization entry.
    is_summary: bool,
}

/// Serializable conversation entry for snapshots.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SerializableEntry {
    role: String,
    content: String,
    timestamp_secs: u64,
    is_summary: bool,
}

/// Conversation memory with per-agent history tracking and auto-summarization.
pub struct ConversationMemory {
    config: ConversationConfig,
    conversations: Arc<RwLock<HashMap<String, Vec<ConversationEntry>>>>,
    start_time: Instant,
}

impl ConversationMemory {
    /// Create a new ConversationMemory with default configuration.
    pub fn new() -> Self {
        Self::with_config(ConversationConfig::default())
    }

    /// Create a new ConversationMemory with custom configuration.
    pub fn with_config(config: ConversationConfig) -> Self {
        ConversationMemory {
            config,
            conversations: Arc::new(RwLock::new(HashMap::new())),
            start_time: Instant::now(),
        }
    }

    /// Get the number of messages for a specific agent.
    pub fn message_count(&self, agent_key: &str) -> usize {
        self.conversations
            .read()
            .get(agent_key)
            .map(|history| history.len())
            .unwrap_or(0)
    }

    /// Get conversation history for a specific agent.
    pub fn get_history(&self, agent_key: &str) -> Vec<(String, String, bool)> {
        self.conversations
            .read()
            .get(agent_key)
            .map(|history| {
                history
                    .iter()
                    .map(|entry| (entry.role.clone(), entry.content.clone(), entry.is_summary))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Summarize old conversation entries to reduce memory usage.
    fn summarize_history(&self, agent_key: &str, history: &mut Vec<ConversationEntry>) {
        if history.len() <= self.config.summarization_threshold {
            return;
        }

        // Calculate how many messages to summarize
        let to_summarize = history.len() - (self.config.summarization_threshold / 2);

        // Extract messages to summarize
        let old_messages: Vec<_> = history.drain(0..to_summarize).collect();

        // Create summary content
        let summary_lines: Vec<String> = old_messages
            .iter()
            .map(|entry| format!("[{}] {}", entry.role, entry.content))
            .collect();

        let summary_content = format!(
            "Summary of {} earlier messages:\n{}",
            old_messages.len(),
            summary_lines.join("\n")
        );

        // Insert summary at the beginning
        history.insert(
            0,
            ConversationEntry {
                role: "system".to_string(),
                content: summary_content,
                timestamp: Instant::now(),
                is_summary: true,
            },
        );

        eprintln!(
            "[ConversationMemory] Summarized {} messages for agent: {}",
            to_summarize,
            agent_key
        );
    }

    /// Convert AgentId to string key.
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

    /// Parse key format "agent:<id>:message" to extract agent key.
    fn parse_agent_key(key: &str) -> Option<String> {
        if key.starts_with("agent:") {
            let parts: Vec<&str> = key.split(':').collect();
            if parts.len() >= 2 {
                return Some(parts[1].to_string());
            }
        }
        None
    }
}

impl Default for ConversationMemory {
    fn default() -> Self {
        Self::new()
    }
}

impl Memory for ConversationMemory {
    fn insert(&mut self, key: String, value: Value, source: AgentId) -> Result<()> {
        // Key format: "agent:<id>:message"
        let agent_key = Self::parse_agent_key(&key).unwrap_or_else(|| Self::agent_id_to_key(&source));

        // Extract role and content from value
        let (role, content) = if let Some(obj) = value.as_object() {
            let role = obj
                .get("role")
                .and_then(|v| v.as_str())
                .unwrap_or("assistant")
                .to_string();
            let content = obj
                .get("content")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            (role, content)
        } else if let Some(s) = value.as_str() {
            ("assistant".to_string(), s.to_string())
        } else {
            ("assistant".to_string(), value.to_string())
        };

        let mut conversations = self.conversations.write();
        let history = conversations.entry(agent_key.clone()).or_insert_with(Vec::new);

        // Add new entry
        history.push(ConversationEntry {
            role,
            content,
            timestamp: Instant::now(),
            is_summary: false,
        });

        // Check if summarization is needed
        if history.len() > self.config.summarization_threshold {
            self.summarize_history(&agent_key, history);
        }

        // Enforce max history limit
        if history.len() > self.config.max_history_per_agent {
            let to_remove = history.len() - self.config.max_history_per_agent;
            history.drain(0..to_remove);
        }

        Ok(())
    }

    fn get(&self, key: &str) -> Option<Value> {
        let agent_key = Self::parse_agent_key(key)?;
        let conversations = self.conversations.read();
        let history = conversations.get(&agent_key)?;

        // Return the latest message
        history.last().map(|entry| {
            serde_json::json!({
                "role": entry.role,
                "content": entry.content,
                "is_summary": entry.is_summary
            })
        })
    }

    fn query(&self, pattern: &str) -> Result<Vec<(String, Value)>> {
        let conversations = self.conversations.read();
        let mut results = Vec::new();

        for (agent_key, history) in conversations.iter() {
            for entry in history {
                // Search in content
                if entry.content.to_lowercase().contains(&pattern.to_lowercase()) {
                    let key = format!("agent:{}:message", agent_key);
                    let value = serde_json::json!({
                        "role": entry.role,
                        "content": entry.content,
                        "is_summary": entry.is_summary
                    });
                    results.push((key, value));
                }
            }
        }

        Ok(results)
    }

    fn evict(&mut self) -> Result<usize> {
        let mut conversations = self.conversations.write();
        let mut total_evicted = 0;

        for history in conversations.values_mut() {
            if history.len() > self.config.max_history_per_agent {
                let to_evict = history.len() - self.config.max_history_per_agent;
                history.drain(0..to_evict);
                total_evicted += to_evict;
            }
        }

        Ok(total_evicted)
    }

    fn snapshot(&self) -> Result<Value> {
        let conversations = self.conversations.read();
        let mut snapshot_map = HashMap::new();

        for (agent_key, history) in conversations.iter() {
            let serializable_history: Vec<SerializableEntry> = history
                .iter()
                .map(|entry| SerializableEntry {
                    role: entry.role.clone(),
                    content: entry.content.clone(),
                    timestamp_secs: entry.timestamp.duration_since(self.start_time).as_secs(),
                    is_summary: entry.is_summary,
                })
                .collect();
            snapshot_map.insert(agent_key.clone(), serializable_history);
        }

        serde_json::to_value(&snapshot_map).context("Failed to serialize conversation snapshot")
    }

    fn restore(&mut self, snapshot: Value) -> Result<()> {
        let snapshot_map: HashMap<String, Vec<SerializableEntry>> =
            serde_json::from_value(snapshot).context("Failed to deserialize conversation snapshot")?;

        let mut conversations = self.conversations.write();
        conversations.clear();

        for (agent_key, serializable_history) in snapshot_map {
            let history: Vec<ConversationEntry> = serializable_history
                .into_iter()
                .map(|entry| ConversationEntry {
                    role: entry.role,
                    content: entry.content,
                    timestamp: self.start_time + std::time::Duration::from_secs(entry.timestamp_secs),
                    is_summary: entry.is_summary,
                })
                .collect();
            conversations.insert(agent_key, history);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::types::QueenId;

    #[test]
    fn test_conversation_insert_and_get() {
        let mut mem = ConversationMemory::new();
        let agent = AgentId::Queen(QueenId("Q0".to_string()));

        let msg = serde_json::json!({
            "role": "user",
            "content": "Hello, agent!"
        });

        mem.insert("agent:Q0:message".to_string(), msg, agent)
            .unwrap();

        let result = mem.get("agent:Q0:message");
        assert!(result.is_some());
    }

    #[test]
    fn test_conversation_history() {
        let mut mem = ConversationMemory::with_config(ConversationConfig {
            summarization_threshold: 5,
            max_history_per_agent: 10,
        });
        let agent = AgentId::Queen(QueenId("Q1".to_string()));

        for i in 0..3 {
            let msg = serde_json::json!({
                "role": "user",
                "content": format!("Message {}", i)
            });
            mem.insert("agent:Q1:message".to_string(), msg, agent.clone())
                .unwrap();
        }

        let history = mem.get_history("Q1");
        assert_eq!(history.len(), 3);
    }

    #[test]
    fn test_query_content() {
        let mut mem = ConversationMemory::new();
        let agent = AgentId::Queen(QueenId("Q0".to_string()));

        mem.insert(
            "agent:Q0:message".to_string(),
            serde_json::json!({"role": "user", "content": "test search term"}),
            agent,
        )
        .unwrap();

        let results = mem.query("search").unwrap();
        assert_eq!(results.len(), 1);
    }
}
