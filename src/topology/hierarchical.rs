//! Hierarchical topology — tree structure with coordinators and leaf agents.

use super::{agent_id_to_key, Topology};
use crate::core::types::{AgentId, TaskId, TaskResult};
use anyhow::{anyhow, Result};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;

/// Configuration for hierarchical topology.
#[derive(Debug, Clone)]
pub struct HierarchicalConfig {
    /// Maximum number of agents per coordinator
    pub max_agents_per_coordinator: usize,
    /// Maximum depth of the hierarchy tree
    pub max_depth: usize,
}

impl Default for HierarchicalConfig {
    fn default() -> Self {
        HierarchicalConfig {
            max_agents_per_coordinator: 10,
            max_depth: 3,
        }
    }
}

/// A coordinator node in the hierarchy tree.
#[derive(Debug, Clone)]
struct CoordinatorNode {
    /// Coordinator's agent ID
    id: AgentId,
    /// Child coordinators (for mid-level nodes)
    children: Vec<CoordinatorNode>,
    /// Leaf agents managed by this coordinator
    agents: Vec<String>,
    /// Current load (number of active tasks)
    load: usize,
    /// Depth in the tree (0 = root)
    depth: usize,
}

impl CoordinatorNode {
    fn new(id: AgentId, depth: usize) -> Self {
        CoordinatorNode {
            id,
            children: Vec::new(),
            agents: Vec::new(),
            load: 0,
            depth,
        }
    }

    /// Total number of agents under this coordinator (including children)
    fn total_agents(&self) -> usize {
        let mut count = self.agents.len();
        for child in &self.children {
            count += child.total_agents();
        }
        count
    }

    /// Find the least-loaded leaf coordinator
    fn find_least_loaded(&self) -> Option<&CoordinatorNode> {
        if self.children.is_empty() {
            // This is a leaf coordinator
            return Some(self);
        }

        // Find least-loaded child recursively
        let mut best: Option<&CoordinatorNode> = None;
        for child in &self.children {
            if let Some(candidate) = child.find_least_loaded() {
                if best.is_none() || candidate.load < best.unwrap().load {
                    best = Some(candidate);
                }
            }
        }
        best
    }

    /// Find the least-loaded leaf coordinator (mutable)
    fn find_least_loaded_mut(&mut self) -> Option<&mut CoordinatorNode> {
        if self.children.is_empty() {
            return Some(self);
        }

        // Find least-loaded child recursively
        let mut best_index: Option<usize> = None;
        let mut best_load = usize::MAX;

        for (i, child) in self.children.iter().enumerate() {
            if let Some(candidate) = child.find_least_loaded() {
                if candidate.load < best_load {
                    best_load = candidate.load;
                    best_index = Some(i);
                }
            }
        }

        if let Some(index) = best_index {
            self.children[index].find_least_loaded_mut()
        } else {
            None
        }
    }

    /// Add an agent to this coordinator or its children
    fn add_agent(&mut self, agent_key: String, max_per_coordinator: usize, max_depth: usize) -> Result<()> {
        // If we're at max depth or have capacity, add here
        if self.depth >= max_depth - 1 || self.agents.len() < max_per_coordinator {
            if self.agents.len() >= max_per_coordinator {
                return Err(anyhow!("Coordinator at max capacity"));
            }
            self.agents.push(agent_key);
            Ok(())
        } else {
            // Try to add to least-loaded child
            if let Some(child) = self.find_least_loaded_mut() {
                if child.agents.len() < max_per_coordinator {
                    return child.add_agent(agent_key, max_per_coordinator, max_depth);
                }
            }

            // All children at capacity, add to ourselves if possible
            if self.agents.len() < max_per_coordinator {
                self.agents.push(agent_key);
                Ok(())
            } else {
                Err(anyhow!("All coordinators at max capacity"))
            }
        }
    }

    /// Remove an agent from this coordinator or its children
    fn remove_agent(&mut self, agent_key: &str) -> bool {
        // Try to remove from our agents
        if let Some(pos) = self.agents.iter().position(|k| k == agent_key) {
            self.agents.remove(pos);
            return true;
        }

        // Try children
        for child in &mut self.children {
            if child.remove_agent(agent_key) {
                return true;
            }
        }

        false
    }

    /// Collect all agent keys in this subtree
    fn collect_agent_keys(&self) -> Vec<String> {
        let mut keys = self.agents.clone();
        for child in &self.children {
            keys.extend(child.collect_agent_keys());
        }
        keys
    }
}

/// Hierarchical topology implementation.
///
/// Organizes agents in a tree structure with coordinators at each level.
/// Tasks are assigned to the least-loaded coordinator, enabling load balancing
/// across the hierarchy.
pub struct HierarchicalTopology {
    config: HierarchicalConfig,
    /// Root coordinator node
    root: Arc<RwLock<CoordinatorNode>>,
    /// Agent registry (agent_key -> AgentId)
    agent_registry: Arc<RwLock<HashMap<String, AgentId>>>,
    /// Task assignments (task_key -> agent_key)
    task_assignments: Arc<RwLock<HashMap<String, String>>>,
    /// Completed results
    results: Arc<RwLock<HashMap<String, TaskResult>>>,
}

impl HierarchicalTopology {
    /// Create a new hierarchical topology with the given configuration.
    pub fn new(config: HierarchicalConfig, root_coordinator: AgentId) -> Self {
        HierarchicalTopology {
            config,
            root: Arc::new(RwLock::new(CoordinatorNode::new(root_coordinator, 0))),
            agent_registry: Arc::new(RwLock::new(HashMap::new())),
            task_assignments: Arc::new(RwLock::new(HashMap::new())),
            results: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Get the next available agent from the least-loaded coordinator.
    fn get_next_agent(&self) -> Result<AgentId> {
        let root = self.root.read();
        let coordinator = root
            .find_least_loaded()
            .ok_or_else(|| anyhow!("No coordinators available"))?;

        if coordinator.agents.is_empty() {
            return Err(anyhow!("No agents available in least-loaded coordinator"));
        }

        // Select agent with lowest index (simple strategy)
        let agent_key = &coordinator.agents[0];
        let registry = self.agent_registry.read();
        registry
            .get(agent_key)
            .cloned()
            .ok_or_else(|| anyhow!("Agent not found in registry: {}", agent_key))
    }

    /// Increment load for the coordinator managing the given agent.
    fn increment_coordinator_load(&self, agent_key: &str) {
        let mut root = self.root.write();
        self.increment_load_recursive(&mut root, agent_key);
    }

    fn increment_load_recursive(&self, node: &mut CoordinatorNode, agent_key: &str) {
        if node.agents.contains(&agent_key.to_string()) {
            node.load += 1;
            return;
        }

        for child in &mut node.children {
            self.increment_load_recursive(child, agent_key);
        }
    }

    /// Decrement load for the coordinator managing the given agent.
    fn decrement_coordinator_load(&self, agent_key: &str) {
        let mut root = self.root.write();
        self.decrement_load_recursive(&mut root, agent_key);
    }

    fn decrement_load_recursive(&self, node: &mut CoordinatorNode, agent_key: &str) {
        if node.agents.contains(&agent_key.to_string()) {
            node.load = node.load.saturating_sub(1);
            return;
        }

        for child in &mut node.children {
            self.decrement_load_recursive(child, agent_key);
        }
    }

    /// Rebalance the hierarchy if needed.
    fn rebalance(&self) -> Result<()> {
        // Simple rebalancing: if root is overloaded, create child coordinators
        let mut root = self.root.write();

        if root.agents.len() > self.config.max_agents_per_coordinator && root.depth < self.config.max_depth - 1 {
            eprintln!("[HierarchicalTopology] Rebalancing: root overloaded with {} agents", root.agents.len());

            // Move half of agents to a new child coordinator
            let split_point = root.agents.len() / 2;
            let moved_agents = root.agents.split_off(split_point);

            // Create new child coordinator
            let child_id = AgentId::Validator; // Placeholder, should be generated
            let mut child = CoordinatorNode::new(child_id, root.depth + 1);
            child.agents = moved_agents;

            root.children.push(child);
            eprintln!("[HierarchicalTopology] Created child coordinator at depth {}", root.depth + 1);
        }

        Ok(())
    }

    /// Get statistics about the hierarchy.
    pub fn stats(&self) -> HierarchicalStats {
        let root = self.root.read();
        HierarchicalStats {
            total_agents: root.total_agents(),
            tree_depth: self.get_depth(&root),
            coordinators: self.count_coordinators(&root),
        }
    }

    fn get_depth(&self, node: &CoordinatorNode) -> usize {
        if node.children.is_empty() {
            1
        } else {
            1 + node.children.iter().map(|c| self.get_depth(c)).max().unwrap_or(0)
        }
    }

    fn count_coordinators(&self, node: &CoordinatorNode) -> usize {
        1 + node.children.iter().map(|c| self.count_coordinators(c)).sum::<usize>()
    }
}

impl Topology for HierarchicalTopology {
    fn assign_task(&mut self, task_id: &TaskId) -> Result<Vec<AgentId>> {
        // Get next agent from least-loaded coordinator
        let agent = self.get_next_agent()?;
        let agent_key = agent_id_to_key(&agent);

        // Increment coordinator load
        self.increment_coordinator_load(&agent_key);

        // Record assignment
        let task_key = task_id.0.clone();
        self.task_assignments.write().insert(task_key, agent_key.clone());

        eprintln!(
            "[HierarchicalTopology] Assigned task {} to agent {}",
            task_id.0, agent_key
        );

        // Trigger rebalance if needed
        self.rebalance()?;

        Ok(vec![agent])
    }

    fn handle_completion(&mut self, agent_id: AgentId, task_id: TaskId, result: TaskResult) {
        let agent_key = agent_id_to_key(&agent_id);
        eprintln!(
            "[HierarchicalTopology] Agent {} completed task {} with status {:?}",
            agent_key, task_id.0, result.status
        );

        // Decrement coordinator load
        self.decrement_coordinator_load(&agent_key);

        // Store result
        let task_key = task_id.0.clone();
        self.results.write().insert(task_key.clone(), result);

        // Remove assignment
        self.task_assignments.write().remove(&task_key);
    }

    fn agents(&self) -> Vec<AgentId> {
        let root = self.root.read();
        let agent_keys = root.collect_agent_keys();
        let registry = self.agent_registry.read();

        agent_keys
            .iter()
            .filter_map(|key| registry.get(key).cloned())
            .collect()
    }

    fn add_agent(&mut self, agent_id: AgentId) -> Result<()> {
        let agent_key = agent_id_to_key(&agent_id);

        // Check if already registered
        if self.agent_registry.read().contains_key(&agent_key) {
            return Err(anyhow!("Agent already registered: {}", agent_key));
        }

        // Add to hierarchy
        let mut root = self.root.write();
        root.add_agent(
            agent_key.clone(),
            self.config.max_agents_per_coordinator,
            self.config.max_depth,
        )?;

        // Register agent
        self.agent_registry.write().insert(agent_key.clone(), agent_id);

        eprintln!(
            "[HierarchicalTopology] Added agent {} (total: {})",
            agent_key,
            root.total_agents()
        );

        Ok(())
    }

    fn remove_agent(&mut self, agent_id: AgentId) -> Result<()> {
        let agent_key = agent_id_to_key(&agent_id);

        // Remove from hierarchy
        let mut root = self.root.write();
        if !root.remove_agent(&agent_key) {
            return Err(anyhow!("Agent not found: {}", agent_key));
        }

        // Remove from registry
        self.agent_registry.write().remove(&agent_key);

        eprintln!(
            "[HierarchicalTopology] Removed agent {} (remaining: {})",
            agent_key,
            root.total_agents()
        );

        Ok(())
    }
}

/// Statistics about the hierarchical topology.
#[derive(Debug, Clone)]
pub struct HierarchicalStats {
    pub total_agents: usize,
    pub tree_depth: usize,
    pub coordinators: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::types::{OverlordId, QueenId, TaskStatus};
    use std::time::Duration;

    #[test]
    fn test_hierarchical_add_agents() {
        let config = HierarchicalConfig {
            max_agents_per_coordinator: 3,
            max_depth: 2,
        };
        let root = AgentId::Overlord(OverlordId("root".to_string()));
        let mut topology = HierarchicalTopology::new(config, root);

        let agent1 = AgentId::Queen(QueenId("Q1".to_string()));
        let agent2 = AgentId::Queen(QueenId("Q2".to_string()));

        topology.add_agent(agent1).unwrap();
        topology.add_agent(agent2).unwrap();

        assert_eq!(topology.agents().len(), 2);
    }

    #[test]
    fn test_hierarchical_assignment() {
        let config = HierarchicalConfig::default();
        let root = AgentId::Overlord(OverlordId("root".to_string()));
        let mut topology = HierarchicalTopology::new(config, root);

        let agent1 = AgentId::Queen(QueenId("Q1".to_string()));
        topology.add_agent(agent1.clone()).unwrap();

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
