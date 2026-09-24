//! Ripple effect: signal propagation through agent dependency network.

use super::{agent_key, Communication, Message};
use crate::core::types::AgentId;
use anyhow::Result;
use parking_lot::RwLock;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use tokio::sync::mpsc;

use super::direct::{DirectCommunication, DirectConfig};

/// Configuration for RippleEffectCommunication.
#[derive(Debug, Clone)]
pub struct RippleEffectConfig {
    /// Maximum propagation depth (hops).
    pub propagation_depth: usize,
    /// Signal strength attenuation factor per hop (0.0 to 1.0).
    pub attenuation_factor: f64,
    /// Minimum signal strength to continue propagation.
    pub min_signal_strength: f64,
    /// Direct communication config.
    pub direct_config: DirectConfig,
}

impl Default for RippleEffectConfig {
    fn default() -> Self {
        RippleEffectConfig {
            propagation_depth: 3,
            attenuation_factor: 0.5,
            min_signal_strength: 0.1,
            direct_config: DirectConfig::default(),
        }
    }
}

/// Type of signal being propagated.
#[derive(Debug, Clone, PartialEq)]
pub enum SignalType {
    /// Constraint changed
    Constraint,
    /// Decision made
    Decision,
    /// State changed
    StateChange,
    /// Alert/warning
    Alert,
    /// Dependency updated
    Dependency,
}

/// Signal propagating through the network.
#[derive(Debug, Clone)]
pub struct Signal {
    pub id: String,
    pub origin: AgentId,
    pub signal_type: SignalType,
    pub strength: f64, // 0.0 to 1.0
    pub hops: usize,
    pub payload: serde_json::Value,
}

impl Signal {
    /// Create a new signal.
    pub fn new(origin: AgentId, signal_type: SignalType, payload: serde_json::Value) -> Self {
        Signal {
            id: uuid::Uuid::new_v4().to_string(),
            origin,
            signal_type,
            strength: 1.0,
            hops: 0,
            payload,
        }
    }

    /// Attenuate the signal for propagation.
    pub fn attenuate(&self, factor: f64) -> Self {
        Signal {
            id: self.id.clone(),
            origin: self.origin.clone(),
            signal_type: self.signal_type.clone(),
            strength: self.strength * factor,
            hops: self.hops + 1,
            payload: self.payload.clone(),
        }
    }

    /// Check if signal is strong enough to propagate.
    pub fn is_strong_enough(&self, min_strength: f64) -> bool {
        self.strength >= min_strength
    }
}

/// Propagation graph representing agent dependencies.
#[derive(Debug, Clone)]
pub struct PropagationGraph {
    /// Adjacency list: agent -> list of dependent agents
    edges: HashMap<String, Vec<String>>,
}

impl PropagationGraph {
    /// Create an empty propagation graph.
    pub fn new() -> Self {
        PropagationGraph {
            edges: HashMap::new(),
        }
    }

    /// Add a dependency edge: from affects to.
    pub fn add_edge(&mut self, from: &AgentId, to: &AgentId) {
        let from_key = agent_key(from);
        let to_key = agent_key(to);

        self.edges
            .entry(from_key)
            .or_insert_with(Vec::new)
            .push(to_key);
    }

    /// Remove a dependency edge.
    pub fn remove_edge(&mut self, from: &AgentId, to: &AgentId) {
        let from_key = agent_key(from);
        let to_key = agent_key(to);

        if let Some(edges) = self.edges.get_mut(&from_key) {
            edges.retain(|e| e != &to_key);
        }
    }

    /// Get all agents affected by a given agent.
    pub fn get_affected(&self, agent: &AgentId) -> Vec<String> {
        let key = agent_key(agent);
        self.edges.get(&key).cloned().unwrap_or_default()
    }

    /// BFS to find all agents within max_depth hops.
    pub fn reachable_agents(&self, start: &AgentId, max_depth: usize) -> Vec<(String, usize)> {
        let start_key = agent_key(start);
        let mut visited: HashSet<String> = HashSet::new();
        let mut queue: VecDeque<(String, usize)> = VecDeque::new();
        let mut result: Vec<(String, usize)> = Vec::new();

        queue.push_back((start_key.clone(), 0));
        visited.insert(start_key);

        while let Some((current, depth)) = queue.pop_front() {
            if depth > 0 {
                // Don't include the start node
                result.push((current.clone(), depth));
            }

            if depth < max_depth {
                if let Some(neighbors) = self.edges.get(&current) {
                    for neighbor in neighbors {
                        if !visited.contains(neighbor) {
                            visited.insert(neighbor.clone());
                            queue.push_back((neighbor.clone(), depth + 1));
                        }
                    }
                }
            }
        }

        result
    }

    /// Get the number of edges in the graph.
    pub fn edge_count(&self) -> usize {
        self.edges.values().map(|v| v.len()).sum()
    }

    /// Get the number of nodes in the graph.
    pub fn node_count(&self) -> usize {
        let mut nodes: HashSet<String> = HashSet::new();
        for (from, tos) in &self.edges {
            nodes.insert(from.clone());
            for to in tos {
                nodes.insert(to.clone());
            }
        }
        nodes.len()
    }
}

/// Ripple effect communication: signal propagation through dependency network.
pub struct RippleEffectCommunication {
    config: RippleEffectConfig,
    /// Underlying direct communication
    direct: DirectCommunication,
    /// Propagation graph
    graph: Arc<RwLock<PropagationGraph>>,
    /// Signal history: agent_key -> list of signals received
    signal_history: Arc<RwLock<HashMap<String, Vec<Signal>>>>,
}

impl RippleEffectCommunication {
    /// Create a new RippleEffectCommunication instance.
    pub fn new(config: RippleEffectConfig) -> Self {
        let direct = DirectCommunication::new(config.direct_config.clone());

        RippleEffectCommunication {
            config,
            direct,
            graph: Arc::new(RwLock::new(PropagationGraph::new())),
            signal_history: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Add a dependency between agents.
    pub fn add_dependency(&self, from: &AgentId, to: &AgentId) {
        self.graph.write().add_edge(from, to);
    }

    /// Remove a dependency between agents.
    pub fn remove_dependency(&self, from: &AgentId, to: &AgentId) {
        self.graph.write().remove_edge(from, to);
    }

    /// Propagate a signal through the network.
    pub fn propagate_signal(&self, signal: Signal) -> Result<usize> {
        let mut propagated_count = 0;

        // Record signal at origin
        self.record_signal(&signal.origin, &signal);

        // Get reachable agents
        let graph = self.graph.read();
        let reachable = graph.reachable_agents(&signal.origin, self.config.propagation_depth);
        drop(graph);

        // Propagate to each reachable agent
        for (agent_key_str, hops) in reachable {
            // Calculate attenuated signal
            let attenuation = self.config.attenuation_factor.powi(hops as i32);
            let mut attenuated = signal.clone();
            attenuated.strength = signal.strength * attenuation;
            attenuated.hops = hops;

            // Only propagate if signal is strong enough
            if !attenuated.is_strong_enough(self.config.min_signal_strength) {
                continue;
            }

            // Convert agent_key back to AgentId (simplified - only works for queens)
            // In production, you'd need proper deserialization
            if let Some(agent_id) = self.parse_agent_key(&agent_key_str) {
                // Record signal
                self.record_signal(&agent_id, &attenuated);

                // Send as message
                let msg = Message::new(
                    signal.origin.clone(),
                    Some(agent_id.clone()),
                    serde_json::json!({
                        "signal_id": attenuated.id,
                        "signal_type": format!("{:?}", attenuated.signal_type),
                        "strength": attenuated.strength,
                        "hops": attenuated.hops,
                        "payload": attenuated.payload,
                    }),
                );

                // Try to send (ignore errors if agent not registered)
                if self.direct.send(signal.origin.clone(), agent_id, msg).is_ok() {
                    propagated_count += 1;
                }
            }
        }

        Ok(propagated_count)
    }

    /// Record a signal in the history.
    fn record_signal(&self, agent_id: &AgentId, signal: &Signal) {
        let key = agent_key(agent_id);
        self.signal_history
            .write()
            .entry(key)
            .or_insert_with(Vec::new)
            .push(signal.clone());
    }

    /// Get signal history for an agent.
    pub fn signal_history(&self, agent_id: &AgentId) -> Vec<Signal> {
        let key = agent_key(agent_id);
        self.signal_history
            .read()
            .get(&key)
            .cloned()
            .unwrap_or_default()
    }

    /// Clear signal history for an agent.
    pub fn clear_signal_history(&self, agent_id: &AgentId) {
        let key = agent_key(agent_id);
        self.signal_history.write().remove(&key);
    }

    /// Get propagation graph statistics.
    pub fn graph_stats(&self) -> PropagationGraphStats {
        let graph = self.graph.read();
        PropagationGraphStats {
            node_count: graph.node_count(),
            edge_count: graph.edge_count(),
        }
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

/// Propagation graph statistics.
#[derive(Debug, Clone)]
pub struct PropagationGraphStats {
    pub node_count: usize,
    pub edge_count: usize,
}

impl Communication for RippleEffectCommunication {
    fn send(&self, from: AgentId, to: AgentId, message: Message) -> Result<()> {
        // Check if message contains a signal
        if let Some(obj) = message.payload.as_object() {
            if obj.contains_key("signal_type") {
                // Extract signal information
                let signal_type = match obj.get("signal_type").and_then(|v| v.as_str()) {
                    Some("Constraint") => SignalType::Constraint,
                    Some("Decision") => SignalType::Decision,
                    Some("StateChange") => SignalType::StateChange,
                    Some("Alert") => SignalType::Alert,
                    Some("Dependency") => SignalType::Dependency,
                    _ => SignalType::StateChange,
                };

                let payload = obj.get("payload").cloned().unwrap_or(serde_json::Value::Null);
                let signal = Signal::new(from.clone(), signal_type, payload);

                // Propagate the signal
                self.propagate_signal(signal)?;
            }
        }

        // Also send via direct communication
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
    fn test_signal_attenuation() {
        let origin = AgentId::Queen(QueenId("Q1".to_string()));
        let signal = Signal::new(origin, SignalType::Constraint, serde_json::json!({"key": "value"}));

        assert_eq!(signal.strength, 1.0);
        assert_eq!(signal.hops, 0);

        let attenuated = signal.attenuate(0.5);
        assert_eq!(attenuated.strength, 0.5);
        assert_eq!(attenuated.hops, 1);
    }

    #[test]
    fn test_propagation_graph() {
        let mut graph = PropagationGraph::new();

        let q1 = AgentId::Queen(QueenId("Q1".to_string()));
        let q2 = AgentId::Queen(QueenId("Q2".to_string()));
        let q3 = AgentId::Queen(QueenId("Q3".to_string()));

        graph.add_edge(&q1, &q2);
        graph.add_edge(&q2, &q3);

        assert_eq!(graph.edge_count(), 2);

        let affected = graph.get_affected(&q1);
        assert_eq!(affected.len(), 1);
        assert!(affected[0].contains("Q2"));

        let reachable = graph.reachable_agents(&q1, 2);
        assert_eq!(reachable.len(), 2); // Q2 at depth 1, Q3 at depth 2
    }

    #[tokio::test]
    async fn test_ripple_propagation() {
        let comm = RippleEffectCommunication::new(RippleEffectConfig::default());

        let q1 = AgentId::Queen(QueenId("Q1".to_string()));
        let q2 = AgentId::Queen(QueenId("Q2".to_string()));
        let q3 = AgentId::Queen(QueenId("Q3".to_string()));

        // Set up dependency chain: Q1 -> Q2 -> Q3
        comm.add_dependency(&q1, &q2);
        comm.add_dependency(&q2, &q3);

        // Register receivers
        let _rx2 = comm.receiver(q2.clone()).unwrap();
        let _rx3 = comm.receiver(q3.clone()).unwrap();

        // Create and propagate signal
        let signal = Signal::new(q1.clone(), SignalType::Alert, serde_json::json!({"alert": "test"}));
        let count = comm.propagate_signal(signal).unwrap();

        assert_eq!(count, 2); // Propagated to Q2 and Q3

        // Check signal history
        let q2_history = comm.signal_history(&q2);
        assert_eq!(q2_history.len(), 1);
        assert_eq!(q2_history[0].hops, 1);

        let q3_history = comm.signal_history(&q3);
        assert_eq!(q3_history.len(), 1);
        assert_eq!(q3_history[0].hops, 2);
    }

    #[test]
    fn test_graph_stats() {
        let comm = RippleEffectCommunication::new(RippleEffectConfig::default());

        let q1 = AgentId::Queen(QueenId("Q1".to_string()));
        let q2 = AgentId::Queen(QueenId("Q2".to_string()));
        let q3 = AgentId::Queen(QueenId("Q3".to_string()));

        comm.add_dependency(&q1, &q2);
        comm.add_dependency(&q1, &q3);
        comm.add_dependency(&q2, &q3);

        let stats = comm.graph_stats();
        assert_eq!(stats.node_count, 3);
        assert_eq!(stats.edge_count, 3);
    }
}
