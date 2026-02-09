use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;
use anyhow::Result;
use crate::core::types::*;
use super::event_log::SqliteEventLog;

/// MessageRouter — async channel-based message routing.
///
/// Each agent gets a tokio::mpsc channel. The router sends messages
/// to the correct channel and logs to the event log.
///
/// This is the async alternative to SwarmMailbox, suitable for async contexts.
pub struct MessageRouter {
    /// Channel senders indexed by agent string key
    routes: HashMap<String, mpsc::Sender<SwarmMessage>>,
    /// Event log for durability
    event_log: Arc<SqliteEventLog>,
    /// Default channel size
    channel_capacity: usize,
}

impl MessageRouter {
    /// Create a new message router.
    ///
    /// Default channel capacity: 100 messages per agent.
    pub fn new(event_log: Arc<SqliteEventLog>) -> Self {
        Self {
            routes: HashMap::new(),
            event_log,
            channel_capacity: 100,
        }
    }

    /// Create a router with custom channel capacity.
    pub fn with_capacity(event_log: Arc<SqliteEventLog>, capacity: usize) -> Self {
        Self {
            routes: HashMap::new(),
            event_log,
            channel_capacity: capacity,
        }
    }

    /// Register an agent. Returns the Receiver end for them to poll.
    ///
    /// The agent should use the returned receiver to read incoming messages.
    pub fn register(&mut self, agent_key: &str) -> mpsc::Receiver<SwarmMessage> {
        let (tx, rx) = mpsc::channel(self.channel_capacity);
        self.routes.insert(agent_key.to_string(), tx);
        rx
    }

    /// Unregister an agent (drop their sender).
    ///
    /// Any messages sent to this agent after unregistration will be logged but not delivered.
    pub fn unregister(&mut self, agent_key: &str) {
        self.routes.remove(agent_key);
    }

    /// Route a message. Log it, then send to the target channel.
    ///
    /// Returns an error if the send fails (e.g., receiver dropped).
    /// If the target is not registered, the message is logged but not delivered.
    pub async fn route(&self, msg: SwarmMessage) -> Result<()> {
        self.event_log.log(&msg);

        let target_key = agent_id_to_key(&msg.to);

        if let Some(tx) = self.routes.get(&target_key) {
            tx.send(msg).await.map_err(|e| anyhow::anyhow!("Send failed: {}", e))?;
        }
        // If target unknown, message is logged but not delivered (could escalate)

        Ok(())
    }

    /// Broadcast to all registered agents.
    ///
    /// Creates a clone of the message for each agent. Logs once, sends to all.
    pub async fn broadcast(&self, msg: SwarmMessage) -> Result<()> {
        self.event_log.log(&msg);
        for tx in self.routes.values() {
            let _ = tx.send(msg.clone()).await;
        }
        Ok(())
    }

    /// Check if an agent is registered.
    pub fn is_registered(&self, agent_key: &str) -> bool {
        self.routes.contains_key(agent_key)
    }

    /// Get the number of registered agents.
    pub fn agent_count(&self) -> usize {
        self.routes.len()
    }

    /// Get all registered agent keys.
    pub fn registered_agents(&self) -> Vec<String> {
        self.routes.keys().cloned().collect()
    }
}

/// Convert AgentId to a routing key string.
///
/// This is used to map AgentId enum variants to HashMap keys.
fn agent_id_to_key(agent: &AgentId) -> String {
    match agent {
        AgentId::Nydus(id) => format!("nydus:{}", id.0),
        AgentId::Queen(id) => format!("queen:{}", id.0),
        AgentId::Validator => "validator".to_string(),
        AgentId::Operator => "operator".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_id_to_key() {
        assert_eq!(agent_id_to_key(&AgentId::Validator), "validator");
        assert_eq!(agent_id_to_key(&AgentId::Operator), "operator");

        let nydus_id = NydusId("test".to_string());
        assert_eq!(agent_id_to_key(&AgentId::Nydus(nydus_id)), "nydus:test");

        let queen_id = QueenId("Q0".to_string());
        assert_eq!(agent_id_to_key(&AgentId::Queen(queen_id)), "queen:Q0");
    }

    #[tokio::test]
    async fn test_register_and_route() {
        let log = Arc::new(SqliteEventLog::in_memory().unwrap());
        let mut router = MessageRouter::new(log);

        let mut rx = router.register("queen:Q0");

        let msg = SwarmMessage {
            id: uuid::Uuid::new_v4().to_string(),
            from: AgentId::Nydus(NydusId::default()),
            to: AgentId::Queen(QueenId("Q0".into())),
            msg_type: MessageType::TaskAssignment,
            payload: serde_json::json!({"task": "test"}),
            timestamp: chrono::Utc::now(),
            correlation_id: None,
            visibility: Visibility::default_internal(),
        };

        router.route(msg).await.unwrap();
        let received = rx.recv().await;
        assert!(received.is_some());
    }

    #[tokio::test]
    async fn test_unregister() {
        let log = Arc::new(SqliteEventLog::in_memory().unwrap());
        let mut router = MessageRouter::new(log);
        router.register("queen:Q0");
        assert!(router.is_registered("queen:Q0"));
        router.unregister("queen:Q0");
        assert!(!router.is_registered("queen:Q0"));
    }

    #[tokio::test]
    async fn test_broadcast() {
        let log = Arc::new(SqliteEventLog::in_memory().unwrap());
        let mut router = MessageRouter::new(log);
        let mut rx0 = router.register("queen:Q0");
        let mut rx1 = router.register("queen:Q1");

        let msg = SwarmMessage {
            id: uuid::Uuid::new_v4().to_string(),
            from: AgentId::Nydus(NydusId::default()),
            to: AgentId::Operator,
            msg_type: MessageType::StatusReport,
            payload: serde_json::json!({}),
            timestamp: chrono::Utc::now(),
            correlation_id: None,
            visibility: Visibility::default_internal(),
        };

        router.broadcast(msg).await.unwrap();
        assert!(rx0.recv().await.is_some());
        assert!(rx1.recv().await.is_some());
    }

    #[tokio::test]
    async fn test_route_to_unregistered() {
        let log = Arc::new(SqliteEventLog::in_memory().unwrap());
        let router = MessageRouter::new(log.clone());

        let msg = SwarmMessage {
            id: uuid::Uuid::new_v4().to_string(),
            from: AgentId::Nydus(NydusId::default()),
            to: AgentId::Queen(QueenId("NonExistent".into())),
            msg_type: MessageType::TaskAssignment,
            payload: serde_json::json!({}),
            timestamp: chrono::Utc::now(),
            correlation_id: None,
            visibility: Visibility::default_internal(),
        };

        // Should not error even if agent is not registered (message is logged)
        assert!(router.route(msg).await.is_ok());
        assert_eq!(log.count(), 1);
    }

    #[tokio::test]
    async fn test_agent_count() {
        let log = Arc::new(SqliteEventLog::in_memory().unwrap());
        let mut router = MessageRouter::new(log);

        assert_eq!(router.agent_count(), 0);
        router.register("queen:Q0");
        assert_eq!(router.agent_count(), 1);
        router.register("queen:Q1");
        assert_eq!(router.agent_count(), 2);
        router.unregister("queen:Q0");
        assert_eq!(router.agent_count(), 1);
    }

    #[tokio::test]
    async fn test_registered_agents() {
        let log = Arc::new(SqliteEventLog::in_memory().unwrap());
        let mut router = MessageRouter::new(log);

        router.register("queen:Q0");
        router.register("queen:Q1");
        router.register("validator");

        let agents = router.registered_agents();
        assert_eq!(agents.len(), 3);
        assert!(agents.contains(&"queen:Q0".to_string()));
        assert!(agents.contains(&"queen:Q1".to_string()));
        assert!(agents.contains(&"validator".to_string()));
    }

    #[tokio::test]
    async fn test_custom_capacity() {
        let log = Arc::new(SqliteEventLog::in_memory().unwrap());
        let mut router = MessageRouter::with_capacity(log, 5);
        let _rx = router.register("queen:Q0");
        assert_eq!(router.channel_capacity, 5);
    }

    #[tokio::test]
    async fn test_multiple_messages() {
        let log = Arc::new(SqliteEventLog::in_memory().unwrap());
        let mut router = MessageRouter::new(log);
        let mut rx = router.register("queen:Q0");

        // Send multiple messages
        for i in 0..5 {
            let msg = SwarmMessage {
                id: uuid::Uuid::new_v4().to_string(),
                from: AgentId::Nydus(NydusId::default()),
                to: AgentId::Queen(QueenId("Q0".into())),
                msg_type: MessageType::TaskProgress,
                payload: serde_json::json!({"progress": i}),
                timestamp: chrono::Utc::now(),
                correlation_id: None,
                visibility: Visibility::default_internal(),
            };
            router.route(msg).await.unwrap();
        }

        // Receive all messages
        let mut count = 0;
        while rx.try_recv().is_ok() {
            count += 1;
        }
        assert_eq!(count, 5);
    }
}
