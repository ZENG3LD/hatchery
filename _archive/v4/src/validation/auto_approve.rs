//! Auto-approve validation — always approves tasks without review.

use super::{Validation, ValidationReport, ValidationRequest, ValidationVerdict};
use crate::core::types::TaskId;
use anyhow::Result;
use async_trait::async_trait;

/// Auto-approve validation strategy. Approves everything without review.
/// Useful for testing or trusted single-agent pipelines.
pub struct AutoApproveValidation;

impl AutoApproveValidation {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Validation for AutoApproveValidation {
    async fn validate(&mut self, request: ValidationRequest) -> Result<ValidationReport> {
        Ok(ValidationReport {
            verdict: ValidationVerdict::Approve,
            task_id: request.task_id,
            agent_id: request.agent_id,
            files_changed: vec![],
            lines_added: 0,
            lines_removed: 0,
            tests_passed: None,
            quality_score: None,
            details: "Auto-approved without review".to_string(),
        })
    }

    fn can_validate(&self, _task_id: &TaskId) -> bool {
        true
    }

    fn name(&self) -> &str {
        "auto-approve"
    }
}
