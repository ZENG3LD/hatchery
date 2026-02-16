//! Validation trait — abstracts how task output is validated before merge.
//!
//! Overlord is one implementation. This trait allows swapping between:
//! - Hybrid validation (Rust parsers + deterministic checks)
//! - Auto-approve (for testing/trusted pipelines)
//! - LLM-based validation (for complex code review)
//! - Custom validation strategies

use crate::core::types::{AgentId, TaskId};
use anyhow::Result;
use async_trait::async_trait;
use std::path::PathBuf;

pub mod auto_approve;
pub mod hybrid;

/// Result of a validation check.
#[derive(Debug, Clone, PartialEq)]
pub enum ValidationVerdict {
    /// Approved for merge.
    Approve,
    /// Rejected with reason.
    Reject { reason: String },
    /// Needs human review.
    NeedsReview { details: String },
}

/// Summary of what was validated.
#[derive(Debug, Clone)]
pub struct ValidationReport {
    pub verdict: ValidationVerdict,
    pub task_id: TaskId,
    pub agent_id: AgentId,
    pub files_changed: Vec<String>,
    pub lines_added: usize,
    pub lines_removed: usize,
    pub tests_passed: Option<bool>,
    pub quality_score: Option<f64>,
    pub details: String,
}

/// Configuration for a validation request.
#[derive(Debug, Clone)]
pub struct ValidationRequest {
    pub task_id: TaskId,
    pub agent_id: AgentId,
    pub worktree_path: PathBuf,
    pub base_branch: String,
    pub task_description: String,
    pub verify_cmd: Option<String>,
    pub duration_secs: f64,
    pub cost_usd: f64,
}

/// Validation defines how task output is validated before merge.
#[async_trait]
pub trait Validation: Send + Sync {
    /// Validate a completed task's output.
    async fn validate(&mut self, request: ValidationRequest) -> Result<ValidationReport>;

    /// Check if this validator can handle the given task type.
    fn can_validate(&self, task_id: &TaskId) -> bool;

    /// Get the name of this validation strategy.
    fn name(&self) -> &str;
}
