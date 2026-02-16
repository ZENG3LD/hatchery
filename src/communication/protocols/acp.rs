//! Agent Communication Protocol (ACP) with capability-based routing.

use crate::communication::{agent_key, Communication, Message};
use crate::core::types::AgentId;
use anyhow::{anyhow, Result};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;

use crate::communication::direct::{DirectCommunication, DirectConfig};

/// Configuration for AcpCommunication.
#[derive(Debug, Clone)]
pub struct AcpConfig {
    /// Capabilities this agent provides.
    pub capabilities: Vec<String>,
    /// Direct communication config.
    pub direct_config: DirectConfig,
}

impl Default for AcpConfig {
    fn default() -> Self {
        AcpConfig {
            capabilities: Vec::new(),
            direct_config: DirectConfig::default(),
        }
    }
}

/// Agent capability registry entry.
#[derive(Debug, Clone)]
struct CapabilityEntry {
    agent_id: AgentId,
    capabilities: Vec<String>,
}

/// ACP communication with capability-based routing.
///
/// Routes messages to agents based on required capabilities rather than
/// explicit addressing.
pub struct AcpCommunication {
    config: AcpConfig,
    /// Underlying direct communication
    direct: DirectCommunication,
    /// Capability registry: agent_key -> capabilities
    capabilities: Arc<RwLock<HashMap<String, Vec<String>>>>,
}

impl AcpCommunication {
    /// Create a new AcpCommunication instance.
    pub fn new(config: AcpConfig) -> Self {
        let direct = DirectCommunication::new(config.direct_config.clone());

        AcpCommunication {
            config,
            direct,
            capabilities: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register an agent's capabilities.
    pub fn register_capabilities(&self, agent_id: AgentId, capabilities: Vec<String>) {
        let key = agent_key(&agent_id);
        self.capabilities.write().insert(key, capabilities);
    }

    /// Unregister an agent's capabilities.
    pub fn unregister_capabilities(&self, agent_id: &AgentId) {
        let key = agent_key(agent_id);
        self.capabilities.write().remove(&key);
    }

    /// Get an agent's capabilities.
    pub fn get_capabilities(&self, agent_id: &AgentId) -> Vec<String> {
        let key = agent_key(agent_id);
        self.capabilities
            .read()
            .get(&key)
            .cloned()
            .unwrap_or_default()
    }

    /// Find an agent capable of handling a specific capability.
    pub fn find_capable_agent(&self, required_capability: &str) -> Option<String> {
        let caps = self.capabilities.read();

        for (agent_key, agent_caps) in caps.iter() {
            if agent_caps.contains(&required_capability.to_string()) {
                return Some(agent_key.clone());
            }
        }

        None
    }

    /// Find all agents with a specific capability.
    pub fn find_all_capable(&self, required_capability: &str) -> Vec<String> {
        let caps = self.capabilities.read();

        caps.iter()
            .filter_map(|(agent_key, agent_caps)| {
                if agent_caps.contains(&required_capability.to_string()) {
                    Some(agent_key.clone())
                } else {
                    None
                }
            })
            .collect()
    }

    /// Find an agent with all required capabilities.
    pub fn find_multi_capable(&self, required_capabilities: &[String]) -> Option<String> {
        let caps = self.capabilities.read();

        for (agent_key, agent_caps) in caps.iter() {
            let has_all = required_capabilities
                .iter()
                .all(|req| agent_caps.contains(req));

            if has_all {
                return Some(agent_key.clone());
            }
        }

        None
    }

    /// Route a message to an agent with the required capability.
    pub fn route_by_capability(
        &self,
        from: AgentId,
        required_capability: &str,
        message: Message,
    ) -> Result<()> {
        let agent_key = self
            .find_capable_agent(required_capability)
            .ok_or_else(|| {
                anyhow!("No agent with capability: {}", required_capability)
            })?;

        // Parse agent_key back to AgentId
        let to = self
            .parse_agent_key(&agent_key)
            .ok_or_else(|| anyhow!("Failed to parse agent key: {}", agent_key))?;

        self.direct.send(from, to, message)
    }

    /// Parse agent key back to AgentId (simplified implementation).
    fn parse_agent_key(&self, key: &str) -> Option<AgentId> {
        let parts: Vec<&str> = key.split(':').collect();
        if parts.len() != 2 {
            return None;
        }

        match parts[0] {
            "queen" => Some(AgentId::Queen(crate::core::types::QueenId(parts[1].to_string()))),
            "nydus" => Some(AgentId::Nydus(crate::core::types::NydusId(parts[1].to_string()))),
            "overlord" => Some(AgentId::Overlord(crate::core::types::OverlordId(parts[1].to_string()))),
            "overmind" => Some(AgentId::Overmind(crate::core::types::OvermindId(parts[1].to_string()))),
            "validator" => Some(AgentId::Validator),
            "operator" => Some(AgentId::Operator),
            _ => None,
        }
    }

    /// Get all registered capabilities across all agents.
    pub fn all_capabilities(&self) -> Vec<String> {
        let caps = self.capabilities.read();
        let mut all: Vec<String> = caps
            .values()
            .flat_map(|v| v.iter().cloned())
            .collect();

        all.sort();
        all.dedup();
        all
    }

    /// Get the number of agents registered.
    pub fn agent_count(&self) -> usize {
        self.capabilities.read().len()
    }

    /// Get this agent's capabilities.
    pub fn my_capabilities(&self) -> &[String] {
        &self.config.capabilities
    }
}

impl Communication for AcpCommunication {
    fn send(&self, from: AgentId, to: AgentId, message: Message) -> Result<()> {
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
    fn test_capability_registration() {
        let comm = AcpCommunication::new(AcpConfig::default());

        let queen1 = AgentId::Queen(QueenId("Q1".to_string()));
        let queen2 = AgentId::Queen(QueenId("Q2".to_string()));

        comm.register_capabilities(
            queen1.clone(),
            vec!["rust".to_string(), "async".to_string()],
        );

        comm.register_capabilities(
            queen2.clone(),
            vec!["python".to_string(), "ml".to_string()],
        );

        assert_eq!(comm.agent_count(), 2);

        let caps1 = comm.get_capabilities(&queen1);
        assert_eq!(caps1.len(), 2);
        assert!(caps1.contains(&"rust".to_string()));

        let all_caps = comm.all_capabilities();
        assert_eq!(all_caps.len(), 4);
    }

    #[test]
    fn test_find_capable_agent() {
        let comm = AcpCommunication::new(AcpConfig::default());

        let queen1 = AgentId::Queen(QueenId("Q1".to_string()));
        let queen2 = AgentId::Queen(QueenId("Q2".to_string()));

        comm.register_capabilities(queen1, vec!["rust".to_string()]);
        comm.register_capabilities(queen2, vec!["python".to_string()]);

        let rust_agent = comm.find_capable_agent("rust");
        assert!(rust_agent.is_some());
        assert!(rust_agent.unwrap().contains("Q1"));

        let python_agent = comm.find_capable_agent("python");
        assert!(python_agent.is_some());
        assert!(python_agent.unwrap().contains("Q2"));

        let go_agent = comm.find_capable_agent("go");
        assert!(go_agent.is_none());
    }

    #[test]
    fn test_multi_capability_matching() {
        let comm = AcpCommunication::new(AcpConfig::default());

        let queen1 = AgentId::Queen(QueenId("Q1".to_string()));
        let queen2 = AgentId::Queen(QueenId("Q2".to_string()));

        comm.register_capabilities(
            queen1,
            vec!["rust".to_string(), "async".to_string()],
        );

        comm.register_capabilities(
            queen2,
            vec!["rust".to_string()],
        );

        // Find agent with both rust and async
        let multi_agent = comm.find_multi_capable(&[
            "rust".to_string(),
            "async".to_string(),
        ]);
        assert!(multi_agent.is_some());
        assert!(multi_agent.unwrap().contains("Q1"));

        // Find agent with just rust (should find Q1 first)
        let single_agent = comm.find_multi_capable(&["rust".to_string()]);
        assert!(single_agent.is_some());
    }

    #[tokio::test]
    async fn test_capability_based_routing() {
        let comm = AcpCommunication::new(AcpConfig::default());

        let queen1 = AgentId::Queen(QueenId("Q1".to_string()));
        let queen2 = AgentId::Queen(QueenId("Q2".to_string()));

        comm.register_capabilities(queen2.clone(), vec!["code-review".to_string()]);

        let _rx = comm.receiver(queen2).unwrap();

        let msg = Message::new(
            queen1.clone(),
            None,
            serde_json::json!({"request": "review my code"}),
        );

        comm.route_by_capability(queen1, "code-review", msg)
            .unwrap();
    }
}
