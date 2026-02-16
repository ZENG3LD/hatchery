//! Small-scale deployment (3-8 agents).
//!
//! Simple, predictable scaling for small teams. Designed for the current
//! Hatchery setup where we typically run 3-5 Queens at a time.

use super::{generate_queen_id, ScaleDecision, ScaleMetrics, Scaling};
use crate::core::types::AgentId;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

// ============================================================================
// Configuration
// ============================================================================

/// Configuration for small-scale deployment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmallScaleConfig {
    /// Minimum number of agents to maintain
    pub min_agents: usize,
    /// Maximum number of agents to spawn
    pub max_agents: usize,
    /// Scale up when queue pressure exceeds this multiplier
    pub scale_up_multiplier: usize,
    /// Scale down when idle ratio exceeds this threshold
    pub idle_ratio_threshold: f64,
}

impl Default for SmallScaleConfig {
    fn default() -> Self {
        SmallScaleConfig {
            min_agents: 3,
            max_agents: 8,
            scale_up_multiplier: 2,
            idle_ratio_threshold: 0.5, // 50% idle
        }
    }
}

impl SmallScaleConfig {
    /// Create a new config with custom min/max.
    pub fn new(min_agents: usize, max_agents: usize) -> Self {
        SmallScaleConfig {
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
        if self.scale_up_multiplier == 0 {
            anyhow::bail!("scale_up_multiplier must be at least 1");
        }
        if self.idle_ratio_threshold < 0.0 || self.idle_ratio_threshold > 1.0 {
            anyhow::bail!("idle_ratio_threshold must be between 0.0 and 1.0");
        }
        Ok(())
    }
}

// ============================================================================
// SmallScaleScaling
// ============================================================================

/// Small-scale scaling implementation.
///
/// Scaling logic:
/// - Scale up if: ready_tasks > active_agents * scale_up_multiplier AND current < max_agents
/// - Scale down if: idle_agents / total_agents > idle_ratio_threshold AND current > min_agents
/// - Otherwise: no action
pub struct SmallScaleScaling {
    config: SmallScaleConfig,
    current_agents: HashSet<String>,
    next_agent_number: usize,
}

impl SmallScaleScaling {
    /// Create a new small-scale scaling instance.
    pub fn new(config: SmallScaleConfig) -> Result<Self> {
        config.validate()?;
        Ok(SmallScaleScaling {
            config,
            current_agents: HashSet::new(),
            next_agent_number: 0,
        })
    }

    /// Create with default configuration.
    pub fn default() -> Result<Self> {
        Self::new(SmallScaleConfig::default())
    }

    /// Register an existing agent.
    pub fn register_agent(&mut self, id: &AgentId) {
        let key = super::agent_id_to_key(id);
        self.current_agents.insert(key);
    }

    /// Remove an agent from tracking.
    pub fn unregister_agent(&mut self, id: &AgentId) {
        let key = super::agent_id_to_key(id);
        self.current_agents.remove(&key);
    }

    /// Get current agent count.
    pub fn current_count(&self) -> usize {
        self.current_agents.len()
    }

    /// Calculate scale up count.
    fn calculate_scale_up(&self, metrics: &ScaleMetrics) -> usize {
        let current = metrics.total_agents();
        if current >= self.config.max_agents {
            return 0;
        }

        let threshold = metrics.active_agents * self.config.scale_up_multiplier;
        if metrics.ready_tasks > threshold {
            // Scale up by 1 or 2 depending on pressure
            let pressure_ratio = metrics.ready_tasks as f64 / threshold.max(1) as f64;
            let count = if pressure_ratio > 2.0 {
                2 // High pressure: add 2
            } else {
                1 // Normal pressure: add 1
            };
            count.min(self.config.max_agents - current)
        } else {
            0
        }
    }

    /// Calculate scale down count.
    fn calculate_scale_down(&self, metrics: &ScaleMetrics) -> usize {
        let current = metrics.total_agents();
        if current <= self.config.min_agents {
            return 0;
        }

        let idle_ratio = metrics.idle_agents as f64 / current.max(1) as f64;
        if idle_ratio > self.config.idle_ratio_threshold {
            // Scale down by 1
            1.min(current - self.config.min_agents)
        } else {
            0
        }
    }
}

impl Scaling for SmallScaleScaling {
    fn should_scale(&self, metrics: &ScaleMetrics) -> Result<ScaleDecision> {
        // Check scale up first (higher priority)
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
                let mut new_agents = Vec::with_capacity(count);
                for _ in 0..count {
                    let agent_id = generate_queen_id(&format!("small-Q{}", self.next_agent_number));
                    self.next_agent_number += 1;

                    let key = super::agent_id_to_key(&agent_id);
                    self.current_agents.insert(key);
                    new_agents.push(agent_id);
                }

                eprintln!("[SmallScaleScaling] Scaled up by {} agents (total: {})",
                    count, self.current_agents.len());
                Ok(new_agents)
            }
            ScaleDecision::ScaleDown { count } => {
                let mut removed = Vec::with_capacity(count);
                let mut to_remove: Vec<String> = self.current_agents.iter()
                    .take(count)
                    .cloned()
                    .collect();

                for key in to_remove.drain(..) {
                    self.current_agents.remove(&key);
                    // Return a placeholder - actual removal handled by orchestrator
                    removed.push(generate_queen_id("removed"));
                }

                eprintln!("[SmallScaleScaling] Scaled down by {} agents (total: {})",
                    count, self.current_agents.len());
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
    fn test_config_validation() {
        let config = SmallScaleConfig::new(3, 8);
        assert!(config.validate().is_ok());

        let bad_config = SmallScaleConfig::new(0, 8);
        assert!(bad_config.validate().is_err());

        let bad_config2 = SmallScaleConfig::new(10, 5);
        assert!(bad_config2.validate().is_err());
    }

    #[test]
    fn test_scale_up_decision() {
        let mut scaling = SmallScaleScaling::default().unwrap();

        // 3 active, 0 idle, 10 ready tasks -> should scale up (10 > 3*2)
        let metrics = make_metrics(3, 0, 10);
        let decision = scaling.should_scale(&metrics).unwrap();
        assert!(decision.is_scale_up());
    }

    #[test]
    fn test_scale_down_decision() {
        let mut scaling = SmallScaleScaling::default().unwrap();

        // 2 active, 3 idle, 0 ready -> should scale down (60% idle)
        let metrics = make_metrics(2, 3, 0);
        let decision = scaling.should_scale(&metrics).unwrap();
        assert!(decision.is_scale_down());
    }

    #[test]
    fn test_no_action() {
        let mut scaling = SmallScaleScaling::default().unwrap();

        // 3 active, 1 idle, 4 ready -> no action (4 < 3*2, 25% idle)
        let metrics = make_metrics(3, 1, 4);
        let decision = scaling.should_scale(&metrics).unwrap();
        assert!(decision.is_no_action());
    }

    #[test]
    fn test_execute_scale_up() {
        let mut scaling = SmallScaleScaling::default().unwrap();
        let decision = ScaleDecision::ScaleUp { count: 2 };

        let agents = scaling.execute_scale(decision).unwrap();
        assert_eq!(agents.len(), 2);
        assert_eq!(scaling.current_count(), 2);
    }

    #[test]
    fn test_max_agents_limit() {
        let mut scaling = SmallScaleScaling::new(3, 5).unwrap();

        // Already at 5 agents
        let metrics = make_metrics(5, 0, 20);
        let decision = scaling.should_scale(&metrics).unwrap();
        assert!(decision.is_no_action());
    }

    #[test]
    fn test_min_agents_limit() {
        let mut scaling = SmallScaleScaling::default().unwrap();

        // At minimum (3 agents), all idle
        let metrics = make_metrics(0, 3, 0);
        let decision = scaling.should_scale(&metrics).unwrap();
        assert!(decision.is_no_action());
    }
}
