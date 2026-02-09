//! InfestorHandle: Cloneable handle for the Infestor actor.
//!
//! The Infestor is a StreamQueen with a reviewer system prompt. It wraps a QueenHandle
//! internally and provides a review() method that formats the review request as a task.

use crate::core::types::{QueenId, InfestorId, Task, TaskContext, TaskId, TaskStatus};
use crate::queen::handle::QueenHandle;
use std::path::PathBuf;
use chrono::Utc;
use std::collections::HashMap;

/// Cloneable handle to communicate with an Infestor actor.
///
/// The Infestor is a StreamQueen with a reviewer role. It reviews Queen-completed
/// tasks by examining diffs and running cargo check before merge decisions.
#[derive(Clone)]
pub struct InfestorHandle {
    /// Unique identifier for this Infestor.
    pub id: InfestorId,
    /// Underlying Queen handle.
    queen_handle: QueenHandle,
}

impl InfestorHandle {
    /// Create an InfestorHandle from a QueenHandle.
    pub fn from_queen_handle(id: InfestorId, queen_handle: QueenHandle) -> Self {
        Self { id, queen_handle }
    }

    /// Send a review task to the Infestor.
    ///
    /// Builds the diff + review prompt and sends as a normal task assignment.
    ///
    /// # Errors
    /// Returns an error if the Infestor's command channel is closed.
    pub async fn review(
        &self,
        queen_id: QueenId,
        task_id: String,
        branch_name: String,
        worktree_path: PathBuf,
    ) -> Result<(), anyhow::Error> {
        // Build review prompt with instructions
        let review_description = format!(
            "## Review Request\n\n\
            **Queen**: {queen_id}\n\
            **Task**: {task_id}\n\
            **Branch**: {branch_name}\n\
            **Worktree**: {worktree_path}\n\n\
            ## Instructions\n\n\
            1. Run `git diff HEAD~1..HEAD` in the worktree path to see the changes\n\
            2. Run `cargo check --workspace` to verify compilation\n\
            3. Review the diff for correctness, scope, and quality\n\
            4. Respond with your verdict\n\n\
            ## Response Format\n\n\
            You MUST end your response with exactly one of:\n\n\
            VERDICT: APPROVE\n\
            <summary>Brief description of changes</summary>\n\n\
            OR\n\n\
            VERDICT: REJECT\n\
            <reason>What's wrong and needs fixing</reason>",
            queen_id = queen_id.0,
            task_id = task_id,
            branch_name = branch_name,
            worktree_path = worktree_path.display(),
        );

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

    /// Request graceful shutdown of this Infestor.
    ///
    /// # Errors
    /// Returns an error if the Infestor's command channel is closed.
    pub async fn shutdown(&self) -> Result<(), anyhow::Error> {
        self.queen_handle.shutdown().await
    }

    /// Get the current status of this Infestor.
    pub fn status(&self) -> crate::core::types::QueenStatus {
        self.queen_handle.status()
    }

    /// Check if the Infestor subprocess is still alive.
    pub fn is_alive(&self) -> bool {
        self.queen_handle.is_alive()
    }
}
