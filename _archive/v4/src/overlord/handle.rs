//! OverlordHandle: Cloneable handle for the Overlord actor.
//!
//! The Overlord is a StreamQueen with a reviewer system prompt. It wraps a QueenHandle
//! internally and provides a review() method that formats the review request as a task.

use crate::core::types::{QueenId, OverlordId, Task, TaskContext, TaskId, TaskStatus};
use crate::queen::handle::QueenHandle;
use std::path::PathBuf;
use chrono::Utc;
use std::collections::HashMap;

/// Cloneable handle to communicate with an Overlord actor.
///
/// The Overlord is a StreamQueen with a reviewer role. It reviews Queen-completed
/// tasks by examining diffs and running cargo check before merge decisions.
#[derive(Clone)]
pub struct OverlordHandle {
    /// Unique identifier for this Overlord.
    pub id: OverlordId,
    /// Underlying Queen handle.
    queen_handle: QueenHandle,
}

impl OverlordHandle {
    /// Create an OverlordHandle from a QueenHandle.
    pub fn from_queen_handle(id: OverlordId, queen_handle: QueenHandle) -> Self {
        Self { id, queen_handle }
    }

    /// Send a review task to the Overlord.
    ///
    /// Builds the diff + review prompt and sends as a normal task assignment.
    ///
    /// # Errors
    /// Returns an error if the Overlord's command channel is closed.
    pub async fn review(
        &self,
        queen_id: QueenId,
        task_id: String,
        branch_name: String,
        worktree_path: PathBuf,
        task_description: &str,
        verify_cmd: Option<&str>,
    ) -> Result<(), anyhow::Error> {
        // Build verification command section
        let verify_section = if let Some(cmd) = verify_cmd {
            format!(
                "## Verification Command\n\n\
                The task requires this verification command to pass:\n\
                ```\n{}\n```\n\n\
                You MUST run this command and it MUST pass for approval.\n\n",
                cmd
            )
        } else {
            "## Verification Command\n\nNo verification command specified for this task.\n\n".to_string()
        };

        // Build review prompt with instructions using template
        const REVIEW_TEMPLATE: &str = include_str!("prompts/review_task.md");
        let review_description = REVIEW_TEMPLATE
            .replace("{queen_id}", &queen_id.0)
            .replace("{task_id}", &task_id)
            .replace("{branch_name}", &branch_name)
            .replace("{worktree_path}", &worktree_path.display().to_string())
            .replace("{task_description}", task_description)
            .replace("{verify_section}", &verify_section);

        let task = Task {
            id: TaskId(format!("review-{}-{}", queen_id.0, task_id)),
            description: review_description,
            status: TaskStatus::Assigned,
            assigned_to: Some(QueenId(self.id.0.clone())),
            priority: 200, // High priority for reviews
            blocked_by: vec![],
            created_at: Utc::now(),
        };

        let context = TaskContext {
            knowledge: HashMap::new(),
            recent_messages: vec![],
            shared_state: HashMap::new(),
            skill_hint: None,
            knowledge_entries: vec![],
            other_tasks_summary: None,
            rejection_feedback: None,
        };

        self.queen_handle.assign(task, context).await
    }

    /// Request graceful shutdown of this Overlord.
    ///
    /// # Errors
    /// Returns an error if the Overlord's command channel is closed.
    pub async fn shutdown(&self) -> Result<(), anyhow::Error> {
        self.queen_handle.shutdown().await
    }

    /// Get the current status of this Overlord.
    pub fn status(&self) -> crate::core::types::QueenStatus {
        self.queen_handle.status()
    }

    /// Check if the Overlord subprocess is still alive.
    pub fn is_alive(&self) -> bool {
        self.queen_handle.is_alive()
    }
}
