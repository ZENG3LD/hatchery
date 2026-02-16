//! Centralized message bus with routing and audit logging.

use super::{agent_key, Communication, Message};
use crate::core::types::AgentId;
use anyhow::{anyhow, Result};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc;

/// Configuration for MessageBusCommunication.
#[derive(Debug, Clone)]
pub struct MessageBusConfig {
    /// Channel capacity for each agent.
    pub capacity: usize,
    /// Enable audit logging of all messages.
    pub enable_audit: bool,
    /// Maximum audit log entries to keep.
    pub max_audit_entries: usize,
}

impl Default for MessageBusConfig {
    fn default() -> Self {
        MessageBusConfig {
            capacity: 256,
            enable_audit: true,
            max_audit_entries: 10000,
        }
    }
}

/// Audit log entry for message tracking.
#[derive(Debug, Clone)]
pub struct AuditEntry {
    pub message_id: String,
    pub from: String,
    pub to: String,
    pub topic: Option<String>,
    pub timestamp: Instant,
    pub routing_time_us: u64, // microseconds
}

impl AuditEntry {
    fn new(message: &Message, routing_time_us: u64) -> Self {
        AuditEntry {
            message_id: message.id.clone(),
            from: agent_key(&message.from),
            to: message.to.as_ref().map(agent_key).unwrap_or_else(|| "broadcast".to_string()),
            topic: message.topic.clone(),
            timestamp: Instant::now(),
            routing_time_us,
        }
    }
}

/// Centralized message bus with routing and audit logging.
///
/// All messages flow through a central router, enabling:
/// - Audit logging for debugging and analysis
/// - Message metrics and statistics
/// - Centralized policy enforcement (future)
pub struct MessageBusCommunication {
    config: MessageBusConfig,
    /// Per-agent direct message channels
    agent_channels: Arc<RwLock<HashMap<String, mpsc::Sender<Message>>>>,
    /// Topic subscriptions: topic -> list of agent keys
    topic_subscriptions: Arc<RwLock<HashMap<String, Vec<String>>>>,
    /// Audit log
    audit_log: Arc<RwLock<Vec<AuditEntry>>>,
    /// Message counter
    message_count: Arc<AtomicUsize>,
}

impl MessageBusCommunication {
    /// Create a new MessageBusCommunication instance.
    pub fn new(config: MessageBusConfig) -> Self {
        MessageBusCommunication {
            config,
            agent_channels: Arc::new(RwLock::new(HashMap::new())),
            topic_subscriptions: Arc::new(RwLock::new(HashMap::new())),
            audit_log: Arc::new(RwLock::new(Vec::new())),
            message_count: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Register an agent with the bus.
    fn register_agent(&self, agent_id: &AgentId, sender: mpsc::Sender<Message>) {
        let key = agent_key(agent_id);
        self.agent_channels.write().insert(key, sender);
    }

    /// Get sender for a specific agent.
    fn get_agent_sender(&self, agent_id: &AgentId) -> Option<mpsc::Sender<Message>> {
        let key = agent_key(agent_id);
        self.agent_channels.read().get(&key).cloned()
    }

    /// Get all registered agent senders.
    fn get_all_senders(&self) -> Vec<mpsc::Sender<Message>> {
        self.agent_channels.read().values().cloned().collect()
    }

    /// Route a message through the bus.
    fn route_message(&self, message: Message) -> Result<()> {
        let start = Instant::now();

        // Increment message counter
        self.message_count.fetch_add(1, Ordering::Relaxed);

        // Route based on addressing
        if let Some(ref to) = message.to {
            // Direct message
            let sender = self
                .get_agent_sender(to)
                .ok_or_else(|| anyhow!("Agent not registered: {}", agent_key(to)))?;

            sender
                .try_send(message.clone())
                .map_err(|e| anyhow!("Failed to route message: {}", e))?;
        } else {
            // Broadcast
            let senders = self.get_all_senders();
            for sender in senders {
                let _ = sender.try_send(message.clone());
            }
        }

        // Audit if enabled
        if self.config.enable_audit {
            let routing_time = start.elapsed().as_micros() as u64;
            self.add_audit_entry(AuditEntry::new(&message, routing_time));
        }

        Ok(())
    }

    /// Add an entry to the audit log.
    fn add_audit_entry(&self, entry: AuditEntry) {
        let mut log = self.audit_log.write();

        // Evict oldest if at capacity
        if log.len() >= self.config.max_audit_entries {
            log.remove(0);
        }

        log.push(entry);
    }

    /// Subscribe an agent to a topic.
    fn subscribe_to_topic(&self, agent_id: &AgentId, topic: &str) {
        let key = agent_key(agent_id);
        let mut subs = self.topic_subscriptions.write();

        subs.entry(topic.to_string())
            .or_insert_with(Vec::new)
            .push(key);
    }

    /// Get subscribers for a topic.
    fn get_topic_subscribers(&self, topic: &str) -> Vec<String> {
        self.topic_subscriptions
            .read()
            .get(topic)
            .cloned()
            .unwrap_or_default()
    }

    /// Publish to a topic.
    fn publish_to_topic(&self, topic: &str, message: Message) -> Result<()> {
        let subscribers = self.get_topic_subscribers(topic);

        if subscribers.is_empty() {
            return Ok(()); // No subscribers, no error
        }

        let start = Instant::now();
        self.message_count.fetch_add(1, Ordering::Relaxed);

        let channels = self.agent_channels.read();

        for sub_key in subscribers {
            if let Some(sender) = channels.get(&sub_key) {
                let _ = sender.try_send(message.clone());
            }
        }

        if self.config.enable_audit {
            let routing_time = start.elapsed().as_micros() as u64;
            self.add_audit_entry(AuditEntry::new(&message, routing_time));
        }

        Ok(())
    }

    /// Get a copy of the audit log.
    pub fn audit_log(&self) -> Vec<AuditEntry> {
        self.audit_log.read().clone()
    }

    /// Get total number of messages routed.
    pub fn message_count(&self) -> usize {
        self.message_count.load(Ordering::Relaxed)
    }

    /// Get audit statistics.
    pub fn audit_stats(&self) -> AuditStats {
        let log = self.audit_log.read();

        let total_messages = log.len();
        let avg_routing_time = if total_messages > 0 {
            log.iter().map(|e| e.routing_time_us).sum::<u64>() / total_messages as u64
        } else {
            0
        };

        let max_routing_time = log.iter().map(|e| e.routing_time_us).max().unwrap_or(0);

        AuditStats {
            total_messages,
            avg_routing_time_us: avg_routing_time,
            max_routing_time_us: max_routing_time,
        }
    }

    /// Clear the audit log.
    pub fn clear_audit_log(&self) {
        self.audit_log.write().clear();
    }

    /// Unregister an agent.
    pub fn unregister_agent(&self, agent_id: &AgentId) {
        let key = agent_key(agent_id);
        self.agent_channels.write().remove(&key);

        // Remove from topic subscriptions
        let mut subs = self.topic_subscriptions.write();
        for subscribers in subs.values_mut() {
            subscribers.retain(|k| k != &key);
        }
    }

    /// Get number of registered agents.
    pub fn agent_count(&self) -> usize {
        self.agent_channels.read().len()
    }

    /// Get number of topics with subscribers.
    pub fn topic_count(&self) -> usize {
        self.topic_subscriptions.read().len()
    }
}

/// Audit statistics.
#[derive(Debug, Clone)]
pub struct AuditStats {
    pub total_messages: usize,
    pub avg_routing_time_us: u64,
    pub max_routing_time_us: u64,
}

impl Communication for MessageBusCommunication {
    fn send(&self, from: AgentId, to: AgentId, message: Message) -> Result<()> {
        let mut msg = message;
        msg.from = from;
        msg.to = Some(to);
        self.route_message(msg)
    }

    fn broadcast(&self, from: AgentId, message: Message) -> Result<()> {
        let mut msg = message;
        msg.from = from;
        msg.to = None;
        self.route_message(msg)
    }

    fn subscribe(&self, agent_id: AgentId, topic: &str) -> Result<mpsc::Receiver<Message>> {
        // Subscribe to topic
        self.subscribe_to_topic(&agent_id, topic);

        // Create receiver for this agent (if not already registered)
        let key = agent_key(&agent_id);
        if self.agent_channels.read().get(&key).is_none() {
            self.receiver(agent_id)
        } else {
            Err(anyhow!("Agent already has a receiver - cannot create duplicate"))
        }
    }

    fn publish(&self, topic: &str, message: Message) -> Result<()> {
        let mut msg = message;
        msg.topic = Some(topic.to_string());
        self.publish_to_topic(topic, msg)
    }

    fn receiver(&self, agent_id: AgentId) -> Result<mpsc::Receiver<Message>> {
        let (tx, rx) = mpsc::channel(self.config.capacity);
        self.register_agent(&agent_id, tx);
        Ok(rx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::types::{QueenId, NydusId};

    #[tokio::test]
    async fn test_message_bus_routing() {
        let comm = MessageBusCommunication::new(MessageBusConfig::default());

        let queen1 = AgentId::Queen(QueenId("Q1".to_string()));
        let queen2 = AgentId::Queen(QueenId("Q2".to_string()));

        let mut rx = comm.receiver(queen2.clone()).unwrap();

        let msg = Message::new(queen1.clone(), Some(queen2.clone()), serde_json::json!({"data": "test"}));
        comm.send(queen1, queen2, msg).unwrap();

        let received = rx.recv().await.unwrap();
        assert_eq!(received.payload, serde_json::json!({"data": "test"}));

        assert_eq!(comm.message_count(), 1);
    }

    #[tokio::test]
    async fn test_audit_logging() {
        let config = MessageBusConfig {
            enable_audit: true,
            ..Default::default()
        };
        let comm = MessageBusCommunication::new(config);

        let queen1 = AgentId::Queen(QueenId("Q1".to_string()));
        let queen2 = AgentId::Queen(QueenId("Q2".to_string()));

        let _rx = comm.receiver(queen2.clone()).unwrap();

        let msg = Message::new(queen1.clone(), Some(queen2.clone()), serde_json::json!({"test": 1}));
        comm.send(queen1, queen2, msg).unwrap();

        let log = comm.audit_log();
        assert_eq!(log.len(), 1);
        assert!(log[0].from.contains("queen:Q1"));
        assert!(log[0].to.contains("queen:Q2"));
    }

    #[tokio::test]
    async fn test_topic_pub_sub() {
        let comm = MessageBusCommunication::new(MessageBusConfig::default());

        let queen1 = AgentId::Queen(QueenId("Q1".to_string()));
        let queen2 = AgentId::Queen(QueenId("Q2".to_string()));

        let mut rx = comm.subscribe(queen2.clone(), "alerts").unwrap();

        let msg = Message::with_topic(queen1, "alerts".to_string(), serde_json::json!({"alert": true}));
        comm.publish("alerts", msg).unwrap();

        let received = rx.recv().await.unwrap();
        assert_eq!(received.payload, serde_json::json!({"alert": true}));
    }

    #[test]
    fn test_audit_stats() {
        let comm = MessageBusCommunication::new(MessageBusConfig::default());

        let queen1 = AgentId::Queen(QueenId("Q1".to_string()));
        let queen2 = AgentId::Queen(QueenId("Q2".to_string()));

        let _rx = comm.receiver(queen2.clone()).unwrap();

        for i in 0..10 {
            let msg = Message::new(queen1.clone(), Some(queen2.clone()), serde_json::json!({"seq": i}));
            let _ = comm.send(queen1.clone(), queen2.clone(), msg);
        }

        let stats = comm.audit_stats();
        assert_eq!(stats.total_messages, 10);
        assert!(stats.avg_routing_time_us > 0);
    }
}
