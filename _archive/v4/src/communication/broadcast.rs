//! Topic-based pub/sub communication via tokio::broadcast channels.

use super::{agent_key, Communication, Message};
use crate::core::types::AgentId;
use anyhow::{anyhow, Result};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc};

/// Configuration for BroadcastCommunication.
#[derive(Debug, Clone)]
pub struct BroadcastConfig {
    /// Pre-configured topics to create on initialization.
    pub topics: Vec<String>,
    /// Broadcast channel capacity per topic.
    pub capacity_per_topic: usize,
    /// Direct message channel capacity per agent.
    pub direct_capacity: usize,
}

impl Default for BroadcastConfig {
    fn default() -> Self {
        BroadcastConfig {
            topics: vec![],
            capacity_per_topic: 256,
            direct_capacity: 128,
        }
    }
}

/// Topic-based pub/sub communication with broadcast channels.
///
/// Supports both topic-based pub/sub and direct agent-to-agent messaging.
/// Topics are created on-demand if not pre-configured.
pub struct BroadcastCommunication {
    config: BroadcastConfig,
    /// Map of topic -> broadcast sender
    topics: Arc<RwLock<HashMap<String, broadcast::Sender<Message>>>>,
    /// Map of agent_key -> direct message sender
    direct_senders: Arc<RwLock<HashMap<String, mpsc::Sender<Message>>>>,
}

impl BroadcastCommunication {
    /// Create a new BroadcastCommunication instance.
    pub fn new(config: BroadcastConfig) -> Self {
        let topics = Arc::new(RwLock::new(HashMap::new()));

        // Pre-create configured topics
        for topic in &config.topics {
            let (tx, _) = broadcast::channel(config.capacity_per_topic);
            topics.write().insert(topic.clone(), tx);
        }

        BroadcastCommunication {
            config,
            topics,
            direct_senders: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Get or create a broadcast sender for a topic.
    fn get_or_create_topic(&self, topic: &str) -> broadcast::Sender<Message> {
        {
            let topics = self.topics.read();
            if let Some(sender) = topics.get(topic) {
                return sender.clone();
            }
        }

        // Create new topic
        let mut topics = self.topics.write();

        // Double-check after acquiring write lock
        if let Some(sender) = topics.get(topic) {
            return sender.clone();
        }

        let (tx, _) = broadcast::channel(self.config.capacity_per_topic);
        topics.insert(topic.to_string(), tx.clone());
        tx
    }

    /// Register a direct message sender for an agent.
    fn register_direct(&self, agent_id: &AgentId, sender: mpsc::Sender<Message>) {
        let key = agent_key(agent_id);
        self.direct_senders.write().insert(key, sender);
    }

    /// Get the direct sender for a specific agent.
    fn get_direct_sender(&self, agent_id: &AgentId) -> Option<mpsc::Sender<Message>> {
        let key = agent_key(agent_id);
        self.direct_senders.read().get(&key).cloned()
    }

    /// Get all topic names.
    pub fn topics(&self) -> Vec<String> {
        self.topics.read().keys().cloned().collect()
    }

    /// Remove an agent's direct sender (for cleanup).
    pub fn unregister_agent(&self, agent_id: &AgentId) {
        let key = agent_key(agent_id);
        self.direct_senders.write().remove(&key);
    }

    /// Get the number of active topics.
    pub fn topic_count(&self) -> usize {
        self.topics.read().len()
    }
}

impl Communication for BroadcastCommunication {
    fn send(&self, _from: AgentId, to: AgentId, message: Message) -> Result<()> {
        let sender = self
            .get_direct_sender(&to)
            .ok_or_else(|| anyhow!("Agent not registered: {}", agent_key(&to)))?;

        sender
            .try_send(message)
            .map_err(|e| anyhow!("Failed to send direct message: {}", e))?;

        Ok(())
    }

    fn broadcast(&self, _from: AgentId, message: Message) -> Result<()> {
        let topics = self.topics.read();

        if topics.is_empty() {
            return Err(anyhow!("No topics available for broadcast"));
        }

        let mut errors = Vec::new();

        for (topic_name, sender) in topics.iter() {
            // Clone message for each topic
            if let Err(e) = sender.send(message.clone()) {
                errors.push(format!("Failed to broadcast to topic '{}': {}", topic_name, e));
            }
        }

        if !errors.is_empty() {
            return Err(anyhow!("Broadcast had {} errors: {:?}", errors.len(), errors));
        }

        Ok(())
    }

    fn subscribe(&self, agent_id: AgentId, topic: &str) -> Result<mpsc::Receiver<Message>> {
        // Get or create the broadcast channel for this topic
        let broadcast_tx = self.get_or_create_topic(topic);
        let mut broadcast_rx = broadcast_tx.subscribe();

        // Create an mpsc channel to forward messages to the agent
        let (tx, rx) = mpsc::channel(self.config.direct_capacity);

        // Spawn a task to forward messages from broadcast to mpsc
        let agent_key_str = agent_key(&agent_id);
        tokio::spawn(async move {
            while let Ok(msg) = broadcast_rx.recv().await {
                // Forward the message to the agent's mpsc channel
                if tx.send(msg).await.is_err() {
                    // Agent's receiver was dropped, stop forwarding
                    eprintln!("[BroadcastComm] Agent {} dropped receiver, stopping topic forwarding", agent_key_str);
                    break;
                }
            }
        });

        Ok(rx)
    }

    fn publish(&self, topic: &str, message: Message) -> Result<()> {
        let sender = self.get_or_create_topic(topic);

        // broadcast::send returns the number of active receivers
        // It only errors if there are no receivers, which is not an error for us
        let _ = sender.send(message);

        Ok(())
    }

    fn receiver(&self, agent_id: AgentId) -> Result<mpsc::Receiver<Message>> {
        let (tx, rx) = mpsc::channel(self.config.direct_capacity);
        self.register_direct(&agent_id, tx);
        Ok(rx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::types::{QueenId};

    #[tokio::test]
    async fn test_topic_publish_subscribe() {
        let config = BroadcastConfig {
            topics: vec!["test-topic".to_string()],
            ..Default::default()
        };
        let comm = BroadcastCommunication::new(config);

        let queen = AgentId::Queen(QueenId("Q1".to_string()));
        let mut rx = comm.subscribe(queen.clone(), "test-topic").unwrap();

        let msg = Message::with_topic(queen, "test-topic".to_string(), serde_json::json!({"data": "test"}));
        comm.publish("test-topic", msg).unwrap();

        let received = rx.recv().await.unwrap();
        assert_eq!(received.payload, serde_json::json!({"data": "test"}));
    }

    #[tokio::test]
    async fn test_auto_topic_creation() {
        let comm = BroadcastCommunication::new(BroadcastConfig::default());
        assert_eq!(comm.topic_count(), 0);

        let queen = AgentId::Queen(QueenId("Q1".to_string()));
        let msg = Message::with_topic(queen, "new-topic".to_string(), serde_json::json!({"auto": true}));

        comm.publish("new-topic", msg).unwrap();
        assert_eq!(comm.topic_count(), 1);
        assert!(comm.topics().contains(&"new-topic".to_string()));
    }

    #[tokio::test]
    async fn test_direct_and_topic_mixed() {
        let comm = BroadcastCommunication::new(BroadcastConfig::default());

        let queen1 = AgentId::Queen(QueenId("Q1".to_string()));
        let queen2 = AgentId::Queen(QueenId("Q2".to_string()));

        // Set up direct messaging
        let mut direct_rx = comm.receiver(queen2.clone()).unwrap();

        // Set up topic subscription
        let mut topic_rx = comm.subscribe(queen1.clone(), "alerts").unwrap();

        // Send direct message
        let direct_msg = Message::new(queen1.clone(), Some(queen2.clone()), serde_json::json!({"type": "direct"}));
        comm.send(queen1.clone(), queen2, direct_msg).unwrap();

        // Publish to topic
        let topic_msg = Message::with_topic(queen1, "alerts".to_string(), serde_json::json!({"type": "topic"}));
        comm.publish("alerts", topic_msg).unwrap();

        // Verify both work
        let received_direct = direct_rx.recv().await.unwrap();
        assert_eq!(received_direct.payload, serde_json::json!({"type": "direct"}));

        let received_topic = topic_rx.recv().await.unwrap();
        assert_eq!(received_topic.payload, serde_json::json!({"type": "topic"}));
    }
}
