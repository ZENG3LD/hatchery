//! Retry resilience with configurable backoff strategies.

use super::{Resilience, ResilienceAction, HealthStatus, RecoveryPlan, RecoveryAction, agent_key, task_key};
use crate::core::types::{TaskId, AgentId};
use anyhow::Result;
use std::collections::HashMap;
use std::time::{Duration, Instant};

// ============================================================================
// Configuration
// ============================================================================

/// Backoff strategy for retry delays.
#[derive(Debug, Clone, Copy)]
pub enum BackoffStrategy {
    /// Retry immediately without delay
    Immediate,
    /// Linear backoff: delay = base_delay * attempt
    Linear,
    /// Exponential backoff: delay = base_delay * 2^attempt
    Exponential,
    /// Exponential with jitter: adds random variance to prevent thundering herd
    ExponentialWithJitter,
}

/// Configuration for retry behavior.
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Maximum number of retry attempts before giving up
    pub max_retries: usize,
    /// Backoff strategy to use
    pub backoff_strategy: BackoffStrategy,
    /// Base delay for backoff calculations
    pub base_delay: Duration,
}

impl Default for RetryConfig {
    fn default() -> Self {
        RetryConfig {
            max_retries: 3,
            backoff_strategy: BackoffStrategy::ExponentialWithJitter,
            base_delay: Duration::from_secs(1),
        }
    }
}

// ============================================================================
// State Tracking
// ============================================================================

/// State of a task being retried.
#[derive(Debug, Clone)]
struct RetryState {
    task_id: String,
    attempt: usize,
    last_error: String,
    last_attempt: Instant,
}

// ============================================================================
// Implementation
// ============================================================================

/// Retry resilience handler with configurable backoff.
pub struct RetryResilience {
    config: RetryConfig,
    retry_states: HashMap<String, RetryState>,
    agent_retry_counts: HashMap<String, usize>,
}

impl RetryResilience {
    /// Create a new retry resilience handler with the given configuration.
    pub fn new(config: RetryConfig) -> Self {
        RetryResilience {
            config,
            retry_states: HashMap::new(),
            agent_retry_counts: HashMap::new(),
        }
    }

    /// Create with default configuration.
    pub fn default() -> Self {
        Self::new(RetryConfig::default())
    }

    /// Compute the delay for the next retry attempt based on the backoff strategy.
    fn compute_delay(&self, attempt: usize) -> Duration {
        match self.config.backoff_strategy {
            BackoffStrategy::Immediate => Duration::from_secs(0),
            BackoffStrategy::Linear => {
                self.config.base_delay * (attempt as u32)
            }
            BackoffStrategy::Exponential => {
                let multiplier = 2_u32.pow(attempt as u32);
                self.config.base_delay * multiplier
            }
            BackoffStrategy::ExponentialWithJitter => {
                let multiplier = 2_u32.pow(attempt as u32);
                let base = self.config.base_delay * multiplier;

                // Add up to 50% jitter
                let jitter_millis = (base.as_millis() as f64 * 0.5 * rand()) as u64;
                base + Duration::from_millis(jitter_millis)
            }
        }
    }

    /// Get the current retry count for a task.
    pub fn get_retry_count(&self, task_id: &TaskId) -> usize {
        let key = task_key(task_id);
        self.retry_states
            .get(&key)
            .map(|state| state.attempt)
            .unwrap_or(0)
    }

    /// Reset retry state for a task (useful after successful completion).
    pub fn reset_task(&mut self, task_id: &TaskId) {
        let key = task_key(task_id);
        self.retry_states.remove(&key);
    }

    /// Get total retries across all tasks for an agent.
    pub fn agent_retry_count(&self, agent_id: &AgentId) -> usize {
        let key = agent_key(agent_id);
        *self.agent_retry_counts.get(&key).unwrap_or(&0)
    }

    /// Calculate overall retry rate (retrying tasks / total tasks).
    fn retry_rate(&self) -> f64 {
        if self.retry_states.is_empty() {
            return 0.0;
        }

        let retrying = self.retry_states.len();
        let total = retrying; // We only track tasks that have been retried

        retrying as f64 / total as f64
    }
}

impl Resilience for RetryResilience {
    fn handle_failure(&mut self, task_id: TaskId, agent_id: AgentId, error: String) -> Result<ResilienceAction> {
        let task_key_str = task_key(&task_id);
        let agent_key_str = agent_key(&agent_id);

        // Get or create retry state
        let attempt = if let Some(state) = self.retry_states.get_mut(&task_key_str) {
            state.attempt += 1;
            state.last_error = error.clone();
            state.last_attempt = Instant::now();
            state.attempt
        } else {
            let state = RetryState {
                task_id: task_key_str.clone(),
                attempt: 1,
                last_error: error.clone(),
                last_attempt: Instant::now(),
            };
            self.retry_states.insert(task_key_str.clone(), state);
            1
        };

        // Update agent retry counter
        *self.agent_retry_counts.entry(agent_key_str).or_insert(0) += 1;

        // Decide action based on retry count
        if attempt <= self.config.max_retries {
            let delay = self.compute_delay(attempt);
            Ok(ResilienceAction::Retry { delay })
        } else {
            // Max retries exceeded, abandon task
            self.retry_states.remove(&task_key_str);
            Ok(ResilienceAction::Abandon)
        }
    }

    fn check_health(&self, _agent_id: AgentId) -> Result<HealthStatus> {
        let rate = self.retry_rate();

        let status = if rate < 0.5 {
            HealthStatus::Healthy
        } else if rate < 0.8 {
            HealthStatus::Degraded
        } else {
            HealthStatus::Critical
        };

        Ok(status)
    }

    fn plan_recovery(&mut self, agent_id: AgentId) -> Result<RecoveryPlan> {
        let agent_retries = self.agent_retry_count(&agent_id);

        let action = if agent_retries > self.config.max_retries * 3 {
            // Too many retries, restart the agent
            RecoveryAction::Restart
        } else {
            // Continue with current agent
            RecoveryAction::Resume {
                session_id: format!("retry_session_{}", agent_key(&agent_id)),
            }
        };

        Ok(RecoveryPlan {
            agent_id: agent_id.clone(),
            action,
        })
    }

    fn record_failure(&mut self, _task_id: TaskId) -> Result<()> {
        // Recording is handled in handle_failure
        // This is a no-op for retry resilience
        Ok(())
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Simple pseudo-random number generator (0.0 to 1.0) using current time.
/// This is a basic implementation to avoid external dependencies.
fn rand() -> f64 {
    use std::time::SystemTime;

    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or(Duration::from_secs(0))
        .subsec_nanos();

    (nanos % 1000) as f64 / 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_retry_escalation() {
        let mut retry = RetryResilience::new(RetryConfig {
            max_retries: 2,
            backoff_strategy: BackoffStrategy::Linear,
            base_delay: Duration::from_secs(1),
        });

        let task_id = TaskId("task1".to_string());
        let agent_id = AgentId::Validator;

        // First failure - should retry
        let action = retry.handle_failure(task_id.clone(), agent_id.clone(), "error1".to_string()).unwrap();
        assert!(matches!(action, ResilienceAction::Retry { .. }));

        // Second failure - should retry
        let action = retry.handle_failure(task_id.clone(), agent_id.clone(), "error2".to_string()).unwrap();
        assert!(matches!(action, ResilienceAction::Retry { .. }));

        // Third failure - should abandon
        let action = retry.handle_failure(task_id.clone(), agent_id.clone(), "error3".to_string()).unwrap();
        assert!(matches!(action, ResilienceAction::Abandon));
    }

    #[test]
    fn test_backoff_delay() {
        let retry = RetryResilience::new(RetryConfig {
            max_retries: 5,
            backoff_strategy: BackoffStrategy::Exponential,
            base_delay: Duration::from_secs(1),
        });

        // Exponential: 1s * 2^1 = 2s
        assert_eq!(retry.compute_delay(1), Duration::from_secs(2));

        // Exponential: 1s * 2^2 = 4s
        assert_eq!(retry.compute_delay(2), Duration::from_secs(4));

        // Exponential: 1s * 2^3 = 8s
        assert_eq!(retry.compute_delay(3), Duration::from_secs(8));
    }
}
