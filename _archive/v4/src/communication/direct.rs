//! Direct point-to-point communication via tokio::mpsc channels.

use super::{agent_key, Communication, Message};
use crate::core::types::AgentId;
use anyhow::{anyhow, Result};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;

/// Configuration for DirectCommunication.
#[derive(Debug, Clone)]
pub struct DirectConfig {
    /// Channel capacity per agent (default 128).
    pub channel_capacity: usize,
}

impl Default for DirectConfig {
    fn default() -> Self {
        DirectConfig {
            channel_capacity: 128,
        }
    }
}

/// Direct point-to-point messaging via mpsc channels.
///
/// Each agent gets a dedicated channel. Messages are sent directly from
/// sender to receiver without intermediaries.
pub struct DirectCommunication {
    config: DirectConfig,
    /// Map of agent_key -> sender channel
    senders: Arc<RwLock<HashMap<String, mpsc::Sender<Message>>>>,
}

impl DirectCommunication {
    /// Create a new DirectCommunication instance.
    pub fn new(config: DirectConfig) -> Self {
        DirectCommunication {
            config,
            senders: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register a new agent with a sender channel.
    fn register_agent(&self, agent_id: &AgentId, sender: mpsc::Sender<Message>) {
        let key = agent_key(agent_id);
        self.senders.write().insert(key, sender);
    }

    /// Get the sender for a specific agent.
    fn get_sender(&self, agent_id: &AgentId) -> Option<mpsc::Sender<Message>> {
        let key = agent_key(agent_id);
        self.senders.read().get(&key).cloned()
    }

    /// Get all registered agent senders.
    fn get_all_senders(&self) -> Vec<mpsc::Sender<Message>> {
        self.senders.read().values().cloned().collect()
    }

    /// Remove an agent's sender (for cleanup).
    pub fn unregister_agent(&self, agent_id: &AgentId) {
        let key = agent_key(agent_id);
        self.senders.write().remove(&key);
    }

    /// Get the number of registered agents.
    pub fn agent_count(&self) -> usize {
        self.senders.read().len()
    }
}

impl Communication for DirectCommunication {
    fn send(&self, _from: AgentId, to: AgentId, message: Message) -> Result<()> {
        let sender = self
            .get_sender(&to)
            .ok_or_else(|| anyhow!("Agent not registered: {}", agent_key(&to)))?;

        // Send asynchronously - if channel is full, this will return an error
        sender
            .try_send(message)
            .map_err(|e| anyhow!("Failed to send message: {}", e))?;

        Ok(())
    }

    fn broadcast(&self, _from: AgentId, message: Message) -> Result<()> {
        let senders = self.get_all_senders();

        if senders.is_empty() {
            return Err(anyhow!("No agents registered for broadcast"));
        }

        let mut errors = Vec::new();

        for sender in senders {
            // Clone message for each recipient
            if let Err(e) = sender.try_send(message.clone()) {
                errors.push(format!("Broadcast send failed: {}", e));
            }
        }

        if !errors.is_empty() {
            return Err(anyhow!("Broadcast had {} errors: {:?}", errors.len(), errors));
        }

        Ok(())
    }

    fn subscribe(&self, _agent_id: AgentId, _topic: &str) -> Result<mpsc::Receiver<Message>> {
        Err(anyhow!("Subscribe not supported in DirectCommunication - use BroadcastCommunication for pub/sub"))
    }

    fn publish(&self, _topic: &str, _message: Message) -> Result<()> {
        Err(anyhow!("Publish not supported in DirectCommunication - use BroadcastCommunication for pub/sub"))
    }

    fn receiver(&self, agent_id: AgentId) -> Result<mpsc::Receiver<Message>> {
        // Create a new channel for this agent
        let (tx, rx) = mpsc::channel(self.config.channel_capacity);

        // Register the sender
        self.register_agent(&agent_id, tx);

        Ok(rx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::types::{QueenId, NydusId};

    #[test]
    fn test_agent_key_generation() {
        let queen_id = AgentId::Queen(QueenId("Q0".to_string()));
        assert_eq!(agent_key(&queen_id), "queen:Q0");

        let nydus_id = AgentId::Nydus(NydusId("N1".to_string()));
        assert_eq!(agent_key(&nydus_id), "nydus:N1");
    }

    #[tokio::test]
    async fn test_direct_send_receive() {
        let comm = DirectCommunication::new(DirectConfig::default());

        let queen1 = AgentId::Queen(QueenId("Q1".to_string()));
        let queen2 = AgentId::Queen(QueenId("Q2".to_string()));

        let mut rx = comm.receiver(queen2.clone()).unwrap();

        let msg = Message::new(queen1.clone(), Some(queen2.clone()), serde_json::json!({"test": "data"}));
        comm.send(queen1, queen2, msg.clone()).unwrap();

        let received = rx.recv().await.unwrap();
        assert_eq!(received.payload, serde_json::json!({"test": "data"}));
    }

    #[tokio::test]
    async fn test_broadcast() {
        let comm = DirectCommunication::new(DirectConfig::default());

        let queen1 = AgentId::Queen(QueenId("Q1".to_string()));
        let queen2 = AgentId::Queen(QueenId("Q2".to_string()));
        let queen3 = AgentId::Queen(QueenId("Q3".to_string()));

        let mut rx2 = comm.receiver(queen2).unwrap();
        let mut rx3 = comm.receiver(queen3).unwrap();

        let msg = Message::new(queen1.clone(), None, serde_json::json!({"broadcast": "message"}));
        comm.broadcast(queen1, msg).unwrap();

        let received2 = rx2.recv().await.unwrap();
        let received3 = rx3.recv().await.unwrap();

        assert_eq!(received2.payload, serde_json::json!({"broadcast": "message"}));
        assert_eq!(received3.payload, serde_json::json!({"broadcast": "message"}));
    }
}
