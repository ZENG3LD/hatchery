//! Agent-to-Agent (A2A) protocol with agent cards.

use crate::communication::{agent_key, Communication, Message};
use crate::core::types::AgentId;
use anyhow::{anyhow, Result};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;

use crate::communication::direct::{DirectCommunication, DirectConfig};

/// Configuration for A2aCommunication.
#[derive(Debug, Clone)]
pub struct A2aConfig {
    /// This agent's card.
    pub agent_card: A2aAgentCard,
    /// Direct communication config.
    pub direct_config: DirectConfig,
}

impl Default for A2aConfig {
    fn default() -> Self {
        A2aConfig {
            agent_card: A2aAgentCard::default(),
            direct_config: DirectConfig::default(),
        }
    }
}

/// Agent card describing an agent's identity and capabilities.
#[derive(Debug, Clone)]
pub struct A2aAgentCard {
    pub name: String,
    pub description: String,
    pub capabilities: Vec<String>,
    pub endpoint: String,
}

impl Default for A2aAgentCard {
    fn default() -> Self {
        A2aAgentCard {
            name: "unknown".to_string(),
            description: "No description".to_string(),
            capabilities: Vec::new(),
            endpoint: "local://unknown".to_string(),
        }
    }
}

impl A2aAgentCard {
    /// Create a new agent card.
    pub fn new(
        name: String,
        description: String,
        capabilities: Vec<String>,
        endpoint: String,
    ) -> Self {
        A2aAgentCard {
            name,
            description,
            capabilities,
            endpoint,
        }
    }

    /// Check if this agent has a specific capability.
    pub fn has_capability(&self, capability: &str) -> bool {
        self.capabilities.iter().any(|c| c == capability)
    }
}

/// Agent-to-Agent communication with agent cards.
///
/// Agents discover each other through cards and route messages based on
/// agent identity and capabilities.
pub struct A2aCommunication {
    config: A2aConfig,
    /// Underlying direct communication
    direct: DirectCommunication,
    /// Known agent cards indexed by agent_key
    agent_cards: Arc<RwLock<HashMap<String, A2aAgentCard>>>,
}

impl A2aCommunication {
    /// Create a new A2aCommunication instance.
    pub fn new(config: A2aConfig) -> Self {
        let direct = DirectCommunication::new(config.direct_config.clone());

        A2aCommunication {
            config,
            direct,
            agent_cards: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register an agent card.
    pub fn register_card(&self, agent_id: AgentId, card: A2aAgentCard) {
        let key = agent_key(&agent_id);
        self.agent_cards.write().insert(key, card);
    }

    /// Unregister an agent card.
    pub fn unregister_card(&self, agent_id: &AgentId) {
        let key = agent_key(agent_id);
        self.agent_cards.write().remove(&key);
    }

    /// Get an agent's card.
    pub fn get_card(&self, agent_id: &AgentId) -> Option<A2aAgentCard> {
        let key = agent_key(agent_id);
        self.agent_cards.read().get(&key).cloned()
    }

    /// Discover all known agents.
    pub fn discover_agents(&self) -> Vec<A2aAgentCard> {
        self.agent_cards.read().values().cloned().collect()
    }

    /// Find agents with a specific capability.
    pub fn find_by_capability(&self, capability: &str) -> Vec<A2aAgentCard> {
        self.agent_cards
            .read()
            .values()
            .filter(|card| card.has_capability(capability))
            .cloned()
            .collect()
    }

    /// Find agents by name pattern.
    pub fn find_by_name(&self, name_pattern: &str) -> Vec<A2aAgentCard> {
        self.agent_cards
            .read()
            .values()
            .filter(|card| card.name.contains(name_pattern))
            .cloned()
            .collect()
    }

    /// Get the number of registered agents.
    pub fn agent_count(&self) -> usize {
        self.agent_cards.read().len()
    }

    /// Get this agent's card.
    pub fn my_card(&self) -> &A2aAgentCard {
        &self.config.agent_card
    }
}

impl Communication for A2aCommunication {
    fn send(&self, from: AgentId, to: AgentId, message: Message) -> Result<()> {
        // Verify target agent is registered
        if self.get_card(&to).is_none() {
            return Err(anyhow!(
                "Target agent not registered: {}",
                agent_key(&to)
            ));
        }

        self.direct.send(from, to, message)
    }

    fn broadcast(&self, from: AgentId, message: Message) -> Result<()> {
        self.direct.broadcast(from, message)
    }

    fn subscribe(&self, agent_id: AgentId, topic: &str) -> Result<mpsc::Receiver<Message>> {
        self.direct.subscribe(agent_id, topic)
    }

    fn publish(&self, topic: &str, message: Message) -> Result<()> {
        self.direct.publish(topic, message)
    }

    fn receiver(&self, agent_id: AgentId) -> Result<mpsc::Receiver<Message>> {
        self.direct.receiver(agent_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::types::QueenId;

    #[test]
    fn test_agent_card() {
        let card = A2aAgentCard::new(
            "QueenA".to_string(),
            "Specialist in Rust".to_string(),
            vec!["rust".to_string(), "async".to_string()],
            "local://queen-a".to_string(),
        );

        assert!(card.has_capability("rust"));
        assert!(card.has_capability("async"));
        assert!(!card.has_capability("python"));
    }

    #[test]
    fn test_agent_discovery() {
        let config = A2aConfig::default();
        let comm = A2aCommunication::new(config);

        let queen1 = AgentId::Queen(QueenId("Q1".to_string()));
        let queen2 = AgentId::Queen(QueenId("Q2".to_string()));

        let card1 = A2aAgentCard::new(
            "Queen1".to_string(),
            "Rust expert".to_string(),
            vec!["rust".to_string()],
            "local://q1".to_string(),
        );

        let card2 = A2aAgentCard::new(
            "Queen2".to_string(),
            "Python expert".to_string(),
            vec!["python".to_string()],
            "local://q2".to_string(),
        );

        comm.register_card(queen1, card1);
        comm.register_card(queen2, card2);

        let all_agents = comm.discover_agents();
        assert_eq!(all_agents.len(), 2);

        let rust_experts = comm.find_by_capability("rust");
        assert_eq!(rust_experts.len(), 1);
        assert_eq!(rust_experts[0].name, "Queen1");

        let python_experts = comm.find_by_capability("python");
        assert_eq!(python_experts.len(), 1);
        assert_eq!(python_experts[0].name, "Queen2");
    }

    #[tokio::test]
    async fn test_a2a_messaging() {
        let config = A2aConfig::default();
        let comm = A2aCommunication::new(config);

        let queen1 = AgentId::Queen(QueenId("Q1".to_string()));
        let queen2 = AgentId::Queen(QueenId("Q2".to_string()));

        // Register cards
        let card1 = A2aAgentCard::new(
            "Q1".to_string(),
            "Test agent".to_string(),
            vec![],
            "local://q1".to_string(),
        );
        let card2 = A2aAgentCard::new(
            "Q2".to_string(),
            "Test agent".to_string(),
            vec![],
            "local://q2".to_string(),
        );

        comm.register_card(queen1.clone(), card1);
        comm.register_card(queen2.clone(), card2);

        // Set up receiver
        let mut rx = comm.receiver(queen2.clone()).unwrap();

        // Send message
        let msg = Message::new(
            queen1.clone(),
            Some(queen2.clone()),
            serde_json::json!({"test": "a2a"}),
        );
        comm.send(queen1, queen2, msg).unwrap();

        // Verify receipt
        let received = rx.recv().await.unwrap();
        assert_eq!(received.payload, serde_json::json!({"test": "a2a"}));
    }
}
