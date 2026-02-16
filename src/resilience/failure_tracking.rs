//! Comprehensive failure analytics and tracking.

use super::{Resilience, ResilienceAction, HealthStatus, RecoveryPlan, RecoveryAction, agent_key, task_key};
use crate::core::types::{TaskId, AgentId};
use anyhow::Result;
use std::collections::HashMap;
use std::time::{Duration, Instant};

// ============================================================================
// Configuration
// ============================================================================

/// Category of failure.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FailureCategory {
    /// Request timeout
    Timeout,
    /// Agent crash or panic
    CrashError,
    /// Input/output validation error
    ValidationError,
    /// Resource exhaustion (memory, disk, etc.)
    ResourceError,
    /// External dependency failure
    DependencyError,
    /// Unknown or uncategorized error
    Unknown,
}

impl FailureCategory {
    /// Categorize an error message.
    fn categorize(error: &str) -> Self {
        let error_lower = error.to_lowercase();

        if error_lower.contains("timeout") || error_lower.contains("timed out") {
            FailureCategory::Timeout
        } else if error_lower.contains("panic") || error_lower.contains("crash") || error_lower.contains("segfault") {
            FailureCategory::CrashError
        } else if error_lower.contains("validation") || error_lower.contains("invalid") || error_lower.contains("malformed") {
            FailureCategory::ValidationError
        } else if error_lower.contains("out of memory") || error_lower.contains("disk full") || error_lower.contains("resource") {
            FailureCategory::ResourceError
        } else if error_lower.contains("connection") || error_lower.contains("network") || error_lower.contains("dependency") {
            FailureCategory::DependencyError
        } else {
            FailureCategory::Unknown
        }
    }
}

/// Configuration for failure tracking.
#[derive(Debug, Clone)]
pub struct FailureTrackingConfig {
    /// Whether to track detailed failure reasons
    pub track_reasons: bool,
    /// Maximum number of failure records to keep in history
    pub max_history: usize,
}

impl Default for FailureTrackingConfig {
    fn default() -> Self {
        FailureTrackingConfig {
            track_reasons: true,
            max_history: 1000,
        }
    }
}

// ============================================================================
// Tracking Types
// ============================================================================

/// A single failure record.
#[derive(Debug, Clone)]
struct FailureRecord {
    task_id: String,
    agent_key: String,
    error: String,
    timestamp: Instant,
    category: FailureCategory,
}

/// Aggregate failure statistics.
#[derive(Debug, Clone)]
pub struct FailureStats {
    /// Total number of failures
    pub total_failures: usize,
    /// Failures grouped by category
    pub failures_by_category: HashMap<FailureCategory, usize>,
    /// Overall failure rate (failures per hour)
    pub failure_rate: f64,
    /// Mean time between failures
    pub mtbf: Duration,
}

// ============================================================================
// Implementation
// ============================================================================

/// Failure tracking resilience handler.
pub struct FailureTrackingResilience {
    config: FailureTrackingConfig,
    failure_history: Vec<FailureRecord>,
    agent_failure_counts: HashMap<String, usize>,
    task_failure_counts: HashMap<String, usize>,
    category_counts: HashMap<FailureCategory, usize>,
    first_failure: Option<Instant>,
}

impl FailureTrackingResilience {
    /// Create a new failure tracking resilience handler with the given configuration.
    pub fn new(config: FailureTrackingConfig) -> Self {
        FailureTrackingResilience {
            config,
            failure_history: Vec::new(),
            agent_failure_counts: HashMap::new(),
            task_failure_counts: HashMap::new(),
            category_counts: HashMap::new(),
            first_failure: None,
        }
    }

    /// Create with default configuration.
    pub fn default() -> Self {
        Self::new(FailureTrackingConfig::default())
    }

    /// Record a failure event.
    fn record_failure_internal(&mut self, task_id: &TaskId, agent_id: &AgentId, error: String) {
        let task_key_str = task_key(task_id);
        let agent_key_str = agent_key(agent_id);
        let category = FailureCategory::categorize(&error);
        let timestamp = Instant::now();

        // Track first failure time
        if self.first_failure.is_none() {
            self.first_failure = Some(timestamp);
        }

        // Create failure record
        let record = FailureRecord {
            task_id: task_key_str.clone(),
            agent_key: agent_key_str.clone(),
            error: if self.config.track_reasons { error } else { String::new() },
            timestamp,
            category: category.clone(),
        };

        // Add to history (with size limit)
        self.failure_history.push(record);
        if self.failure_history.len() > self.config.max_history {
            self.failure_history.remove(0);
        }

        // Update counters
        *self.agent_failure_counts.entry(agent_key_str).or_insert(0) += 1;
        *self.task_failure_counts.entry(task_key_str).or_insert(0) += 1;
        *self.category_counts.entry(category).or_insert(0) += 1;
    }

    /// Get aggregate failure statistics.
    pub fn failure_stats(&self) -> FailureStats {
        let total = self.failure_history.len();

        let failure_rate = if let Some(first) = self.first_failure {
            let elapsed_hours = Instant::now().duration_since(first).as_secs_f64() / 3600.0;
            if elapsed_hours > 0.0 {
                total as f64 / elapsed_hours
            } else {
                0.0
            }
        } else {
            0.0
        };

        let mtbf = if total > 1 && self.first_failure.is_some() {
            let elapsed = Instant::now().duration_since(self.first_failure.unwrap());
            Duration::from_secs_f64(elapsed.as_secs_f64() / total as f64)
        } else {
            Duration::from_secs(0)
        };

        FailureStats {
            total_failures: total,
            failures_by_category: self.category_counts.clone(),
            failure_rate,
            mtbf,
        }
    }

    /// Get failure rate for a specific agent.
    pub fn agent_failure_rate(&self, agent_id: &AgentId) -> f64 {
        let key = agent_key(agent_id);
        let failures = *self.agent_failure_counts.get(&key).unwrap_or(&0);

        if let Some(first) = self.first_failure {
            let elapsed_hours = Instant::now().duration_since(first).as_secs_f64() / 3600.0;
            if elapsed_hours > 0.0 {
                failures as f64 / elapsed_hours
            } else {
                0.0
            }
        } else {
            0.0
        }
    }

    /// Get failure count for a specific task.
    pub fn task_failure_rate(&self, task_id: &TaskId) -> usize {
        let key = task_key(task_id);
        *self.task_failure_counts.get(&key).unwrap_or(&0)
    }

    /// Get the most common failure categories.
    pub fn top_failure_reasons(&self, limit: usize) -> Vec<(FailureCategory, usize)> {
        let mut categories: Vec<_> = self.category_counts.iter()
            .map(|(cat, count)| (cat.clone(), *count))
            .collect();

        categories.sort_by(|a, b| b.1.cmp(&a.1));
        categories.truncate(limit);
        categories
    }

    /// Get recent failures (last N).
    pub fn recent_failures(&self, limit: usize) -> Vec<(String, String, FailureCategory)> {
        let start = if self.failure_history.len() > limit {
            self.failure_history.len() - limit
        } else {
            0
        };

        self.failure_history[start..]
            .iter()
            .map(|r| (r.task_id.clone(), r.agent_key.clone(), r.category.clone()))
            .collect()
    }

    /// Get failures for a specific agent.
    pub fn agent_failures(&self, agent_id: &AgentId) -> Vec<(String, String, FailureCategory)> {
        let key = agent_key(agent_id);
        self.failure_history
            .iter()
            .filter(|r| r.agent_key == key)
            .map(|r| (r.task_id.clone(), r.error.clone(), r.category.clone()))
            .collect()
    }

    /// Clear all failure tracking data.
    pub fn clear(&mut self) {
        self.failure_history.clear();
        self.agent_failure_counts.clear();
        self.task_failure_counts.clear();
        self.category_counts.clear();
        self.first_failure = None;
    }

    /// Get total failure count.
    pub fn total_failures(&self) -> usize {
        self.failure_history.len()
    }
}

impl Resilience for FailureTrackingResilience {
    fn handle_failure(&mut self, task_id: TaskId, agent_id: AgentId, error: String) -> Result<ResilienceAction> {
        // Categorize and record the error
        let category = FailureCategory::categorize(&error);
        self.record_failure_internal(&task_id, &agent_id, error);

        // Determine action based on category
        let action = match category {
            FailureCategory::Timeout => {
                // Timeout - retry with longer delay
                ResilienceAction::Retry {
                    delay: Duration::from_secs(10),
                }
            }
            FailureCategory::CrashError => {
                // Crash - escalate for restart
                ResilienceAction::Escalate {
                    to: AgentId::Operator,
                }
            }
            FailureCategory::ValidationError => {
                // Validation error - likely won't fix itself, escalate
                ResilienceAction::Escalate {
                    to: AgentId::Operator,
                }
            }
            FailureCategory::ResourceError => {
                // Resource issue - retry after delay to let resources free up
                ResilienceAction::Retry {
                    delay: Duration::from_secs(30),
                }
            }
            FailureCategory::DependencyError => {
                // Dependency issue - retry after delay
                ResilienceAction::Retry {
                    delay: Duration::from_secs(15),
                }
            }
            FailureCategory::Unknown => {
                // Unknown - conservative retry
                ResilienceAction::Retry {
                    delay: Duration::from_secs(5),
                }
            }
        };

        Ok(action)
    }

    fn check_health(&self, agent_id: AgentId) -> Result<HealthStatus> {
        let failure_rate = self.agent_failure_rate(&agent_id);

        let status = if failure_rate < 1.0 {
            HealthStatus::Healthy
        } else if failure_rate < 5.0 {
            HealthStatus::Degraded
        } else {
            HealthStatus::Critical
        };

        Ok(status)
    }

    fn plan_recovery(&mut self, agent_id: AgentId) -> Result<RecoveryPlan> {
        let key = agent_key(&agent_id);
        let failures = *self.agent_failure_counts.get(&key).unwrap_or(&0);

        let action = if failures > 10 {
            // Too many failures, restart
            RecoveryAction::Restart
        } else if failures > 5 {
            // Moderate failures, try resuming
            RecoveryAction::Resume {
                session_id: format!("recovery_{}", key),
            }
        } else {
            // Few failures, continue
            RecoveryAction::Resume {
                session_id: format!("healthy_{}", key),
            }
        };

        Ok(RecoveryPlan {
            agent_id: agent_id.clone(),
            action,
        })
    }

    fn record_failure(&mut self, task_id: TaskId) -> Result<()> {
        // Create a generic failure record
        self.record_failure_internal(
            &task_id,
            &AgentId::Validator, // Default agent
            "Unspecified failure".to_string(),
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_failure_categorization() {
        assert_eq!(
            FailureCategory::categorize("connection timeout"),
            FailureCategory::Timeout
        );

        assert_eq!(
            FailureCategory::categorize("agent panic at line 42"),
            FailureCategory::CrashError
        );

        assert_eq!(
            FailureCategory::categorize("invalid input format"),
            FailureCategory::ValidationError
        );

        assert_eq!(
            FailureCategory::categorize("out of memory error"),
            FailureCategory::ResourceError
        );

        assert_eq!(
            FailureCategory::categorize("network connection failed"),
            FailureCategory::DependencyError
        );

        assert_eq!(
            FailureCategory::categorize("something went wrong"),
            FailureCategory::Unknown
        );
    }

    #[test]
    fn test_failure_stats() {
        let mut tracker = FailureTrackingResilience::default();

        // Record some failures
        for i in 0..5 {
            let task_id = TaskId(format!("task_{}", i));
            let agent_id = AgentId::Validator;
            tracker
                .handle_failure(task_id, agent_id, "timeout error".to_string())
                .unwrap();
        }

        let stats = tracker.failure_stats();
        assert_eq!(stats.total_failures, 5);
        assert!(stats.failures_by_category.contains_key(&FailureCategory::Timeout));
    }

    #[test]
    fn test_top_failure_reasons() {
        let mut tracker = FailureTrackingResilience::default();

        // Record various failures
        tracker.record_failure_internal(
            &TaskId("t1".to_string()),
            &AgentId::Validator,
            "timeout".to_string(),
        );
        tracker.record_failure_internal(
            &TaskId("t2".to_string()),
            &AgentId::Validator,
            "timeout".to_string(),
        );
        tracker.record_failure_internal(
            &TaskId("t3".to_string()),
            &AgentId::Validator,
            "panic".to_string(),
        );

        let top = tracker.top_failure_reasons(2);
        assert_eq!(top.len(), 2);
        assert_eq!(top[0].0, FailureCategory::Timeout);
        assert_eq!(top[0].1, 2);
    }

    #[test]
    fn test_max_history_limit() {
        let mut tracker = FailureTrackingResilience::new(FailureTrackingConfig {
            track_reasons: true,
            max_history: 10,
        });

        // Record more than max_history failures
        for i in 0..20 {
            tracker.record_failure_internal(
                &TaskId(format!("task_{}", i)),
                &AgentId::Validator,
                "error".to_string(),
            );
        }

        // Should only keep the last 10
        assert_eq!(tracker.failure_history.len(), 10);
        assert_eq!(tracker.total_failures(), 10);
    }
}
