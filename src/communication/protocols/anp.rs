//! Agent Network Protocol (ANP) with network topology awareness.

use crate::communication::{agent_key, Communication, Message};
use crate::core::types::AgentId;
use anyhow::{anyhow, Result};
use parking_lot::RwLock;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use tokio::sync::mpsc;

use crate::communication::direct::{DirectCommunication, DirectConfig};

/// Configuration for AnpCommunication.
#[derive(Debug, Clone)]
pub struct AnpConfig {
    /// Network identifier.
    pub network_id: String,
    /// Direct communication config.
    pub direct_config: DirectConfig,
}

impl Default for AnpConfig {
    fn default() -> Self {
        AnpConfig {
            network_id: "default-network".to_string(),
            direct_config: DirectConfig::default(),
        }
    }
}

/// Network topology graph.
#[derive(Debug, Clone)]
struct NetworkTopology {
    /// Adjacency list: agent_key -> connected neighbors
    edges: HashMap<String, Vec<String>>,
}

impl NetworkTopology {
    fn new() -> Self {
        NetworkTopology {
            edges: HashMap::new(),
        }
    }

    /// Add a bidirectional connection between two agents.
    fn add_connection(&mut self, agent1: &str, agent2: &str) {
        self.edges
            .entry(agent1.to_string())
            .or_insert_with(Vec::new)
            .push(agent2.to_string());

        self.edges
            .entry(agent2.to_string())
            .or_insert_with(Vec::new)
            .push(agent1.to_string());
    }

    /// Remove a connection between two agents.
    fn remove_connection(&mut self, agent1: &str, agent2: &str) {
        if let Some(neighbors) = self.edges.get_mut(agent1) {
            neighbors.retain(|n| n != agent2);
        }

        if let Some(neighbors) = self.edges.get_mut(agent2) {
            neighbors.retain(|n| n != agent1);
        }
    }

    /// Find shortest path between two agents using BFS.
    fn shortest_path(&self, from: &str, to: &str) -> Option<Vec<String>> {
        if from == to {
            return Some(vec![from.to_string()]);
        }

        let mut visited: HashSet<String> = HashSet::new();
        let mut queue: VecDeque<(String, Vec<String>)> = VecDeque::new();

        queue.push_back((from.to_string(), vec![from.to_string()]));
        visited.insert(from.to_string());

        while let Some((current, path)) = queue.pop_front() {
            if let Some(neighbors) = self.edges.get(&current) {
                for neighbor in neighbors {
                    if neighbor == to {
                        let mut result = path.clone();
                        result.push(neighbor.clone());
                        return Some(result);
                    }

                    if !visited.contains(neighbor) {
                        visited.insert(neighbor.clone());
                        let mut new_path = path.clone();
                        new_path.push(neighbor.clone());
                        queue.push_back((neighbor.clone(), new_path));
                    }
                }
            }
        }

        None
    }

    /// Get all neighbors of an agent.
    fn get_neighbors(&self, agent: &str) -> Vec<String> {
        self.edges.get(agent).cloned().unwrap_or_default()
    }

    /// Get network statistics.
    fn stats(&self) -> NetworkStats {
        let node_count = self.edges.len();
        let edge_count: usize = self.edges.values().map(|v| v.len()).sum();
        let total_edges = edge_count / 2; // Bidirectional, so divide by 2

        NetworkStats {
            node_count,
            edge_count: total_edges,
        }
    }
}

/// Network statistics.
#[derive(Debug, Clone)]
pub struct NetworkStats {
    pub node_count: usize,
    pub edge_count: usize,
}

/// ANP communication with network topology awareness.
///
/// Routes messages through a network topology, finding optimal paths
/// between agents.
pub struct AnpCommunication {
    config: AnpConfig,
    /// Underlying direct communication
    direct: DirectCommunication,
    /// Network topology
    topology: Arc<RwLock<NetworkTopology>>,
}

impl AnpCommunication {
    /// Create a new AnpCommunication instance.
    pub fn new(config: AnpConfig) -> Self {
        let direct = DirectCommunication::new(config.direct_config.clone());

        AnpCommunication {
            config,
            direct,
            topology: Arc::new(RwLock::new(NetworkTopology::new())),
        }
    }

    /// Add a network connection between two agents.
    pub fn add_connection(&self, agent1: &AgentId, agent2: &AgentId) {
        let key1 = agent_key(agent1);
        let key2 = agent_key(agent2);
        self.topology.write().add_connection(&key1, &key2);
    }

    /// Remove a network connection.
    pub fn remove_connection(&self, agent1: &AgentId, agent2: &AgentId) {
        let key1 = agent_key(agent1);
        let key2 = agent_key(agent2);
        self.topology.write().remove_connection(&key1, &key2);
    }

    /// Get neighbors of an agent in the network.
    pub fn get_neighbors(&self, agent_id: &AgentId) -> Vec<String> {
        let key = agent_key(agent_id);
        self.topology.read().get_neighbors(&key)
    }

    /// Find the shortest path between two agents.
    pub fn find_path(&self, from: &AgentId, to: &AgentId) -> Option<Vec<String>> {
        let from_key = agent_key(from);
        let to_key = agent_key(to);
        self.topology.read().shortest_path(&from_key, &to_key)
    }

    /// Route a message through the network topology.
    pub fn route_message(&self, from: AgentId, to: AgentId, message: Message) -> Result<()> {
        // Find shortest path
        let path = self
            .find_path(&from, &to)
            .ok_or_else(|| anyhow!("No path between agents"))?;

        if path.len() == 1 {
            // Direct connection not needed, use standard send
            return self.direct.send(from, to, message);
        }

        // For multi-hop, send to next hop
        // In a real implementation, each intermediate node would forward
        // For now, we just send directly (simplified)
        self.direct.send(from, to, message)
    }

    /// Get network statistics.
    pub fn network_stats(&self) -> NetworkStats {
        self.topology.read().stats()
    }

    /// Get the network ID.
    pub fn network_id(&self) -> &str {
        &self.config.network_id
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
}

impl Communication for AnpCommunication {
    fn send(&self, from: AgentId, to: AgentId, message: Message) -> Result<()> {
        self.route_message(from, to, message)
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
    fn test_network_topology() {
        let mut topology = NetworkTopology::new();

        topology.add_connection("q1", "q2");
        topology.add_connection("q2", "q3");

        let neighbors = topology.get_neighbors("q2");
        assert_eq!(neighbors.len(), 2);
        assert!(neighbors.contains(&"q1".to_string()));
        assert!(neighbors.contains(&"q3".to_string()));

        let path = topology.shortest_path("q1", "q3");
        assert!(path.is_some());
        let path = path.unwrap();
        assert_eq!(path.len(), 3);
        assert_eq!(path, vec!["q1", "q2", "q3"]);
    }

    #[test]
    fn test_anp_connections() {
        let comm = AnpCommunication::new(AnpConfig::default());

        let q1 = AgentId::Queen(QueenId("Q1".to_string()));
        let q2 = AgentId::Queen(QueenId("Q2".to_string()));
        let q3 = AgentId::Queen(QueenId("Q3".to_string()));

        comm.add_connection(&q1, &q2);
        comm.add_connection(&q2, &q3);

        let stats = comm.network_stats();
        assert_eq!(stats.node_count, 3);
        assert_eq!(stats.edge_count, 2);

        let neighbors = comm.get_neighbors(&q2);
        assert_eq!(neighbors.len(), 2);

        let path = comm.find_path(&q1, &q3);
        assert!(path.is_some());
        assert_eq!(path.unwrap().len(), 3);
    }

    #[tokio::test]
    async fn test_anp_routing() {
        let comm = AnpCommunication::new(AnpConfig::default());

        let q1 = AgentId::Queen(QueenId("Q1".to_string()));
        let q2 = AgentId::Queen(QueenId("Q2".to_string()));

        comm.add_connection(&q1, &q2);

        let mut rx = comm.receiver(q2.clone()).unwrap();

        let msg = Message::new(
            q1.clone(),
            Some(q2.clone()),
            serde_json::json!({"network": "test"}),
        );

        comm.send(q1, q2, msg).unwrap();

        let received = rx.recv().await.unwrap();
        assert_eq!(received.payload, serde_json::json!({"network": "test"}));
    }
}
