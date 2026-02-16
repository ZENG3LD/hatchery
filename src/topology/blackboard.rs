//! Blackboard topology — shared semantic space where agents self-select tasks.

use super::{agent_id_to_key, Topology};
use crate::core::types::{AgentId, TaskId, TaskResult};
use anyhow::{anyhow, Result};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Configuration for blackboard topology.
#[derive(Debug, Clone)]
pub struct BlackboardConfig {
    /// Maximum number of pending task requests on the blackboard
    pub max_requests: usize,
    /// Timeout for stale blackboard entries
    pub cycle_timeout: Duration,
}

impl Default for BlackboardConfig {
    fn default() -> Self {
        BlackboardConfig {
            max_requests: 1000,
            cycle_timeout: Duration::from_secs(300), // 5 minutes
        }
    }
}

/// Entry on the blackboard representing a task request.
#[derive(Debug, Clone)]
pub struct BlackboardEntry {
    pub task_id: TaskId,
    pub posted_by: AgentId,
    pub claimed_by: Option<String>,
    pub posted_at: Instant,
}

/// Shared blackboard for task posting and claiming.
struct Blackboard {
    entries: Vec<BlackboardEntry>,
    max_requests: usize,
}

impl Blackboard {
    fn new(max_requests: usize) -> Self {
        Blackboard {
            entries: Vec::new(),
            max_requests,
        }
    }

    /// Post a new task to the blackboard.
    fn post(&mut self, task_id: TaskId, posted_by: AgentId) -> Result<()> {
        if self.entries.len() >= self.max_requests {
            return Err(anyhow!(
                "Blackboard at max capacity: {}",
                self.max_requests
            ));
        }

        self.entries.push(BlackboardEntry {
            task_id,
            posted_by,
            claimed_by: None,
            posted_at: Instant::now(),
        });

        Ok(())
    }

    /// Claim a task from the blackboard.
    fn claim(&mut self, task_id: &TaskId, agent_key: String) -> Result<()> {
        let entry = self
            .entries
            .iter_mut()
            .find(|e| &e.task_id == task_id)
            .ok_or_else(|| anyhow!("Task not found on blackboard: {}", task_id.0))?;

        if entry.claimed_by.is_some() {
            return Err(anyhow!("Task already claimed: {}", task_id.0));
        }

        entry.claimed_by = Some(agent_key);
        Ok(())
    }

    /// Remove a task from the blackboard.
    fn remove(&mut self, task_id: &TaskId) -> Result<BlackboardEntry> {
        let pos = self
            .entries
            .iter()
            .position(|e| &e.task_id == task_id)
            .ok_or_else(|| anyhow!("Task not found on blackboard: {}", task_id.0))?;

        Ok(self.entries.remove(pos))
    }

    /// Get all unclaimed entries.
    fn unclaimed(&self) -> Vec<&BlackboardEntry> {
        self.entries
            .iter()
            .filter(|e| e.claimed_by.is_none())
            .collect()
    }

    /// Remove stale entries older than the timeout.
    fn prune_stale(&mut self, timeout: Duration) -> usize {
        let now = Instant::now();
        let before = self.entries.len();

        self.entries
            .retain(|e| now.duration_since(e.posted_at) < timeout);

        before - self.entries.len()
    }
}

/// Control unit that selects best agent for unclaimed tasks.
struct ControlUnit {
    agent_loads: HashMap<String, usize>,
}

impl ControlUnit {
    fn new() -> Self {
        ControlUnit {
            agent_loads: HashMap::new(),
        }
    }

    /// Select the best agent for a task based on current load.
    fn select_agent(&self, available_agents: &[String]) -> Option<String> {
        if available_agents.is_empty() {
            return None;
        }

        // Find agent with lowest load
        let mut best_agent: Option<String> = None;
        let mut best_load = usize::MAX;

        for agent_key in available_agents {
            let load = self.agent_loads.get(agent_key).copied().unwrap_or(0);
            if load < best_load {
                best_load = load;
                best_agent = Some(agent_key.clone());
            }
        }

        best_agent
    }

    /// Update agent load.
    fn update_load(&mut self, agent_key: &str, delta: i32) {
        let entry = self.agent_loads.entry(agent_key.to_string()).or_insert(0);
        if delta < 0 {
            *entry = entry.saturating_sub(delta.abs() as usize);
        } else {
            *entry += delta as usize;
        }
    }

    /// Remove agent from tracking.
    fn remove_agent(&mut self, agent_key: &str) {
        self.agent_loads.remove(agent_key);
    }
}

/// Blackboard topology implementation.
///
/// Agents post tasks to a shared blackboard and other agents self-select
/// tasks based on their capabilities and availability. A control unit
/// helps coordinate by suggesting optimal assignments based on agent load.
pub struct BlackboardTopology {
    config: BlackboardConfig,
    /// Shared blackboard
    blackboard: Arc<RwLock<Blackboard>>,
    /// Control unit for agent selection
    control_unit: Arc<RwLock<ControlUnit>>,
    /// Agent registry (agent_key -> AgentId)
    agent_registry: Arc<RwLock<HashMap<String, AgentId>>>,
    /// Completed results
    results: Arc<RwLock<HashMap<String, TaskResult>>>,
}

impl BlackboardTopology {
    /// Create a new blackboard topology with the given configuration.
    pub fn new(config: BlackboardConfig) -> Self {
        let blackboard = Blackboard::new(config.max_requests);

        BlackboardTopology {
            config,
            blackboard: Arc::new(RwLock::new(blackboard)),
            control_unit: Arc::new(RwLock::new(ControlUnit::new())),
            agent_registry: Arc::new(RwLock::new(HashMap::new())),
            results: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Cleanup stale entries from the blackboard.
    fn cleanup_stale_entries(&self) {
        let mut blackboard = self.blackboard.write();
        let removed = blackboard.prune_stale(self.config.cycle_timeout);

        if removed > 0 {
            eprintln!(
                "[BlackboardTopology] Pruned {} stale entries from blackboard",
                removed
            );
        }
    }

    /// Get statistics about the blackboard.
    pub fn stats(&self) -> BlackboardStats {
        let blackboard = self.blackboard.read();
        let total_entries = blackboard.entries.len();
        let unclaimed = blackboard.unclaimed().len();
        let claimed = total_entries - unclaimed;

        BlackboardStats {
            total_entries,
            unclaimed_entries: unclaimed,
            claimed_entries: claimed,
            registered_agents: self.agent_registry.read().len(),
        }
    }

    /// Get the current blackboard entries (for debugging/monitoring).
    pub fn entries(&self) -> Vec<BlackboardEntry> {
        self.blackboard.read().entries.clone()
    }
}

impl Topology for BlackboardTopology {
    fn assign_task(&mut self, task_id: &TaskId) -> Result<Vec<AgentId>> {
        // Cleanup stale entries first
        self.cleanup_stale_entries();

        // Post task to blackboard
        let poster = AgentId::Validator; // Placeholder for task originator
        {
            let mut blackboard = self.blackboard.write();
            blackboard.post(task_id.clone(), poster)?;
        }

        eprintln!(
            "[BlackboardTopology] Posted task {} to blackboard",
            task_id.0
        );

        // Control unit selects best agent
        let agent_keys: Vec<_> = self.agent_registry.read().keys().cloned().collect();
        let selected_key = {
            let control_unit = self.control_unit.read();
            control_unit
                .select_agent(&agent_keys)
                .ok_or_else(|| anyhow!("No agents available for task assignment"))?
        };

        // Claim task on behalf of selected agent
        {
            let mut blackboard = self.blackboard.write();
            blackboard.claim(task_id, selected_key.clone())?;
        }

        // Update control unit load
        {
            let mut control_unit = self.control_unit.write();
            control_unit.update_load(&selected_key, 1);
        }

        // Get agent ID
        let agent_id = {
            let registry = self.agent_registry.read();
            registry
                .get(&selected_key)
                .cloned()
                .ok_or_else(|| anyhow!("Agent not found in registry: {}", selected_key))?
        };

        eprintln!(
            "[BlackboardTopology] Assigned task {} to agent {} via control unit",
            task_id.0, selected_key
        );

        Ok(vec![agent_id])
    }

    fn handle_completion(&mut self, agent_id: AgentId, task_id: TaskId, result: TaskResult) {
        let agent_key = agent_id_to_key(&agent_id);
        eprintln!(
            "[BlackboardTopology] Agent {} completed task {} with status {:?}",
            agent_key, task_id.0, result.status
        );

        // Remove from blackboard
        {
            let mut blackboard = self.blackboard.write();
            if let Err(e) = blackboard.remove(&task_id) {
                eprintln!(
                    "[BlackboardTopology] Failed to remove task from blackboard: {}",
                    e
                );
            }
        }

        // Update control unit load
        {
            let mut control_unit = self.control_unit.write();
            control_unit.update_load(&agent_key, -1);
        }

        // Store result
        let task_key = task_id.0.clone();
        self.results.write().insert(task_key, result);
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
        let mut registry = self.agent_registry.write();

        if registry.contains_key(&agent_key) {
            return Err(anyhow!("Agent already registered: {}", agent_key));
        }

        registry.insert(agent_key.clone(), agent_id);

        // Initialize in control unit
        {
            let mut control_unit = self.control_unit.write();
            control_unit.update_load(&agent_key, 0);
        }

        eprintln!(
            "[BlackboardTopology] Added agent {} (total: {})",
            agent_key,
            registry.len()
        );

        Ok(())
    }

    fn remove_agent(&mut self, agent_id: AgentId) -> Result<()> {
        let agent_key = agent_id_to_key(&agent_id);
        let mut registry = self.agent_registry.write();

        if registry.remove(&agent_key).is_none() {
            return Err(anyhow!("Agent not found: {}", agent_key));
        }

        // Remove from control unit
        {
            let mut control_unit = self.control_unit.write();
            control_unit.remove_agent(&agent_key);
        }

        eprintln!(
            "[BlackboardTopology] Removed agent {} (remaining: {})",
            agent_key,
            registry.len()
        );

        Ok(())
    }
}

/// Statistics about the blackboard topology.
#[derive(Debug, Clone)]
pub struct BlackboardStats {
    pub total_entries: usize,
    pub unclaimed_entries: usize,
    pub claimed_entries: usize,
    pub registered_agents: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::types::{QueenId, TaskStatus};

    #[test]
    fn test_blackboard_post_claim() {
        let mut blackboard = Blackboard::new(100);

        let task = TaskId("task1".to_string());
        let poster = AgentId::Validator;

        blackboard.post(task.clone(), poster).unwrap();
        assert_eq!(blackboard.entries.len(), 1);

        blackboard.claim(&task, "agent1".to_string()).unwrap();
        assert_eq!(blackboard.entries[0].claimed_by, Some("agent1".to_string()));
    }

    #[test]
    fn test_blackboard_topology() {
        let config = BlackboardConfig::default();
        let mut topology = BlackboardTopology::new(config);

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

    #[test]
    fn test_control_unit_selection() {
        let mut control_unit = ControlUnit::new();
        control_unit.update_load("agent1", 5);
        control_unit.update_load("agent2", 2);

        let agents = vec!["agent1".to_string(), "agent2".to_string()];
        let selected = control_unit.select_agent(&agents);

        assert_eq!(selected, Some("agent2".to_string())); // Lower load
    }
}
