//! Large-scale deployment (100-1000 agents).
//!
//! Hierarchical structure with coordinator tiers. Each coordinator manages
//! a group of worker agents, enabling efficient management of large swarms.

use super::{generate_queen_id, ScaleDecision, ScaleMetrics, Scaling};
use crate::core::types::AgentId;
use anyhow::Result;
use serde::{Deserialize, Serialize};

// ============================================================================
// Configuration
// ============================================================================

/// Configuration for large-scale deployment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LargeScaleConfig {
    /// Minimum number of agents to maintain
    pub min_agents: usize,
    /// Maximum number of agents to spawn
    pub max_agents: usize,
    /// Number of coordinators per tier level
    pub coordinators_per_level: usize,
    /// Target number of agents per coordinator
    pub agents_per_coordinator: usize,
}

impl Default for LargeScaleConfig {
    fn default() -> Self {
        LargeScaleConfig {
            min_agents: 100,
            max_agents: 1000,
            coordinators_per_level: 10,
            agents_per_coordinator: 10,
        }
    }
}

impl LargeScaleConfig {
    /// Create a new config.
    pub fn new(min_agents: usize, max_agents: usize) -> Self {
        LargeScaleConfig {
            min_agents,
            max_agents,
            ..Default::default()
        }
    }

    /// Validate configuration.
    pub fn validate(&self) -> Result<()> {
        if self.min_agents == 0 {
            anyhow::bail!("min_agents must be at least 1");
        }
        if self.max_agents < self.min_agents {
            anyhow::bail!("max_agents must be >= min_agents");
        }
        if self.coordinators_per_level == 0 {
            anyhow::bail!("coordinators_per_level must be at least 1");
        }
        if self.agents_per_coordinator == 0 {
            anyhow::bail!("agents_per_coordinator must be at least 1");
        }
        Ok(())
    }
}

// ============================================================================
// CoordinatorTier
// ============================================================================

/// A tier in the hierarchical structure.
#[derive(Debug, Clone)]
pub struct CoordinatorTier {
    /// Tier level (0 = bottom tier of worker agents)
    pub level: usize,
    /// Coordinator agents at this tier
    pub coordinators: Vec<AgentId>,
    /// Number of agents per coordinator
    pub agents_per_coordinator: Vec<usize>,
}

impl CoordinatorTier {
    /// Create a new tier.
    pub fn new(level: usize) -> Self {
        CoordinatorTier {
            level,
            coordinators: Vec::new(),
            agents_per_coordinator: Vec::new(),
        }
    }

    /// Add a coordinator to this tier.
    pub fn add_coordinator(&mut self, id: AgentId) {
        self.coordinators.push(id);
        self.agents_per_coordinator.push(0);
    }

    /// Remove a coordinator by index.
    pub fn remove_coordinator(&mut self, index: usize) -> Option<AgentId> {
        if index < self.coordinators.len() {
            self.agents_per_coordinator.remove(index);
            Some(self.coordinators.remove(index))
        } else {
            None
        }
    }

    /// Get total agents in this tier.
    pub fn total_agents(&self) -> usize {
        self.agents_per_coordinator.iter().sum()
    }

    /// Find the coordinator with the least load.
    pub fn least_loaded_coordinator(&self) -> Option<usize> {
        if self.coordinators.is_empty() {
            return None;
        }
        self.agents_per_coordinator
            .iter()
            .enumerate()
            .min_by_key(|(_, &count)| count)
            .map(|(idx, _)| idx)
    }

    /// Find the coordinator with the most idle capacity.
    pub fn most_idle_coordinator(&self) -> Option<usize> {
        if self.coordinators.is_empty() {
            return None;
        }
        // Return coordinator with most agents (likely has idle ones)
        self.agents_per_coordinator
            .iter()
            .enumerate()
            .max_by_key(|(_, &count)| count)
            .map(|(idx, _)| idx)
    }

    /// Calculate average load per coordinator.
    pub fn average_load(&self) -> f64 {
        if self.coordinators.is_empty() {
            return 0.0;
        }
        self.total_agents() as f64 / self.coordinators.len() as f64
    }
}

// ============================================================================
// LargeScaleScaling
// ============================================================================

/// Large-scale scaling implementation with hierarchical tiers.
pub struct LargeScaleScaling {
    config: LargeScaleConfig,
    tiers: Vec<CoordinatorTier>,
    next_agent_number: usize,
}

impl LargeScaleScaling {
    /// Create a new large-scale scaling instance.
    pub fn new(config: LargeScaleConfig) -> Result<Self> {
        config.validate()?;

        // Initialize with one tier
        let mut tiers = Vec::new();
        tiers.push(CoordinatorTier::new(0));

        Ok(LargeScaleScaling {
            config,
            tiers,
            next_agent_number: 0,
        })
    }

    /// Create with default configuration.
    pub fn default() -> Result<Self> {
        Self::new(LargeScaleConfig::default())
    }

    /// Get total agent count across all tiers.
    pub fn total_agents(&self) -> usize {
        self.tiers.iter().map(|t| t.total_agents()).sum()
    }

    /// Calculate optimal number of coordinators for N agents.
    #[allow(dead_code)]
    fn optimal_coordinator_count(&self, agent_count: usize) -> usize {
        ((agent_count as f64 / self.config.agents_per_coordinator as f64).ceil() as usize)
            .max(1)
    }

    /// Rebalance agents across coordinators in a tier.
    pub fn rebalance_tier(&mut self, tier_index: usize) -> Result<()> {
        if tier_index >= self.tiers.len() {
            return Ok(());
        }

        let tier = &mut self.tiers[tier_index];
        let total_agents = tier.total_agents();
        let num_coordinators = tier.coordinators.len();

        if num_coordinators == 0 {
            return Ok(());
        }

        // Distribute evenly
        let base = total_agents / num_coordinators;
        let remainder = total_agents % num_coordinators;

        for (i, count) in tier.agents_per_coordinator.iter_mut().enumerate() {
            *count = base + if i < remainder { 1 } else { 0 };
        }

        Ok(())
    }

    /// Calculate scale up count for a specific tier.
    fn calculate_scale_up(&self, metrics: &ScaleMetrics) -> usize {
        let current = self.total_agents();
        if current >= self.config.max_agents {
            return 0;
        }

        // Scale up if queue pressure is high
        let pressure = metrics.queue_pressure();
        if pressure > 1.5 {
            // Add 5-10% of current agents
            let increment = ((current as f64 * 0.05).max(1.0)) as usize;
            increment.min(self.config.max_agents - current)
        } else {
            0
        }
    }

    /// Calculate scale down count.
    fn calculate_scale_down(&self, metrics: &ScaleMetrics) -> usize {
        let current = self.total_agents();
        if current <= self.config.min_agents {
            return 0;
        }

        let utilization = metrics.utilization();
        if utilization < 0.5 {
            // Remove 5-10% of current agents
            let decrement = ((current as f64 * 0.05).max(1.0)) as usize;
            decrement.min(current - self.config.min_agents)
        } else {
            0
        }
    }

    /// Add agents to the least loaded coordinator.
    fn add_agents_to_tier(&mut self, count: usize) -> Vec<AgentId> {
        let mut new_agents = Vec::with_capacity(count);

        // Ensure we have at least one coordinator in tier 0
        if self.tiers[0].coordinators.is_empty() {
            let coordinator_id = generate_queen_id(&format!("coord-L0-C{}", 0));
            self.tiers[0].add_coordinator(coordinator_id);
        }

        for _ in 0..count {
            // Find least loaded coordinator
            if let Some(coord_idx) = self.tiers[0].least_loaded_coordinator() {
                // Check if we should add a new coordinator
                let coord_load = self.tiers[0].agents_per_coordinator[coord_idx];
                let should_add_coordinator =
                    coord_load >= self.config.agents_per_coordinator &&
                    self.tiers[0].coordinators.len() < self.config.coordinators_per_level;

                if should_add_coordinator {
                    // Add new coordinator
                    let new_coord_id = generate_queen_id(&format!("coord-L0-C{}",
                        self.tiers[0].coordinators.len()));
                    self.tiers[0].add_coordinator(new_coord_id);
                }

                // Add agent to least loaded coordinator
                if let Some(idx) = self.tiers[0].least_loaded_coordinator() {
                    let agent_id = generate_queen_id(&format!("large-Q{}", self.next_agent_number));
                    self.next_agent_number += 1;
                    self.tiers[0].agents_per_coordinator[idx] += 1;
                    new_agents.push(agent_id);
                }
            }
        }

        new_agents
    }

    /// Remove agents from the most idle coordinator.
    fn remove_agents_from_tier(&mut self, count: usize) -> Vec<AgentId> {
        let mut removed = Vec::with_capacity(count);

        for _ in 0..count {
            if let Some(coord_idx) = self.tiers[0].most_idle_coordinator() {
                if self.tiers[0].agents_per_coordinator[coord_idx] > 0 {
                    self.tiers[0].agents_per_coordinator[coord_idx] -= 1;
                    removed.push(generate_queen_id("removed"));

                    // Remove coordinator if empty and we have more than 1
                    if self.tiers[0].agents_per_coordinator[coord_idx] == 0
                        && self.tiers[0].coordinators.len() > 1 {
                        self.tiers[0].remove_coordinator(coord_idx);
                    }
                }
            }
        }

        removed
    }
}

impl Scaling for LargeScaleScaling {
    fn should_scale(&self, metrics: &ScaleMetrics) -> Result<ScaleDecision> {
        // Check scale up first
        let scale_up_count = self.calculate_scale_up(metrics);
        if scale_up_count > 0 {
            return Ok(ScaleDecision::ScaleUp { count: scale_up_count });
        }

        // Then check scale down
        let scale_down_count = self.calculate_scale_down(metrics);
        if scale_down_count > 0 {
            return Ok(ScaleDecision::ScaleDown { count: scale_down_count });
        }

        Ok(ScaleDecision::NoAction)
    }

    fn execute_scale(&mut self, decision: ScaleDecision) -> Result<Vec<AgentId>> {
        match decision {
            ScaleDecision::ScaleUp { count } => {
                let new_agents = self.add_agents_to_tier(count);
                eprintln!("[LargeScaleScaling] Scaled up by {} agents (total: {}, coordinators: {})",
                    count, self.total_agents(), self.tiers[0].coordinators.len());
                Ok(new_agents)
            }
            ScaleDecision::ScaleDown { count } => {
                let removed = self.remove_agents_from_tier(count);
                eprintln!("[LargeScaleScaling] Scaled down by {} agents (total: {}, coordinators: {})",
                    count, self.total_agents(), self.tiers[0].coordinators.len());
                Ok(removed)
            }
            ScaleDecision::NoAction => Ok(Vec::new()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn make_metrics(active: usize, idle: usize, ready: usize) -> ScaleMetrics {
        ScaleMetrics {
            active_agents: active,
            idle_agents: idle,
            ready_tasks: ready,
            queue_depth: ready,
            avg_task_duration: Duration::from_secs(60),
            error_rate: 0.0,
        }
    }

    #[test]
    fn test_coordinator_tier() {
        let mut tier = CoordinatorTier::new(0);

        let id1 = generate_queen_id("coord-1");
        let id2 = generate_queen_id("coord-2");

        tier.add_coordinator(id1);
        tier.add_coordinator(id2);

        assert_eq!(tier.coordinators.len(), 2);
        assert_eq!(tier.agents_per_coordinator.len(), 2);
    }

    #[test]
    fn test_least_loaded_coordinator() {
        let mut tier = CoordinatorTier::new(0);
        tier.add_coordinator(generate_queen_id("c1"));
        tier.add_coordinator(generate_queen_id("c2"));
        tier.agents_per_coordinator[0] = 5;
        tier.agents_per_coordinator[1] = 3;

        let least = tier.least_loaded_coordinator().unwrap();
        assert_eq!(least, 1); // Second coordinator has 3 agents
    }

    #[test]
    fn test_scale_up_creates_coordinators() {
        let mut scaling = LargeScaleScaling::default().unwrap();

        let decision = ScaleDecision::ScaleUp { count: 15 };
        let agents = scaling.execute_scale(decision).unwrap();

        assert_eq!(agents.len(), 15);
        assert!(scaling.tiers[0].coordinators.len() >= 1);
    }

    #[test]
    fn test_scale_down_removes_empty_coordinators() {
        let mut scaling = LargeScaleScaling::default().unwrap();

        // Add agents first
        scaling.execute_scale(ScaleDecision::ScaleUp { count: 20 }).unwrap();
        let coord_count = scaling.tiers[0].coordinators.len();

        // Scale down all agents
        scaling.execute_scale(ScaleDecision::ScaleDown { count: 20 }).unwrap();

        // Should have collapsed to 1 coordinator
        assert_eq!(scaling.tiers[0].coordinators.len(), 1);
    }

    #[test]
    fn test_rebalance_tier() {
        let mut scaling = LargeScaleScaling::default().unwrap();

        // Manually create unbalanced tier
        scaling.tiers[0].add_coordinator(generate_queen_id("c1"));
        scaling.tiers[0].add_coordinator(generate_queen_id("c2"));
        scaling.tiers[0].agents_per_coordinator[0] = 10;
        scaling.tiers[0].agents_per_coordinator[1] = 2;

        scaling.rebalance_tier(0).unwrap();

        // Should be balanced to 6 and 6
        assert_eq!(scaling.tiers[0].agents_per_coordinator[0], 6);
        assert_eq!(scaling.tiers[0].agents_per_coordinator[1], 6);
    }
}
