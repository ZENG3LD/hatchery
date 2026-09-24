//! Human-in-the-loop intervention for critical decisions.

use super::{Resilience, ResilienceAction, HealthStatus, RecoveryPlan, RecoveryAction, agent_key, task_key};
use crate::core::types::{TaskId, AgentId};
use anyhow::{Result, anyhow};
use std::collections::HashMap;
use std::time::{Duration, Instant};

// ============================================================================
// Configuration
// ============================================================================

/// Severity level of an intervention request.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// Informational - FYI only
    Info,
    /// Warning - attention recommended
    Warning,
    /// Error - intervention helpful
    Error,
    /// Critical - intervention required
    Critical,
}

/// Configuration for HITL behavior.
#[derive(Debug, Clone)]
pub struct HitlConfig {
    /// Auto-approve threshold (confidence score 0.0-1.0)
    /// Errors with confidence below this require human approval
    pub auto_approve_threshold: f64,
    /// Timeout for human response before auto-escalation
    pub escalation_timeout: Duration,
}

impl Default for HitlConfig {
    fn default() -> Self {
        HitlConfig {
            auto_approve_threshold: 0.8,
            escalation_timeout: Duration::from_secs(300), // 5 minutes
        }
    }
}

// ============================================================================
// Request Types
// ============================================================================

/// Resolution of a HITL request.
#[derive(Debug, Clone)]
pub struct HitlResolution {
    /// Whether the request was approved
    pub approved: bool,
    /// Human feedback/guidance
    pub feedback: String,
    /// When the resolution was made
    pub resolved_at: Instant,
}

/// A human-in-the-loop intervention request.
#[derive(Debug, Clone)]
pub struct HitlRequest {
    /// Unique request ID
    pub id: String,
    /// Associated task ID
    pub task_id: String,
    /// Reason for intervention
    pub reason: String,
    /// Severity level
    pub severity: Severity,
    /// When the request was created
    pub created_at: Instant,
    /// Whether this request has been resolved
    pub resolved: bool,
    /// Resolution details (if resolved)
    pub resolution: Option<HitlResolution>,
}

impl HitlRequest {
    fn new(id: String, task_id: String, reason: String, severity: Severity) -> Self {
        HitlRequest {
            id,
            task_id,
            reason,
            severity,
            created_at: Instant::now(),
            resolved: false,
            resolution: None,
        }
    }

    fn is_expired(&self, timeout: Duration) -> bool {
        Instant::now().duration_since(self.created_at) > timeout
    }
}

// ============================================================================
// Implementation
// ============================================================================

/// Human-in-the-loop resilience handler.
pub struct HitlResilience {
    config: HitlConfig,
    pending_requests: HashMap<String, HitlRequest>,
    request_counter: usize,
}

impl HitlResilience {
    /// Create a new HITL resilience handler with the given configuration.
    pub fn new(config: HitlConfig) -> Self {
        HitlResilience {
            config,
            pending_requests: HashMap::new(),
            request_counter: 0,
        }
    }

    /// Create with default configuration.
    pub fn default() -> Self {
        Self::new(HitlConfig::default())
    }

    /// Create a new intervention request.
    pub fn create_request(&mut self, task_id: TaskId, reason: String, severity: Severity) -> String {
        self.request_counter += 1;
        let request_id = format!("hitl_{}", self.request_counter);

        let request = HitlRequest::new(
            request_id.clone(),
            task_key(&task_id),
            reason,
            severity,
        );

        self.pending_requests.insert(request_id.clone(), request);
        request_id
    }

    /// Resolve a request with human input.
    pub fn resolve_request(&mut self, request_id: &str, approved: bool, feedback: String) -> Result<()> {
        let request = self.pending_requests
            .get_mut(request_id)
            .ok_or_else(|| anyhow!("Request not found: {}", request_id))?;

        if request.resolved {
            return Err(anyhow!("Request already resolved"));
        }

        request.resolved = true;
        request.resolution = Some(HitlResolution {
            approved,
            feedback,
            resolved_at: Instant::now(),
        });

        Ok(())
    }

    /// Get all pending (unresolved) requests.
    pub fn pending_requests(&self) -> Vec<HitlRequest> {
        self.pending_requests
            .values()
            .filter(|r| !r.resolved)
            .cloned()
            .collect()
    }

    /// Get a specific request by ID.
    pub fn get_request(&self, request_id: &str) -> Option<&HitlRequest> {
        self.pending_requests.get(request_id)
    }

    /// Check if a request is pending for a task.
    pub fn has_pending_request(&self, task_id: &TaskId) -> bool {
        let task_key_str = task_key(task_id);
        self.pending_requests
            .values()
            .any(|r| !r.resolved && r.task_id == task_key_str)
    }

    /// Auto-escalate expired requests.
    pub fn auto_escalate_expired(&mut self) -> Vec<String> {
        let timeout = self.config.escalation_timeout;
        let mut escalated = Vec::new();

        for (id, request) in self.pending_requests.iter_mut() {
            if !request.resolved && request.is_expired(timeout) {
                request.resolved = true;
                request.resolution = Some(HitlResolution {
                    approved: false,
                    feedback: "Auto-escalated due to timeout".to_string(),
                    resolved_at: Instant::now(),
                });
                escalated.push(id.clone());
            }
        }

        escalated
    }

    /// Clean up resolved requests.
    pub fn cleanup_resolved(&mut self) {
        self.pending_requests.retain(|_, r| !r.resolved);
    }

    /// Compute error confidence score (simple heuristic based on error message).
    fn compute_error_confidence(&self, error: &str) -> f64 {
        // Simple heuristic: shorter, more specific errors = higher confidence
        let length_score = 1.0 - (error.len() as f64 / 1000.0).min(0.5);

        // Check for uncertain keywords
        let uncertain_keywords = ["maybe", "might", "possibly", "unclear", "unknown"];
        let uncertainty_penalty = if uncertain_keywords.iter().any(|k| error.to_lowercase().contains(k)) {
            0.3
        } else {
            0.0
        };

        (length_score - uncertainty_penalty).max(0.0).min(1.0)
    }
}

impl Resilience for HitlResilience {
    fn handle_failure(&mut self, task_id: TaskId, _agent_id: AgentId, error: String) -> Result<ResilienceAction> {
        // Compute confidence in handling this error automatically
        let confidence = self.compute_error_confidence(&error);

        if confidence >= self.config.auto_approve_threshold {
            // High confidence - auto-retry
            Ok(ResilienceAction::Retry {
                delay: Duration::from_secs(5),
            })
        } else {
            // Low confidence - escalate to human
            let severity = if confidence < 0.3 {
                Severity::Critical
            } else if confidence < 0.5 {
                Severity::Error
            } else {
                Severity::Warning
            };

            let _request_id = self.create_request(
                task_id.clone(),
                format!("Task failed with error: {}", error),
                severity,
            );

            // Escalate to operator for decision
            Ok(ResilienceAction::Escalate {
                to: AgentId::Operator,
            })
        }
    }

    fn check_health(&self, _agent_id: AgentId) -> Result<HealthStatus> {
        let pending_count = self.pending_requests().len();

        let status = if pending_count == 0 {
            HealthStatus::Healthy
        } else if pending_count < 3 {
            HealthStatus::Degraded
        } else {
            HealthStatus::Critical
        };

        Ok(status)
    }

    fn plan_recovery(&mut self, agent_id: AgentId) -> Result<RecoveryPlan> {
        // Create HITL request for recovery decision
        let request_id = self.create_request(
            TaskId(format!("recovery_{}", agent_key(&agent_id))),
            format!("Agent {:?} requires recovery", agent_id),
            Severity::Error,
        );

        // Default to waiting for human response
        Ok(RecoveryPlan {
            agent_id: agent_id.clone(),
            action: RecoveryAction::Resume {
                session_id: format!("hitl_pending_{}", request_id),
            },
        })
    }

    fn record_failure(&mut self, _task_id: TaskId) -> Result<()> {
        // HITL doesn't track individual failures
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auto_approval() {
        let mut hitl = HitlResilience::new(HitlConfig {
            auto_approve_threshold: 0.8,
            escalation_timeout: Duration::from_secs(300),
        });

        let task_id = TaskId("task1".to_string());
        let agent_id = AgentId::Validator;

        // Short, clear error should auto-approve retry
        let action = hitl
            .handle_failure(task_id, agent_id, "timeout".to_string())
            .unwrap();

        assert!(matches!(action, ResilienceAction::Retry { .. }));
    }

    #[test]
    fn test_human_escalation() {
        let mut hitl = HitlResilience::new(HitlConfig {
            auto_approve_threshold: 0.8,
            escalation_timeout: Duration::from_secs(300),
        });

        let task_id = TaskId("task1".to_string());
        let agent_id = AgentId::Validator;

        // Long, uncertain error should escalate
        let action = hitl
            .handle_failure(
                task_id,
                agent_id,
                "maybe this is unclear and possibly unknown error that might be related to something".to_string(),
            )
            .unwrap();

        assert!(matches!(action, ResilienceAction::Escalate { .. }));
    }

    #[test]
    fn test_request_resolution() {
        let mut hitl = HitlResilience::default();

        let request_id = hitl.create_request(
            TaskId("task1".to_string()),
            "Test reason".to_string(),
            Severity::Warning,
        );

        // Should have one pending request
        assert_eq!(hitl.pending_requests().len(), 1);

        // Resolve the request
        hitl.resolve_request(&request_id, true, "Looks good".to_string())
            .unwrap();

        // Should have no pending requests
        assert_eq!(hitl.pending_requests().len(), 0);
    }
}
