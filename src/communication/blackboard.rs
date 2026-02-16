//! Blackboard communication: shared knowledge space where agents post and read entries.

use super::{agent_key, Communication, Message};
use crate::core::types::AgentId;
use anyhow::{anyhow, Result};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc;

/// Configuration for BlackboardCommunication.
#[derive(Debug, Clone)]
pub struct BlackboardCommunicationConfig {
    /// Maximum number of entries to keep (oldest evicted).
    pub max_entries: usize,
    /// Channel capacity for agent notifications.
    pub channel_capacity: usize,
}

impl Default for BlackboardCommunicationConfig {
    fn default() -> Self {
        BlackboardCommunicationConfig {
            max_entries: 1000,
            channel_capacity: 128,
        }
    }
}

/// Entry on the blackboard.
#[derive(Debug, Clone)]
pub struct BlackboardEntry {
    pub id: String,
    pub author: AgentId,
    pub content: Message,
    pub created_at: Instant,
    pub read_by: Vec<String>, // agent keys
}

impl BlackboardEntry {
    /// Create a new blackboard entry.
    pub fn new(author: AgentId, content: Message) -> Self {
        BlackboardEntry {
            id: uuid::Uuid::new_v4().to_string(),
            author,
            content,
            created_at: Instant::now(),
            read_by: Vec::new(),
        }
    }

    /// Mark an entry as read by an agent.
    pub fn mark_read(&mut self, agent_id: &AgentId) {
        let key = agent_key(agent_id);
        if !self.read_by.contains(&key) {
            self.read_by.push(key);
        }
    }

    /// Check if an agent has read this entry.
    pub fn is_read_by(&self, agent_id: &AgentId) -> bool {
        let key = agent_key(agent_id);
        self.read_by.contains(&key)
    }
}

/// Blackboard communication: shared space for posting and reading entries.
///
/// Agents can:
/// - Post entries visible to all or specific agents
/// - Read entries matching filters
/// - Subscribe to topics and get notified of new entries
pub struct BlackboardCommunication {
    config: BlackboardCommunicationConfig,
    /// All entries on the blackboard
    entries: Arc<RwLock<Vec<BlackboardEntry>>>,
    /// Notification channels per agent
    notify_channels: Arc<RwLock<HashMap<String, mpsc::Sender<Message>>>>,
}

impl BlackboardCommunication {
    /// Create a new BlackboardCommunication instance.
    pub fn new(config: BlackboardCommunicationConfig) -> Self {
        BlackboardCommunication {
            config,
            entries: Arc::new(RwLock::new(Vec::new())),
            notify_channels: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Post an entry to the blackboard.
    fn post_entry(&self, entry: BlackboardEntry) -> Result<()> {
        let mut entries = self.entries.write();

        // Evict oldest if at capacity
        if entries.len() >= self.config.max_entries {
            entries.remove(0);
        }

        entries.push(entry.clone());

        // Notify agents based on addressing
        self.notify_agents(&entry)?;

        Ok(())
    }

    /// Notify relevant agents of a new entry.
    fn notify_agents(&self, entry: &BlackboardEntry) -> Result<()> {
        let channels = self.notify_channels.read();

        // Determine who to notify
        let recipients: Vec<String> = if let Some(ref to) = entry.content.to {
            // Addressed to specific agent
            vec![agent_key(to)]
        } else {
            // Broadcast to all
            channels.keys().cloned().collect()
        };

        for recipient_key in recipients {
            if let Some(sender) = channels.get(&recipient_key) {
                // Try to send notification (non-blocking)
                let _ = sender.try_send(entry.content.clone());
            }
        }

        Ok(())
    }

    /// Get entries matching a filter.
    pub fn get_entries<F>(&self, filter: F) -> Vec<BlackboardEntry>
    where
        F: Fn(&BlackboardEntry) -> bool,
    {
        self.entries.read().iter().filter(|e| filter(e)).cloned().collect()
    }

    /// Get entries by topic.
    pub fn get_by_topic(&self, topic: &str) -> Vec<BlackboardEntry> {
        self.get_entries(|entry| {
            entry.content.topic.as_ref().map(|t| t == topic).unwrap_or(false)
        })
    }

    /// Get entries addressed to a specific agent.
    pub fn get_for_agent(&self, agent_id: &AgentId) -> Vec<BlackboardEntry> {
        let target_key = agent_key(agent_id);
        self.get_entries(|entry| {
            entry.content.to.as_ref().map(|to| agent_key(to) == target_key).unwrap_or(false)
        })
    }

    /// Get unread entries for an agent.
    pub fn get_unread(&self, agent_id: &AgentId) -> Vec<BlackboardEntry> {
        self.get_entries(|entry| !entry.is_read_by(agent_id))
    }

    /// Mark an entry as read by an agent.
    pub fn mark_read(&self, entry_id: &str, agent_id: &AgentId) -> Result<()> {
        let mut entries = self.entries.write();

        if let Some(entry) = entries.iter_mut().find(|e| e.id == entry_id) {
            entry.mark_read(agent_id);
            Ok(())
        } else {
            Err(anyhow!("Entry not found: {}", entry_id))
        }
    }

    /// Get total number of entries.
    pub fn entry_count(&self) -> usize {
        self.entries.read().len()
    }

    /// Clear all entries.
    pub fn clear(&self) {
        self.entries.write().clear();
    }

    /// Register notification channel for an agent.
    fn register_notify(&self, agent_id: &AgentId, sender: mpsc::Sender<Message>) {
        let key = agent_key(agent_id);
        self.notify_channels.write().insert(key, sender);
    }

    /// Unregister an agent's notification channel.
    pub fn unregister_agent(&self, agent_id: &AgentId) {
        let key = agent_key(agent_id);
        self.notify_channels.write().remove(&key);
    }
}

impl Communication for BlackboardCommunication {
    fn send(&self, from: AgentId, to: AgentId, message: Message) -> Result<()> {
        // Create entry addressed to specific agent
        let mut msg = message;
        msg.from = from.clone();
        msg.to = Some(to);

        let entry = BlackboardEntry::new(from, msg);
        self.post_entry(entry)
    }

    fn broadcast(&self, from: AgentId, message: Message) -> Result<()> {
        // Create entry visible to all
        let mut msg = message;
        msg.from = from.clone();
        msg.to = None;

        let entry = BlackboardEntry::new(from, msg);
        self.post_entry(entry)
    }

    fn subscribe(&self, agent_id: AgentId, topic: &str) -> Result<mpsc::Receiver<Message>> {
        // Create channel for topic-filtered notifications
        let (tx, rx) = mpsc::channel(self.config.channel_capacity);

        // Get existing entries for this topic and send them
        let entries = self.get_by_topic(topic);
        for entry in entries {
            let _ = tx.try_send(entry.content);
        }

        // Register for future notifications (will need filtering in notify_agents)
        // For now, agent will receive all notifications and filter client-side
        self.register_notify(&agent_id, tx);

        Ok(rx)
    }

    fn publish(&self, topic: &str, message: Message) -> Result<()> {
        // Create entry with topic
        let mut msg = message;
        msg.topic = Some(topic.to_string());

        let entry = BlackboardEntry::new(msg.from.clone(), msg);
        self.post_entry(entry)
    }

    fn receiver(&self, agent_id: AgentId) -> Result<mpsc::Receiver<Message>> {
        let (tx, rx) = mpsc::channel(self.config.channel_capacity);

        // Send existing entries addressed to this agent
        let entries = self.get_for_agent(&agent_id);
        for entry in entries {
            let _ = tx.try_send(entry.content);
        }

        // Register for future notifications
        self.register_notify(&agent_id, tx);

        Ok(rx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::types::{QueenId, NydusId};

    #[test]
    fn test_blackboard_entry_creation() {
        let queen = AgentId::Queen(QueenId("Q1".to_string()));
        let msg = Message::new(queen.clone(), None, serde_json::json!({"test": true}));
        let entry = BlackboardEntry::new(queen, msg);

        assert!(!entry.id.is_empty());
        assert_eq!(entry.read_by.len(), 0);
    }

    #[test]
    fn test_mark_read() {
        let queen = AgentId::Queen(QueenId("Q1".to_string()));
        let nydus = AgentId::Nydus(NydusId("N1".to_string()));
        let msg = Message::new(queen.clone(), None, serde_json::json!({"test": true}));
        let mut entry = BlackboardEntry::new(queen, msg);

        assert!(!entry.is_read_by(&nydus));
        entry.mark_read(&nydus);
        assert!(entry.is_read_by(&nydus));
    }

    #[tokio::test]
    async fn test_blackboard_post_and_read() {
        let comm = BlackboardCommunication::new(BlackboardCommunicationConfig::default());

        let queen1 = AgentId::Queen(QueenId("Q1".to_string()));
        let queen2 = AgentId::Queen(QueenId("Q2".to_string()));

        let msg = Message::new(queen1.clone(), Some(queen2.clone()), serde_json::json!({"data": "test"}));
        comm.send(queen1, queen2.clone(), msg).unwrap();

        assert_eq!(comm.entry_count(), 1);
        let entries = comm.get_for_agent(&queen2);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].content.payload, serde_json::json!({"data": "test"}));
    }

    #[tokio::test]
    async fn test_topic_filtering() {
        let comm = BlackboardCommunication::new(BlackboardCommunicationConfig::default());

        let queen = AgentId::Queen(QueenId("Q1".to_string()));

        let msg1 = Message::with_topic(queen.clone(), "topic-a".to_string(), serde_json::json!({"msg": 1}));
        let msg2 = Message::with_topic(queen.clone(), "topic-b".to_string(), serde_json::json!({"msg": 2}));

        comm.publish("topic-a", msg1).unwrap();
        comm.publish("topic-b", msg2).unwrap();

        assert_eq!(comm.entry_count(), 2);
        assert_eq!(comm.get_by_topic("topic-a").len(), 1);
        assert_eq!(comm.get_by_topic("topic-b").len(), 1);
        assert_eq!(comm.get_by_topic("topic-c").len(), 0);
    }

    #[test]
    fn test_eviction() {
        let config = BlackboardCommunicationConfig {
            max_entries: 3,
            channel_capacity: 128,
        };
        let comm = BlackboardCommunication::new(config);

        let queen = AgentId::Queen(QueenId("Q1".to_string()));

        for i in 0..5 {
            let msg = Message::new(queen.clone(), None, serde_json::json!({"seq": i}));
            comm.broadcast(queen.clone(), msg).unwrap();
        }

        // Should have evicted first 2 entries
        assert_eq!(comm.entry_count(), 3);
    }
}
