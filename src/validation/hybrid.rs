//! Hybrid validation — Rust parsers + deterministic checks + optional LLM review.
//!
//! This wraps the existing overlord::verdict::run_hybrid_review() as a Validation trait.

use super::{Validation, ValidationReport, ValidationRequest, ValidationVerdict};
use crate::core::types::TaskId;
use anyhow::Result;
use async_trait::async_trait;

/// Hybrid validation strategy using Rust parsers and deterministic code checks.
///
/// Phase 1: Parse diff summary (git diff --numstat)
/// Phase 2: Run verification command (if provided)
/// Phase 3: Scan code quality (quality issues in diff)
/// Phase 4: Deterministic verdict (auto-approve / auto-reject / needs-review)
pub struct HybridValidation {
    /// Whether to auto-approve when all checks pass.
    pub auto_approve: bool,
    /// Maximum allowed quality issues before rejection.
    pub max_quality_issues: usize,
    /// Whether to require verification command to pass.
    pub require_verify: bool,
}

impl Default for HybridValidation {
    fn default() -> Self {
        Self {
            auto_approve: true,
            max_quality_issues: 5,
            require_verify: true,
        }
    }
}

impl HybridValidation {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_auto_approve(mut self, auto: bool) -> Self {
        self.auto_approve = auto;
        self
    }

    pub fn with_max_quality_issues(mut self, max: usize) -> Self {
        self.max_quality_issues = max;
        self
    }

    pub fn with_require_verify(mut self, require: bool) -> Self {
        self.require_verify = require;
        self
    }
}

#[async_trait]
impl Validation for HybridValidation {
    async fn validate(&mut self, request: ValidationRequest) -> Result<ValidationReport> {
        // Use the existing hybrid review pipeline from overlord::verdict
        let review_result = crate::overlord::verdict::run_hybrid_review(
            &request.worktree_path,
            &request.base_branch,
            request.verify_cmd.as_deref(),
            &request.task_description,
            request.duration_secs,
            request.cost_usd,
            0, // turns not tracked at this level
        )
        .await?;

        let verdict = match review_result.verdict {
            crate::overlord::verdict::OverlordVerdict::Approve => ValidationVerdict::Approve,
            crate::overlord::verdict::OverlordVerdict::Reject { reason } => {
                ValidationVerdict::Reject { reason }
            }
        };

        Ok(ValidationReport {
            verdict,
            task_id: request.task_id,
            agent_id: request.agent_id,
            files_changed: review_result
                .diff
                .files
                .iter()
                .map(|f| f.path.clone())
                .collect(),
            lines_added: review_result.diff.total_added,
            lines_removed: review_result.diff.total_removed,
            tests_passed: review_result.tests.as_ref().map(|t| t.failed == 0),
            quality_score: Some(
                1.0 - (review_result.quality.total_hits as f64
                    / self.max_quality_issues.max(1) as f64)
                    .min(1.0),
            ),
            details: format!(
                "Files: {} changed, +{} -{}. Quality issues: {}. Code check: {:?}",
                review_result.diff.files.len(),
                review_result.diff.total_added,
                review_result.diff.total_removed,
                review_result.quality.total_hits,
                review_result.code_check_verdict,
            ),
        })
    }

    fn can_validate(&self, _task_id: &TaskId) -> bool {
        true // Can validate any task that has a worktree
    }

    fn name(&self) -> &str {
        "hybrid"
    }
}
