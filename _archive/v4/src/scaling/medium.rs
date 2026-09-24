//! Medium-scale deployment (10-100 agents).
//!
//! Gradual scaling with cooldown periods to avoid thrashing.

use super::{generate_queen_id, ScaleDecision, ScaleMetrics, Scaling};
use crate::core::types::AgentId;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::time::{Duration, Instant};

// ============================================================================
// Configuration
// ============================================================================

/// Configuration for medium-scale deployment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediumScaleConfig {
    /// Minimum number of agents to maintain
    pub min_agents: usize,
    /// Maximum number of agents to spawn
    pub max_agents: usize,
    /// Scale up when queue_depth / active_agents exceeds this
    pub scale_up_threshold: f64,
    /// Scale down when idle_agents / total_agents exceeds this
    pub scale_down_threshold: f64,
    /// Minimum time between scale events
    pub cooldown: Duration,
}

impl Default for MediumScaleConfig {
    fn default() -> Self {
        MediumScaleConfig {
            min_agents: 10,
            max_agents: 100,
            scale_up_threshold: 2.0,   // 2 tasks per agent
            scale_down_threshold: 0.4, // 40% idle
            cooldown: Duration::from_secs(30),
        }
    }
}

impl MediumScaleConfig {
    /// Create a new config.
    pub fn new(min_agents: usize, max_agents: usize) -> Self {
        MediumScaleConfig {
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
        if self.scale_up_threshold <= 0.0 {
            anyhow::bail!("scale_up_threshold must be positive");
        }
        if self.scale_down_threshold < 0.0 || self.scale_down_threshold > 1.0 {
            anyhow::bail!("scale_down_threshold must be between 0.0 and 1.0");
        }
        Ok(())
    }
}

// ============================================================================
// MediumScaleScaling
// ============================================================================

/// Medium-scale scaling implementation.
///
/// Features:
/// - Gradual scaling based on queue pressure
/// - Cooldown period between scale events
/// - Batch sizing: scale up by sqrt(ready_tasks), scale down by half of idle
pub struct MediumScaleScaling {
    config: MediumScaleConfig,
    current_agents: HashSet<String>,
    last_scale_event: Option<Instant>,
    next_agent_number: usize,
}

impl MediumScaleScaling {
    /// Create a new medium-scale scaling instance.
    pub fn new(config: MediumScaleConfig) -> Result<Self> {
        config.validate()?;
        Ok(MediumScaleScaling {
            config,
            current_agents: HashSet::new(),
            last_scale_event: None,
            next_agent_number: 0,
        })
    }

    /// Create with default configuration.
    pub fn default() -> Result<Self> {
        Self::new(MediumScaleConfig::default())
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

    /// Check if we're in cooldown period.
    fn in_cooldown(&self) -> bool {
        if let Some(last) = self.last_scale_event {
            last.elapsed() < self.config.cooldown
        } else {
            false
        }
    }

    /// Calculate scale up count using sqrt batching.
    fn calculate_scale_up(&self, metrics: &ScaleMetrics) -> usize {
        let current = metrics.total_agents();
        if current >= self.config.max_agents {
            return 0;
        }

        let queue_pressure = if metrics.active_agents > 0 {
            metrics.queue_depth as f64 / metrics.active_agents as f64
        } else {
            f64::INFINITY
        };

        if queue_pressure > self.config.scale_up_threshold {
            // Batch size: sqrt of ready tasks, clamped to reasonable limits
            let batch = (metrics.ready_tasks as f64).sqrt().ceil() as usize;
            let clamped = batch.max(1).min(10); // Between 1 and 10
            clamped.min(self.config.max_agents - current)
        } else {
            0
        }
    }

    /// Calculate scale down count (half of idle agents).
    fn calculate_scale_down(&self, metrics: &ScaleMetrics) -> usize {
        let current = metrics.total_agents();
        if current <= self.config.min_agents {
            return 0;
        }

        let idle_ratio = metrics.idle_agents as f64 / current.max(1) as f64;
        if idle_ratio > self.config.scale_down_threshold {
            // Remove half of idle agents
            let to_remove = (metrics.idle_agents / 2).max(1);
            to_remove.min(current - self.config.min_agents)
        } else {
            0
        }
    }
}

impl Scaling for MediumScaleScaling {
    fn should_scale(&self, metrics: &ScaleMetrics) -> Result<ScaleDecision> {
        // Check cooldown
        if self.in_cooldown() {
            return Ok(ScaleDecision::NoAction);
        }

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
                let mut new_agents = Vec::with_capacity(count);
                for _ in 0..count {
                    let agent_id = generate_queen_id(&format!("med-Q{}", self.next_agent_number));
                    self.next_agent_number += 1;

                    let key = super::agent_id_to_key(&agent_id);
                    self.current_agents.insert(key);
                    new_agents.push(agent_id);
                }

                self.last_scale_event = Some(Instant::now());
                eprintln!("[MediumScaleScaling] Scaled up by {} agents (total: {})",
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
                    removed.push(generate_queen_id("removed"));
                }

                self.last_scale_event = Some(Instant::now());
                eprintln!("[MediumScaleScaling] Scaled down by {} agents (total: {})",
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
        let config = MediumScaleConfig::new(10, 100);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_scale_up_decision() {
        let mut scaling = MediumScaleScaling::default().unwrap();

        // 10 active, 0 idle, 30 ready tasks -> queue pressure = 3.0 > 2.0
        let metrics = make_metrics(10, 0, 30);
        let decision = scaling.should_scale(&metrics).unwrap();
        assert!(decision.is_scale_up());
    }

    #[test]
    fn test_scale_down_decision() {
        let mut scaling = MediumScaleScaling::default().unwrap();

        // 5 active, 7 idle (12 total), 0 ready -> 58% idle > 40%
        let metrics = make_metrics(5, 7, 0);
        let decision = scaling.should_scale(&metrics).unwrap();
        assert!(decision.is_scale_down());
    }

    #[test]
    fn test_cooldown() {
        let mut config = MediumScaleConfig::default();
        config.cooldown = Duration::from_millis(100);
        let mut scaling = MediumScaleScaling::new(config).unwrap();

        // First scale should work
        let metrics = make_metrics(10, 0, 30);
        let decision = scaling.should_scale(&metrics).unwrap();
        assert!(decision.is_scale_up());
        scaling.execute_scale(decision).unwrap();

        // Immediate second scale should be blocked by cooldown
        let decision2 = scaling.should_scale(&metrics).unwrap();
        assert!(decision2.is_no_action());

        // After cooldown, should work again
        std::thread::sleep(Duration::from_millis(150));
        let decision3 = scaling.should_scale(&metrics).unwrap();
        assert!(decision3.is_scale_up());
    }

    #[test]
    fn test_batch_sizing() {
        let mut scaling = MediumScaleScaling::default().unwrap();

        // 100 ready tasks -> sqrt(100) = 10 agents
        let metrics = make_metrics(10, 0, 100);
        let decision = scaling.should_scale(&metrics).unwrap();
        if let ScaleDecision::ScaleUp { count } = decision {
            assert_eq!(count, 10);
        } else {
            panic!("Expected ScaleUp");
        }
    }
}
