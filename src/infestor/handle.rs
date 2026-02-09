//! InfestorHandle: Cloneable handle for the Infestor actor.
//!
//! This module provides the handle to communicate with Infestor agents, along with
//! command and event enums for the actor protocol.

use crate::core::types::{QueenId, InfestorId};
use tokio::sync::{mpsc, watch};
use std::path::PathBuf;

// ============================================================================
// Command Enum
// ============================================================================

/// Commands sent to an Infestor actor.
#[derive(Debug, Clone)]
pub enum InfestorCommand {
    /// Review a Queen's completed task branch.
    Review {
        queen_id: QueenId,
        task_id: String,
        branch_name: String,
        worktree_path: PathBuf,
    },
    /// Request graceful shutdown.
    Shutdown,
}

// ============================================================================
// Status Enum
// ============================================================================

/// Current status of an Infestor.
#[derive(Debug, Clone, PartialEq)]
pub enum InfestorStatus {
    /// Infestor is idle and ready to review.
    Idle,
    /// Infestor is actively reviewing a task.
    Reviewing { queen_id: QueenId, task_id: String },
    /// Infestor is dead.
    Dead,
}

// ============================================================================
// Event Enum
// ============================================================================

/// Events emitted by an Infestor actor.
#[derive(Debug, Clone)]
pub enum InfestorEvent {
    /// Review passed — safe to merge.
    ReviewApproved {
        queen_id: QueenId,
        task_id: String,
        summary: String,
    },
    /// Review failed — notify operator.
    ReviewRejected {
        queen_id: QueenId,
        task_id: String,
        reason: String,
    },
    /// Infestor status changed.
    StatusChanged(InfestorStatus),
    /// Infestor subprocess died unexpectedly.
    ProcessDied(String),
}

// ============================================================================
// InfestorHandle (cloneable handle to the Infestor actor)
// ============================================================================

/// Cloneable handle to communicate with an Infestor actor.
///
/// Uses tokio mpsc channels for command sending and watch channel for status.
#[derive(Clone)]
pub struct InfestorHandle {
    /// Unique identifier for this Infestor.
    pub id: InfestorId,
    /// Command sender (mpsc allows multiple senders).
    pub cmd_tx: mpsc::Sender<InfestorCommand>,
    /// Status receiver (watch allows multiple receivers).
    pub status_rx: watch::Receiver<InfestorStatus>,
}

impl InfestorHandle {
    /// Request a review of a Queen's completed task.
    ///
    /// # Errors
    /// Returns an error if the Infestor's command channel is closed.
    pub fn review(
        &self,
        queen_id: QueenId,
        task_id: String,
        branch_name: String,
        worktree_path: PathBuf,
    ) -> Result<(), anyhow::Error> {
        self.cmd_tx
            .try_send(InfestorCommand::Review {
                queen_id,
                task_id,
                branch_name,
                worktree_path,
            })
            .map_err(|e| anyhow::anyhow!("Failed to send review command: {}", e))
    }

    /// Request graceful shutdown of this Infestor.
    ///
    /// # Errors
    /// Returns an error if the Infestor's command channel is closed.
    pub fn shutdown(&self) -> Result<(), anyhow::Error> {
        self.cmd_tx
            .try_send(InfestorCommand::Shutdown)
            .map_err(|e| anyhow::anyhow!("Failed to send shutdown: {}", e))
    }

    /// Get the current status of this Infestor.
    pub fn status(&self) -> InfestorStatus {
        self.status_rx.borrow().clone()
    }
}
