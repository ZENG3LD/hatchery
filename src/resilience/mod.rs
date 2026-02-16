//! Resilience module for Hatchery swarm orchestration.
//!
//! Provides failure handling, recovery, degradation, consensus, HITL, and circuit breaker patterns
//! for robust multi-agent coordination.

use crate::core::types::{TaskId, AgentId};
use anyhow::Result;
use std::time::Duration;

// ============================================================================
// Core Trait
// ============================================================================

/// Core resilience trait for handling failures and coordinating recovery.
pub trait Resilience: Send + Sync {
    /// Handle a task failure and determine the appropriate action.
    fn handle_failure(&mut self, task_id: TaskId, agent_id: AgentId, error: String) -> Result<ResilienceAction>;

    /// Check the health status of an agent.
    fn check_health(&self, agent_id: AgentId) -> Result<HealthStatus>;

    /// Plan recovery strategy for a failed agent.
    fn plan_recovery(&mut self, agent_id: AgentId) -> Result<RecoveryPlan>;

    /// Record a failure event for tracking and analysis.
    fn record_failure(&mut self, task_id: TaskId) -> Result<()>;
}

// ============================================================================
// Core Types
// ============================================================================

/// Action to take in response to a failure.
#[derive(Debug, Clone)]
pub enum ResilienceAction {
    /// Retry the task after a delay
    Retry { delay: Duration },
    /// Escalate to a different agent
    Escalate { to: AgentId },
    /// Use a fallback strategy or alternative task
    Fallback { alternative_task_id: TaskId },
    /// Abandon the task entirely
    Abandon,
}

/// Health status of an agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HealthStatus {
    /// Functioning normally
    Healthy,
    /// Degraded performance but still operational
    Degraded,
    /// Critical state, requires intervention
    Critical,
    /// Non-responsive or crashed
    Dead,
}

/// Recovery plan for a failed agent.
#[derive(Debug, Clone)]
pub struct RecoveryPlan {
    /// Agent to recover
    pub agent_id: AgentId,
    /// Recovery action to take
    pub action: RecoveryAction,
}

/// Type of recovery action.
#[derive(Debug, Clone)]
pub enum RecoveryAction {
    /// Restart the agent
    Restart,
    /// Resume from saved session
    Resume { session_id: String },
    /// Spawn a replacement agent
    Spawn { replacement_id: AgentId },
}

// ============================================================================
// Module Exports
// ============================================================================

mod retry;
mod recovery;
mod degradation;
mod consensus;
mod hitl;
mod circuit_breaker;
mod failure_tracking;

pub use retry::{RetryResilience, RetryConfig, BackoffStrategy};
pub use recovery::{RecoveryResilience, RecoveryConfig};
pub use degradation::{DegradationResilience, DegradationConfig, FallbackStrategy};
pub use consensus::{ConsensusResilience, ConsensusConfig, VotingProtocol, Vote};
pub use hitl::{HitlResilience, HitlConfig, HitlRequest, Severity};
pub use circuit_breaker::{CircuitBreakerResilience, CircuitBreakerConfig, CircuitState};
pub use failure_tracking::{FailureTrackingResilience, FailureTrackingConfig, FailureCategory};

// ============================================================================
// Helper Functions
// ============================================================================

/// Convert AgentId to a string key for HashMap storage (since AgentId doesn't implement Hash).
pub fn agent_key(agent_id: &AgentId) -> String {
    match agent_id {
        AgentId::Nydus(id) => format!("nydus:{}", id.0),
        AgentId::Queen(id) => format!("queen:{}", id.0),
        AgentId::Overlord(id) => format!("overlord:{}", id),
        AgentId::Overmind(id) => format!("overmind:{}", id),
        AgentId::Validator => "validator".to_string(),
        AgentId::Operator => "operator".to_string(),
    }
}

/// Convert TaskId to a string key.
pub fn task_key(task_id: &TaskId) -> String {
    task_id.0.clone()
}
