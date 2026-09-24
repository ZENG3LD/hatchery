//! Graceful degradation with task-level fallback strategies.

use super::{Resilience, ResilienceAction, HealthStatus, RecoveryPlan, RecoveryAction, agent_key, task_key};
use crate::core::types::{TaskId, AgentId};
use anyhow::Result;
use std::collections::HashMap;

// ============================================================================
// Configuration
// ============================================================================

/// Fallback strategy for degraded operations.
#[derive(Debug, Clone)]
pub enum FallbackStrategy {
    /// Use cached data instead of fetching fresh data
    CachedData,
    /// Use a simplified approach with reduced complexity
    SimplifiedApproach,
    /// Require manual intervention from operator
    ManualIntervention,
    /// Skip optional features/steps
    SkipOptional,
    /// Reduce quality/accuracy for performance
    ReduceQuality,
}

/// Configuration for degradation behavior.
#[derive(Debug, Clone)]
pub struct DegradationConfig {
    /// Fallback strategies per task type (task_type -> strategy)
    pub fallback_strategies: HashMap<String, FallbackStrategy>,
    /// Default strategy if no specific strategy is defined
    pub default_strategy: FallbackStrategy,
}

impl Default for DegradationConfig {
    fn default() -> Self {
        DegradationConfig {
            fallback_strategies: HashMap::new(),
            default_strategy: FallbackStrategy::SimplifiedApproach,
        }
    }
}

// ============================================================================
// State Tracking
// ============================================================================

/// Degradation level tracking.
#[derive(Debug, Clone)]
struct DegradationLevel {
    /// Current degradation level (0.0 = healthy, 1.0 = fully degraded)
    current: f64,
    /// Threshold for degraded status
    threshold_degraded: f64,
    /// Threshold for critical status
    threshold_critical: f64,
}

impl Default for DegradationLevel {
    fn default() -> Self {
        DegradationLevel {
            current: 0.0,
            threshold_degraded: 0.3,
            threshold_critical: 0.7,
        }
    }
}

impl DegradationLevel {
    fn status(&self) -> HealthStatus {
        if self.current < self.threshold_degraded {
            HealthStatus::Healthy
        } else if self.current < self.threshold_critical {
            HealthStatus::Degraded
        } else {
            HealthStatus::Critical
        }
    }

    fn increase(&mut self, amount: f64) {
        self.current = (self.current + amount).min(1.0);
    }

    fn decrease(&mut self, amount: f64) {
        self.current = (self.current - amount).max(0.0);
    }
}

// ============================================================================
// Implementation
// ============================================================================

/// Degradation resilience handler with fallback strategies.
pub struct DegradationResilience {
    config: DegradationConfig,
    degradation_levels: HashMap<String, DegradationLevel>,
    applied_fallbacks: HashMap<String, FallbackStrategy>, // task_id -> strategy
    failure_counts: HashMap<String, usize>, // task_type -> count
}

impl DegradationResilience {
    /// Create a new degradation resilience handler with the given configuration.
    pub fn new(config: DegradationConfig) -> Self {
        DegradationResilience {
            config,
            degradation_levels: HashMap::new(),
            applied_fallbacks: HashMap::new(),
            failure_counts: HashMap::new(),
        }
    }

    /// Create with default configuration.
    pub fn default() -> Self {
        Self::new(DegradationConfig::default())
    }

    /// Get or create degradation level for an agent.
    fn get_or_create_level(&mut self, agent_key: String) -> &mut DegradationLevel {
        self.degradation_levels
            .entry(agent_key)
            .or_insert_with(DegradationLevel::default)
    }

    /// Extract task type from task ID (assumes format like "type:id" or just uses whole ID).
    fn extract_task_type(&self, task_id: &TaskId) -> String {
        let id = &task_id.0;
        if let Some(colon_pos) = id.find(':') {
            id[..colon_pos].to_string()
        } else {
            id.clone()
        }
    }

    /// Get fallback strategy for a task type.
    fn get_fallback_strategy(&self, task_type: &str) -> FallbackStrategy {
        self.config
            .fallback_strategies
            .get(task_type)
            .cloned()
            .unwrap_or_else(|| self.config.default_strategy.clone())
    }

    /// Apply a fallback strategy and return the alternative action.
    fn apply_fallback(&mut self, task_id: &TaskId, strategy: FallbackStrategy) -> ResilienceAction {
        let task_key_str = task_key(task_id);

        // Record the applied fallback
        self.applied_fallbacks.insert(task_key_str.clone(), strategy.clone());

        // Generate alternative task ID based on strategy
        let alternative_task_id = match strategy {
            FallbackStrategy::CachedData => {
                TaskId(format!("{}_cached", task_id.0))
            }
            FallbackStrategy::SimplifiedApproach => {
                TaskId(format!("{}_simplified", task_id.0))
            }
            FallbackStrategy::ManualIntervention => {
                return ResilienceAction::Escalate {
                    to: AgentId::Operator,
                };
            }
            FallbackStrategy::SkipOptional => {
                TaskId(format!("{}_minimal", task_id.0))
            }
            FallbackStrategy::ReduceQuality => {
                TaskId(format!("{}_lowquality", task_id.0))
            }
        };

        ResilienceAction::Fallback {
            alternative_task_id,
        }
    }

    /// Reset degradation level after successful operations.
    pub fn reset_degradation(&mut self, agent_id: &AgentId) {
        let key = agent_key(agent_id);
        if let Some(level) = self.degradation_levels.get_mut(&key) {
            level.decrease(0.1);
        }
    }

    /// Get current degradation level for an agent.
    pub fn get_degradation_level(&self, agent_id: &AgentId) -> f64 {
        let key = agent_key(agent_id);
        self.degradation_levels
            .get(&key)
            .map(|level| level.current)
            .unwrap_or(0.0)
    }

    /// Register a fallback strategy for a task type.
    pub fn register_fallback(&mut self, task_type: String, strategy: FallbackStrategy) {
        self.config.fallback_strategies.insert(task_type, strategy);
    }

    /// Get the number of failures for a task type.
    pub fn get_failure_count(&self, task_type: &str) -> usize {
        *self.failure_counts.get(task_type).unwrap_or(&0)
    }
}

impl Resilience for DegradationResilience {
    fn handle_failure(&mut self, task_id: TaskId, agent_id: AgentId, _error: String) -> Result<ResilienceAction> {
        let agent_key_str = agent_key(&agent_id);
        let task_type = self.extract_task_type(&task_id);

        // Increase degradation level
        let level = self.get_or_create_level(agent_key_str);
        level.increase(0.15);

        // Increment failure count for this task type
        *self.failure_counts.entry(task_type.clone()).or_insert(0) += 1;

        // Get and apply fallback strategy
        let strategy = self.get_fallback_strategy(&task_type);
        let action = self.apply_fallback(&task_id, strategy);

        Ok(action)
    }

    fn check_health(&self, agent_id: AgentId) -> Result<HealthStatus> {
        let key = agent_key(&agent_id);

        if let Some(level) = self.degradation_levels.get(&key) {
            Ok(level.status())
        } else {
            Ok(HealthStatus::Healthy)
        }
    }

    fn plan_recovery(&mut self, agent_id: AgentId) -> Result<RecoveryPlan> {
        let agent_key_str = agent_key(&agent_id);
        let level = self.get_or_create_level(agent_key_str);

        let action = if level.current >= level.threshold_critical {
            // Critical degradation - restart
            RecoveryAction::Restart
        } else if level.current >= level.threshold_degraded {
            // Moderate degradation - resume with fallback strategies active
            RecoveryAction::Resume {
                session_id: format!("degraded_session_{}", agent_key(&agent_id)),
            }
        } else {
            // Healthy - no recovery needed, but return a resume action
            RecoveryAction::Resume {
                session_id: format!("healthy_session_{}", agent_key(&agent_id)),
            }
        };

        Ok(RecoveryPlan {
            agent_id: agent_id.clone(),
            action,
        })
    }

    fn record_failure(&mut self, task_id: TaskId) -> Result<()> {
        let task_type = self.extract_task_type(&task_id);
        *self.failure_counts.entry(task_type).or_insert(0) += 1;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_degradation_progression() {
        let mut degradation = DegradationResilience::default();
        let agent_id = AgentId::Validator;

        // Should start healthy
        let status = degradation.check_health(agent_id.clone()).unwrap();
        assert_eq!(status, HealthStatus::Healthy);

        // Trigger multiple failures
        for i in 0..3 {
            let task_id = TaskId(format!("task_{}", i));
            degradation
                .handle_failure(task_id, agent_id.clone(), "error".to_string())
                .unwrap();
        }

        // Should now be degraded
        let status = degradation.check_health(agent_id.clone()).unwrap();
        assert!(matches!(status, HealthStatus::Degraded | HealthStatus::Critical));
    }

    #[test]
    fn test_fallback_strategies() {
        let mut config = DegradationConfig::default();
        config.fallback_strategies.insert(
            "fetch".to_string(),
            FallbackStrategy::CachedData,
        );

        let mut degradation = DegradationResilience::new(config);
        let task_id = TaskId("fetch:data".to_string());
        let agent_id = AgentId::Validator;

        let action = degradation
            .handle_failure(task_id, agent_id, "network error".to_string())
            .unwrap();

        // Should use cached data fallback
        assert!(matches!(action, ResilienceAction::Fallback { .. }));
    }
}
