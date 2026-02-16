//! Graph DAG topology — dependency-based task assignment using directed acyclic graphs.

use super::{agent_id_to_key, Topology};
use crate::core::types::{AgentId, TaskId, TaskResult};
use anyhow::{anyhow, Result};
use parking_lot::RwLock;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

/// Configuration for graph DAG topology.
#[derive(Debug, Clone)]
pub struct GraphDagConfig {
    /// Maximum depth of the dependency graph
    pub max_depth: usize,
}

impl Default for GraphDagConfig {
    fn default() -> Self {
        GraphDagConfig { max_depth: 10 }
    }
}

/// Node in the dependency DAG.
#[derive(Debug, Clone)]
pub struct DagNode {
    pub id: AgentId,
    pub upstream: Vec<String>,   // Agent keys this node depends on
    pub downstream: Vec<String>, // Agent keys that depend on this node
    pub completed: bool,
}

impl DagNode {
    fn new(id: AgentId) -> Self {
        DagNode {
            id,
            upstream: Vec::new(),
            downstream: Vec::new(),
            completed: false,
        }
    }

    /// Check if all upstream dependencies are completed.
    fn is_ready(&self, nodes: &HashMap<String, DagNode>) -> bool {
        self.upstream
            .iter()
            .all(|key| nodes.get(key).map_or(false, |n| n.completed))
    }

    /// Calculate bottleneck score (number of downstream dependents).
    pub fn bottleneck_score(&self) -> usize {
        self.downstream.len()
    }
}

/// Dependency graph manager using Kahn's algorithm for topological sorting.
struct DagManager {
    nodes: HashMap<String, DagNode>,
}

impl DagManager {
    fn new() -> Self {
        DagManager {
            nodes: HashMap::new(),
        }
    }

    /// Add a node to the DAG.
    fn add_node(&mut self, agent_key: String, agent_id: AgentId) {
        if !self.nodes.contains_key(&agent_key) {
            self.nodes.insert(agent_key, DagNode::new(agent_id));
        }
    }

    /// Add a dependency edge from upstream to downstream.
    fn add_edge(&mut self, upstream_key: String, downstream_key: String) -> Result<()> {
        // Validate both nodes exist
        if !self.nodes.contains_key(&upstream_key) {
            return Err(anyhow!("Upstream node not found: {}", upstream_key));
        }
        if !self.nodes.contains_key(&downstream_key) {
            return Err(anyhow!("Downstream node not found: {}", downstream_key));
        }

        // Check for cycle before adding edge
        if self.would_create_cycle(&upstream_key, &downstream_key) {
            return Err(anyhow!(
                "Adding edge {} -> {} would create a cycle",
                upstream_key,
                downstream_key
            ));
        }

        // Add edge
        if let Some(downstream_node) = self.nodes.get_mut(&downstream_key) {
            if !downstream_node.upstream.contains(&upstream_key) {
                downstream_node.upstream.push(upstream_key.clone());
            }
        }

        if let Some(upstream_node) = self.nodes.get_mut(&upstream_key) {
            if !upstream_node.downstream.contains(&downstream_key) {
                upstream_node.downstream.push(downstream_key);
            }
        }

        Ok(())
    }

    /// Check if adding an edge would create a cycle using DFS.
    fn would_create_cycle(&self, from: &str, to: &str) -> bool {
        let mut visited = HashSet::new();
        let mut stack = vec![to];

        while let Some(current) = stack.pop() {
            if current == from {
                return true; // Found cycle
            }

            if visited.contains(current) {
                continue;
            }
            visited.insert(current);

            if let Some(node) = self.nodes.get(current) {
                for downstream in &node.downstream {
                    stack.push(downstream);
                }
            }
        }

        false
    }

    /// Get ready nodes (all upstream dependencies completed) using Kahn's algorithm.
    fn get_ready_nodes(&self) -> Vec<String> {
        self.nodes
            .iter()
            .filter(|(_, node)| !node.completed && node.is_ready(&self.nodes))
            .map(|(key, _)| key.clone())
            .collect()
    }

    /// Topological sort using Kahn's algorithm.
    fn topological_sort(&self) -> Result<Vec<String>> {
        let mut in_degree: HashMap<String, usize> = HashMap::new();
        let mut result = Vec::new();

        // Calculate in-degrees
        for (key, node) in &self.nodes {
            in_degree.insert(key.clone(), node.upstream.len());
        }

        // Queue nodes with zero in-degree
        let mut queue: VecDeque<String> = in_degree
            .iter()
            .filter(|(_, &deg)| deg == 0)
            .map(|(key, _)| key.clone())
            .collect();

        // Process queue
        while let Some(node_key) = queue.pop_front() {
            result.push(node_key.clone());

            if let Some(node) = self.nodes.get(&node_key) {
                for downstream_key in &node.downstream {
                    if let Some(degree) = in_degree.get_mut(downstream_key) {
                        *degree = degree.saturating_sub(1);
                        if *degree == 0 {
                            queue.push_back(downstream_key.clone());
                        }
                    }
                }
            }
        }

        // Check for cycles
        if result.len() != self.nodes.len() {
            return Err(anyhow!("Cycle detected in dependency graph"));
        }

        Ok(result)
    }

    /// Mark a node as completed.
    fn mark_completed(&mut self, agent_key: &str) {
        if let Some(node) = self.nodes.get_mut(agent_key) {
            node.completed = true;
        }
    }

    /// Remove a node from the DAG.
    fn remove_node(&mut self, agent_key: &str) -> bool {
        if self.nodes.remove(agent_key).is_none() {
            return false;
        }

        // Remove from all upstream/downstream references
        for node in self.nodes.values_mut() {
            node.upstream.retain(|k| k != agent_key);
            node.downstream.retain(|k| k != agent_key);
        }

        true
    }

    /// Get bottleneck score for a node.
    fn bottleneck_score(&self, agent_key: &str) -> usize {
        self.nodes
            .get(agent_key)
            .map(|n| n.bottleneck_score())
            .unwrap_or(0)
    }
}

/// Graph DAG topology implementation.
///
/// Agents are organized in a directed acyclic graph where edges represent
/// dependencies. Tasks are assigned to agents whose dependencies are satisfied,
/// using topological ordering to ensure correct execution order.
pub struct GraphDagTopology {
    config: GraphDagConfig,
    /// DAG manager
    dag: Arc<RwLock<DagManager>>,
    /// Agent registry (agent_key -> AgentId)
    agent_registry: Arc<RwLock<HashMap<String, AgentId>>>,
    /// Task assignments (task_key -> agent_key)
    task_assignments: Arc<RwLock<HashMap<String, String>>>,
    /// Completed results
    results: Arc<RwLock<HashMap<String, TaskResult>>>,
}

impl GraphDagTopology {
    /// Create a new graph DAG topology with the given configuration.
    pub fn new(config: GraphDagConfig) -> Self {
        GraphDagTopology {
            config,
            dag: Arc::new(RwLock::new(DagManager::new())),
            agent_registry: Arc::new(RwLock::new(HashMap::new())),
            task_assignments: Arc::new(RwLock::new(HashMap::new())),
            results: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Add a dependency between two agents.
    pub fn add_dependency(&self, upstream: &AgentId, downstream: &AgentId) -> Result<()> {
        let upstream_key = agent_id_to_key(upstream);
        let downstream_key = agent_id_to_key(downstream);

        let mut dag = self.dag.write();
        dag.add_edge(upstream_key, downstream_key)?;

        eprintln!("[GraphDagTopology] Added dependency edge");

        Ok(())
    }

    /// Get the topological order of agents.
    pub fn topological_order(&self) -> Result<Vec<AgentId>> {
        let dag = self.dag.read();
        let sorted_keys = dag.topological_sort()?;

        let registry = self.agent_registry.read();
        let mut result = Vec::new();

        for key in sorted_keys {
            if let Some(agent_id) = registry.get(&key) {
                result.push(agent_id.clone());
            }
        }

        Ok(result)
    }

    /// Get statistics about the DAG.
    pub fn stats(&self) -> GraphDagStats {
        let dag = self.dag.read();
        let total_nodes = dag.nodes.len();
        let completed_nodes = dag.nodes.values().filter(|n| n.completed).count();

        let total_edges = dag
            .nodes
            .values()
            .map(|n| n.downstream.len())
            .sum();

        GraphDagStats {
            total_nodes,
            completed_nodes,
            total_edges,
        }
    }

    /// Get bottleneck scores for all agents.
    pub fn bottleneck_scores(&self) -> HashMap<String, usize> {
        let dag = self.dag.read();
        let mut scores = HashMap::new();

        for (key, node) in &dag.nodes {
            scores.insert(key.clone(), node.bottleneck_score());
        }

        scores
    }
}

impl Topology for GraphDagTopology {
    fn assign_task(&mut self, task_id: &TaskId) -> Result<Vec<AgentId>> {
        // Get ready nodes (dependencies satisfied)
        let ready_keys = {
            let dag = self.dag.read();
            dag.get_ready_nodes()
        };

        if ready_keys.is_empty() {
            return Err(anyhow!("No ready agents available (dependencies not satisfied)"));
        }

        // Select agent with highest bottleneck score (critical path)
        let selected_key = {
            let dag = self.dag.read();
            let mut best_key = ready_keys[0].clone();
            let mut best_score = dag.bottleneck_score(&best_key);

            for key in &ready_keys[1..] {
                let score = dag.bottleneck_score(key);
                if score > best_score {
                    best_score = score;
                    best_key = key.clone();
                }
            }

            best_key
        };

        // Get agent ID
        let agent_id = {
            let registry = self.agent_registry.read();
            registry
                .get(&selected_key)
                .cloned()
                .ok_or_else(|| anyhow!("Agent not found in registry: {}", selected_key))?
        };

        // Record assignment
        let task_key = task_id.0.clone();
        self.task_assignments
            .write()
            .insert(task_key, selected_key.clone());

        eprintln!(
            "[GraphDagTopology] Assigned task {} to agent {} (bottleneck prioritization)",
            task_id.0, selected_key
        );

        Ok(vec![agent_id])
    }

    fn handle_completion(&mut self, agent_id: AgentId, task_id: TaskId, result: TaskResult) {
        let agent_key = agent_id_to_key(&agent_id);
        eprintln!(
            "[GraphDagTopology] Agent {} completed task {} with status {:?}",
            agent_key, task_id.0, result.status
        );

        // Mark node as completed
        {
            let mut dag = self.dag.write();
            dag.mark_completed(&agent_key);
        }

        // Store result
        let task_key = task_id.0.clone();
        self.results.write().insert(task_key.clone(), result);

        // Remove assignment
        self.task_assignments.write().remove(&task_key);

        eprintln!(
            "[GraphDagTopology] Node {} marked completed, downstream nodes may now be ready",
            agent_key
        );
    }

    fn agents(&self) -> Vec<AgentId> {
        self.agent_registry
            .read()
            .values()
            .cloned()
            .collect()
    }

    fn add_agent(&mut self, agent_id: AgentId) -> Result<()> {
        let agent_key = agent_id_to_key(&agent_id);

        // Check if already registered
        if self.agent_registry.read().contains_key(&agent_key) {
            return Err(anyhow!("Agent already registered: {}", agent_key));
        }

        // Add to DAG
        {
            let mut dag = self.dag.write();
            dag.add_node(agent_key.clone(), agent_id.clone());
        }

        // Register agent
        self.agent_registry
            .write()
            .insert(agent_key.clone(), agent_id);

        eprintln!(
            "[GraphDagTopology] Added agent {} to DAG (total: {})",
            agent_key,
            self.agent_registry.read().len()
        );

        Ok(())
    }

    fn remove_agent(&mut self, agent_id: AgentId) -> Result<()> {
        let agent_key = agent_id_to_key(&agent_id);

        // Remove from DAG
        {
            let mut dag = self.dag.write();
            if !dag.remove_node(&agent_key) {
                return Err(anyhow!("Agent not found in DAG: {}", agent_key));
            }
        }

        // Remove from registry
        self.agent_registry.write().remove(&agent_key);

        eprintln!(
            "[GraphDagTopology] Removed agent {} from DAG (remaining: {})",
            agent_key,
            self.agent_registry.read().len()
        );

        Ok(())
    }
}

/// Statistics about the graph DAG topology.
#[derive(Debug, Clone)]
pub struct GraphDagStats {
    pub total_nodes: usize,
    pub completed_nodes: usize,
    pub total_edges: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::types::{QueenId, TaskStatus};
    use std::time::Duration;

    #[test]
    fn test_dag_add_edge() {
        let mut dag = DagManager::new();

        let agent1 = AgentId::Queen(QueenId("Q1".to_string()));
        let agent2 = AgentId::Queen(QueenId("Q2".to_string()));

        dag.add_node("q1".to_string(), agent1);
        dag.add_node("q2".to_string(), agent2);

        dag.add_edge("q1".to_string(), "q2".to_string()).unwrap();

        assert_eq!(dag.nodes.get("q2").unwrap().upstream.len(), 1);
        assert_eq!(dag.nodes.get("q1").unwrap().downstream.len(), 1);
    }

    #[test]
    fn test_dag_cycle_detection() {
        let mut dag = DagManager::new();

        let agent1 = AgentId::Queen(QueenId("Q1".to_string()));
        let agent2 = AgentId::Queen(QueenId("Q2".to_string()));

        dag.add_node("q1".to_string(), agent1);
        dag.add_node("q2".to_string(), agent2);

        dag.add_edge("q1".to_string(), "q2".to_string()).unwrap();

        // This should fail (cycle)
        let result = dag.add_edge("q2".to_string(), "q1".to_string());
        assert!(result.is_err());
    }

    #[test]
    fn test_topological_sort() {
        let mut dag = DagManager::new();

        let agent1 = AgentId::Queen(QueenId("Q1".to_string()));
        let agent2 = AgentId::Queen(QueenId("Q2".to_string()));
        let agent3 = AgentId::Queen(QueenId("Q3".to_string()));

        dag.add_node("q1".to_string(), agent1);
        dag.add_node("q2".to_string(), agent2);
        dag.add_node("q3".to_string(), agent3);

        dag.add_edge("q1".to_string(), "q2".to_string()).unwrap();
        dag.add_edge("q2".to_string(), "q3".to_string()).unwrap();

        let sorted = dag.topological_sort().unwrap();
        assert_eq!(sorted, vec!["q1", "q2", "q3"]);
    }

    #[test]
    fn test_graph_dag_topology() {
        let config = GraphDagConfig::default();
        let mut topology = GraphDagTopology::new(config);

        let agent1 = AgentId::Queen(QueenId("Q1".to_string()));
        let agent2 = AgentId::Queen(QueenId("Q2".to_string()));

        topology.add_agent(agent1.clone()).unwrap();
        topology.add_agent(agent2.clone()).unwrap();

        topology.add_dependency(&agent1, &agent2).unwrap();

        let task1 = TaskId("task1".to_string());
        let assigned = topology.assign_task(&task1).unwrap();
        assert_eq!(assigned.len(), 1);

        // Complete task
        let result = TaskResult {
            status: TaskStatus::Completed,
            output: "done".to_string(),
            artifacts: vec![],
            duration: Duration::from_secs(1),
            git_sha: None,
        };
        topology.handle_completion(assigned[0].clone(), task1, result);
    }
}
