//! Centralized topology — single coordinator assigns tasks round-robin to registered agents.

use super::{agent_id_to_key, Topology};
use crate::core::types::{AgentId, TaskId, TaskResult};
use anyhow::{anyhow, Result};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;

/// Configuration for centralized topology.
#[derive(Debug, Clone)]
pub struct CentralizedConfig {
    /// Maximum number of agents allowed in the pool
    pub max_agents: usize,
}

impl Default for CentralizedConfig {
    fn default() -> Self {
        CentralizedConfig { max_agents: 100 }
    }
}

/// Agent status in the centralized pool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AgentStatus {
    Idle,
    Busy,
}

/// Internal agent registry entry.
#[derive(Debug, Clone)]
struct AgentEntry {
    id: AgentId,
    status: AgentStatus,
    completed_tasks: usize,
}

/// Centralized topology implementation.
///
/// A single coordinator maintains a registry of agents and assigns tasks round-robin
/// to idle agents. This is the simplest topology, suitable for homogeneous workloads
/// where all agents are equally capable.
pub struct CentralizedTopology {
    config: CentralizedConfig,
    /// Agent registry indexed by string key
    agents: Arc<RwLock<HashMap<String, AgentEntry>>>,
    /// Round-robin index for task assignment
    round_robin_index: Arc<RwLock<usize>>,
    /// Task assignments (task_id -> agent_key)
    task_assignments: Arc<RwLock<HashMap<String, String>>>,
    /// Completed task results
    results: Arc<RwLock<HashMap<String, TaskResult>>>,
}

impl CentralizedTopology {
    /// Create a new centralized topology with the given configuration.
    pub fn new(config: CentralizedConfig) -> Self {
        CentralizedTopology {
            config,
            agents: Arc::new(RwLock::new(HashMap::new())),
            round_robin_index: Arc::new(RwLock::new(0)),
            task_assignments: Arc::new(RwLock::new(HashMap::new())),
            results: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Get the next idle agent via round-robin selection.
    fn get_next_idle_agent(&self) -> Result<AgentId> {
        let agents = self.agents.read();
        let agent_list: Vec<_> = agents.values().collect();

        if agent_list.is_empty() {
            return Err(anyhow!("No agents registered in centralized topology"));
        }

        // Find idle agents
        let idle_agents: Vec<_> = agent_list
            .iter()
            .filter(|entry| entry.status == AgentStatus::Idle)
            .collect();

        if idle_agents.is_empty() {
            return Err(anyhow!("No idle agents available"));
        }

        // Round-robin selection among idle agents
        let mut index = self.round_robin_index.write();
        let selected_index = *index % idle_agents.len();
        *index = (*index + 1) % idle_agents.len();

        Ok(idle_agents[selected_index].id.clone())
    }

    /// Mark an agent as busy with a task.
    fn mark_agent_busy(&self, agent_id: &AgentId) -> Result<()> {
        let agent_key = agent_id_to_key(agent_id);
        let mut agents = self.agents.write();

        let entry = agents
            .get_mut(&agent_key)
            .ok_or_else(|| anyhow!("Agent not found: {}", agent_key))?;

        entry.status = AgentStatus::Busy;
        Ok(())
    }

    /// Mark an agent as idle.
    fn mark_agent_idle(&self, agent_id: &AgentId) -> Result<()> {
        let agent_key = agent_id_to_key(agent_id);
        let mut agents = self.agents.write();

        let entry = agents
            .get_mut(&agent_key)
            .ok_or_else(|| anyhow!("Agent not found: {}", agent_key))?;

        entry.status = AgentStatus::Idle;
        entry.completed_tasks += 1;
        Ok(())
    }

    /// Get statistics about the agent pool.
    pub fn stats(&self) -> CentralizedStats {
        let agents = self.agents.read();
        let idle_count = agents
            .values()
            .filter(|e| e.status == AgentStatus::Idle)
            .count();
        let busy_count = agents
            .values()
            .filter(|e| e.status == AgentStatus::Busy)
            .count();
        let total_completed = agents.values().map(|e| e.completed_tasks).sum();

        CentralizedStats {
            total_agents: agents.len(),
            idle_agents: idle_count,
            busy_agents: busy_count,
            total_completed_tasks: total_completed,
        }
    }
}

impl Topology for CentralizedTopology {
    fn assign_task(&mut self, task_id: &TaskId) -> Result<Vec<AgentId>> {
        // Get next idle agent via round-robin
        let agent = self.get_next_idle_agent()?;

        // Mark agent as busy
        self.mark_agent_busy(&agent)?;

        // Record assignment
        let task_key = task_id.0.clone();
        let agent_key = agent_id_to_key(&agent);
        self.task_assignments
            .write()
            .insert(task_key, agent_key.clone());

        eprintln!(
            "[CentralizedTopology] Assigned task {} to agent {}",
            task_id.0, agent_key
        );

        Ok(vec![agent])
    }

    fn handle_completion(&mut self, agent_id: AgentId, task_id: TaskId, result: TaskResult) {
        eprintln!(
            "[CentralizedTopology] Agent {} completed task {} with status {:?}",
            agent_id_to_key(&agent_id),
            task_id.0,
            result.status
        );

        // Mark agent as idle
        if let Err(e) = self.mark_agent_idle(&agent_id) {
            eprintln!(
                "[CentralizedTopology] Failed to mark agent idle: {}",
                e
            );
        }

        // Store result
        let task_key = task_id.0.clone();
        self.results.write().insert(task_key.clone(), result);

        // Remove assignment
        self.task_assignments.write().remove(&task_key);
    }

    fn agents(&self) -> Vec<AgentId> {
        self.agents
            .read()
            .values()
            .map(|entry| entry.id.clone())
            .collect()
    }

    fn add_agent(&mut self, agent_id: AgentId) -> Result<()> {
        let agent_key = agent_id_to_key(&agent_id);
        let mut agents = self.agents.write();

        if agents.len() >= self.config.max_agents {
            return Err(anyhow!(
                "Maximum agent limit reached: {}",
                self.config.max_agents
            ));
        }

        if agents.contains_key(&agent_key) {
            return Err(anyhow!("Agent already registered: {}", agent_key));
        }

        agents.insert(
            agent_key.clone(),
            AgentEntry {
                id: agent_id.clone(),
                status: AgentStatus::Idle,
                completed_tasks: 0,
            },
        );

        eprintln!(
            "[CentralizedTopology] Added agent {} (total: {})",
            agent_key,
            agents.len()
        );

        Ok(())
    }

    fn remove_agent(&mut self, agent_id: AgentId) -> Result<()> {
        let agent_key = agent_id_to_key(&agent_id);
        let mut agents = self.agents.write();

        if agents.remove(&agent_key).is_none() {
            return Err(anyhow!("Agent not found: {}", agent_key));
        }

        eprintln!(
            "[CentralizedTopology] Removed agent {} (remaining: {})",
            agent_key,
            agents.len()
        );

        Ok(())
    }
}

/// Statistics about the centralized topology.
#[derive(Debug, Clone)]
pub struct CentralizedStats {
    pub total_agents: usize,
    pub idle_agents: usize,
    pub busy_agents: usize,
    pub total_completed_tasks: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::types::{QueenId, TaskStatus};
    use std::time::Duration;

    #[test]
    fn test_centralized_add_remove_agents() {
        let config = CentralizedConfig::default();
        let mut topology = CentralizedTopology::new(config);

        let agent1 = AgentId::Queen(QueenId("Q1".to_string()));
        let agent2 = AgentId::Queen(QueenId("Q2".to_string()));

        // Add agents
        topology.add_agent(agent1.clone()).unwrap();
        topology.add_agent(agent2.clone()).unwrap();

        assert_eq!(topology.agents().len(), 2);

        // Remove agent
        topology.remove_agent(agent1).unwrap();
        assert_eq!(topology.agents().len(), 1);
    }

    #[test]
    fn test_centralized_round_robin() {
        let config = CentralizedConfig::default();
        let mut topology = CentralizedTopology::new(config);

        let agent1 = AgentId::Queen(QueenId("Q1".to_string()));
        let agent2 = AgentId::Queen(QueenId("Q2".to_string()));

        topology.add_agent(agent1.clone()).unwrap();
        topology.add_agent(agent2.clone()).unwrap();

        // Assign tasks and check round-robin behavior
        let task1 = TaskId("task1".to_string());
        let task2 = TaskId("task2".to_string());

        let assigned1 = topology.assign_task(&task1).unwrap();
        assert_eq!(assigned1.len(), 1);

        // Complete first task to free up agent
        let result = TaskResult {
            status: TaskStatus::Completed,
            output: "done".to_string(),
            artifacts: vec![],
            duration: Duration::from_secs(1),
            git_sha: None,
        };
        topology.handle_completion(assigned1[0].clone(), task1, result.clone());

        let assigned2 = topology.assign_task(&task2).unwrap();
        assert_eq!(assigned2.len(), 1);
    }

    #[test]
    fn test_centralized_max_agents() {
        let config = CentralizedConfig { max_agents: 2 };
        let mut topology = CentralizedTopology::new(config);

        let agent1 = AgentId::Queen(QueenId("Q1".to_string()));
        let agent2 = AgentId::Queen(QueenId("Q2".to_string()));
        let agent3 = AgentId::Queen(QueenId("Q3".to_string()));

        topology.add_agent(agent1).unwrap();
        topology.add_agent(agent2).unwrap();

        // Third agent should fail
        let result = topology.add_agent(agent3);
        assert!(result.is_err());
    }
}
