//! Hybrid topology — composes multiple topologies based on task properties.

use super::{Topology};
use crate::core::types::{AgentId, TaskId, TaskResult};
use anyhow::Result;

/// Task selector function type.
///
/// Given a task ID, returns a selection hint (e.g., "primary", "fallback", "emergency").
/// This allows routing different types of tasks to different topologies.
pub type TaskSelector = Box<dyn Fn(&TaskId) -> String + Send + Sync>;

/// Hybrid topology implementation.
///
/// Composes multiple topologies and delegates task assignment based on
/// task properties. For example:
/// - High-priority tasks -> centralized (fast assignment)
/// - Normal tasks -> peer-to-peer (democratic)
/// - Complex tasks -> conversational (deliberative)
///
/// The selector function determines which topology to use for each task.
pub struct HybridTopology {
    /// Primary topology for most tasks
    primary: Box<dyn Topology>,
    /// Optional fallback topology
    fallback: Option<Box<dyn Topology>>,
    /// Task selector function
    selector: TaskSelector,
}

impl HybridTopology {
    /// Create a new hybrid topology with a primary topology and selector.
    pub fn new(primary: Box<dyn Topology>, selector: TaskSelector) -> Self {
        HybridTopology {
            primary,
            fallback: None,
            selector,
        }
    }

    /// Create a hybrid topology with both primary and fallback topologies.
    pub fn with_fallback(
        primary: Box<dyn Topology>,
        fallback: Box<dyn Topology>,
        selector: TaskSelector,
    ) -> Self {
        HybridTopology {
            primary,
            fallback: Some(fallback),
            selector,
        }
    }

    /// Select which topology to use for a given task.
    fn select_topology(&mut self, task_id: &TaskId) -> &mut Box<dyn Topology> {
        let hint = (self.selector)(task_id);

        match hint.as_str() {
            "fallback" | "secondary" => {
                if let Some(ref mut fallback) = self.fallback {
                    eprintln!(
                        "[HybridTopology] Using fallback topology for task {}",
                        task_id.0
                    );
                    fallback
                } else {
                    eprintln!(
                        "[HybridTopology] No fallback configured, using primary for task {}",
                        task_id.0
                    );
                    &mut self.primary
                }
            }
            _ => {
                eprintln!(
                    "[HybridTopology] Using primary topology for task {}",
                    task_id.0
                );
                &mut self.primary
            }
        }
    }
}

impl Topology for HybridTopology {
    fn assign_task(&mut self, task_id: &TaskId) -> Result<Vec<AgentId>> {
        let topology = self.select_topology(task_id);
        topology.assign_task(task_id)
    }

    fn handle_completion(&mut self, agent_id: AgentId, task_id: TaskId, result: TaskResult) {
        // Delegate to primary (it handles all completions)
        self.primary.handle_completion(agent_id.clone(), task_id.clone(), result.clone());

        // Also notify fallback if it exists
        if let Some(ref mut fallback) = self.fallback {
            fallback.handle_completion(agent_id, task_id, result);
        }
    }

    fn agents(&self) -> Vec<AgentId> {
        let mut all_agents = self.primary.agents();

        if let Some(ref fallback) = self.fallback {
            all_agents.extend(fallback.agents());
        }

        // Deduplicate by converting to HashMap and back
        let mut seen = std::collections::HashSet::new();
        let mut unique = Vec::new();

        for agent in all_agents {
            let key = super::agent_id_to_key(&agent);
            if seen.insert(key) {
                unique.push(agent);
            }
        }

        unique
    }

    fn add_agent(&mut self, agent_id: AgentId) -> Result<()> {
        // Add to primary
        self.primary.add_agent(agent_id.clone())?;

        // Also add to fallback if it exists
        if let Some(ref mut fallback) = self.fallback {
            // Ignore errors from fallback (agent might already exist there)
            let _ = fallback.add_agent(agent_id.clone());
        }

        eprintln!(
            "[HybridTopology] Added agent {} to all topologies",
            super::agent_id_to_key(&agent_id)
        );

        Ok(())
    }

    fn remove_agent(&mut self, agent_id: AgentId) -> Result<()> {
        // Remove from primary
        self.primary.remove_agent(agent_id.clone())?;

        // Also remove from fallback if it exists
        if let Some(ref mut fallback) = self.fallback {
            let _ = fallback.remove_agent(agent_id.clone());
        }

        eprintln!(
            "[HybridTopology] Removed agent {} from all topologies",
            super::agent_id_to_key(&agent_id)
        );

        Ok(())
    }
}

/// Common task selector implementations.
pub mod selectors {
    use super::*;

    /// Selector based on task ID prefix.
    ///
    /// Example:
    /// - "high-*" -> "primary"
    /// - "low-*" -> "fallback"
    pub fn prefix_selector(primary_prefix: String, fallback_prefix: String) -> TaskSelector {
        Box::new(move |task_id: &TaskId| {
            if task_id.0.starts_with(&primary_prefix) {
                "primary".to_string()
            } else if task_id.0.starts_with(&fallback_prefix) {
                "fallback".to_string()
            } else {
                "primary".to_string()
            }
        })
    }

    /// Selector based on task ID length (simple heuristic).
    ///
    /// Short task IDs -> primary (assumed high priority)
    /// Long task IDs -> fallback (assumed low priority)
    pub fn length_selector(threshold: usize) -> TaskSelector {
        Box::new(move |task_id: &TaskId| {
            if task_id.0.len() < threshold {
                "primary".to_string()
            } else {
                "fallback".to_string()
            }
        })
    }

    /// Selector based on task ID hash (load balancing).
    ///
    /// Even hash -> primary
    /// Odd hash -> fallback
    pub fn hash_selector() -> TaskSelector {
        Box::new(|task_id: &TaskId| {
            let hash = task_id
                .0
                .bytes()
                .fold(0u64, |acc, b| acc.wrapping_add(b as u64));

            if hash % 2 == 0 {
                "primary".to_string()
            } else {
                "fallback".to_string()
            }
        })
    }

    /// Always use primary topology (no-op hybrid).
    pub fn always_primary() -> TaskSelector {
        Box::new(|_| "primary".to_string())
    }

    /// Round-robin between primary and fallback.
    pub fn round_robin_selector() -> TaskSelector {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        let counter = Arc::new(AtomicUsize::new(0));

        Box::new(move |_| {
            let count = counter.fetch_add(1, Ordering::Relaxed);
            if count % 2 == 0 {
                "primary".to_string()
            } else {
                "fallback".to_string()
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::types::{QueenId, TaskStatus};
    use crate::topology::{CentralizedConfig, CentralizedTopology};
    use std::time::Duration;

    #[test]
    fn test_hybrid_primary_only() {
        let primary = Box::new(CentralizedTopology::new(CentralizedConfig::default()));
        let selector = selectors::always_primary();
        let mut topology = HybridTopology::new(primary, selector);

        let agent1 = AgentId::Queen(QueenId("Q1".to_string()));
        topology.add_agent(agent1.clone()).unwrap();

        let task1 = TaskId("task1".to_string());
        let assigned = topology.assign_task(&task1).unwrap();
        assert_eq!(assigned.len(), 1);

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
    fn test_hybrid_with_fallback() {
        let primary = Box::new(CentralizedTopology::new(CentralizedConfig::default()));
        let fallback = Box::new(CentralizedTopology::new(CentralizedConfig::default()));
        let selector = selectors::prefix_selector("high-".to_string(), "low-".to_string());

        let mut topology = HybridTopology::with_fallback(primary, fallback, selector);

        let agent1 = AgentId::Queen(QueenId("Q1".to_string()));
        topology.add_agent(agent1.clone()).unwrap();

        // High priority task
        let task1 = TaskId("high-task1".to_string());
        let assigned1 = topology.assign_task(&task1).unwrap();
        assert_eq!(assigned1.len(), 1);

        let result = TaskResult {
            status: TaskStatus::Completed,
            output: "done".to_string(),
            artifacts: vec![],
            duration: Duration::from_secs(1),
            git_sha: None,
        };
        topology.handle_completion(assigned1[0].clone(), task1, result.clone());

        // Low priority task
        let task2 = TaskId("low-task2".to_string());
        let assigned2 = topology.assign_task(&task2).unwrap();
        assert_eq!(assigned2.len(), 1);

        topology.handle_completion(assigned2[0].clone(), task2, result);
    }

    #[test]
    fn test_selectors() {
        let task1 = TaskId("short".to_string());
        let task2 = TaskId("very_long_task_id".to_string());

        let selector = selectors::length_selector(10);
        assert_eq!(selector(&task1), "primary");
        assert_eq!(selector(&task2), "fallback");

        let hash_selector = selectors::hash_selector();
        let _ = hash_selector(&task1); // Just ensure it doesn't panic
    }
}
