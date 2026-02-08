//! QueenHandle: Cloneable front-half of the Queen actor pattern.
//!
//! This module provides the handle to communicate with Queen agents, along with
//! command and event enums for the actor protocol.

use crate::core::types::{QueenId, QueenStatus, SwarmMessage, Task, TaskContext, TaskId};
use crate::queen::spawn_mode::SpawnMode;
use anyhow::{Context, Result};
use tokio::sync::{mpsc, watch};

// ============================================================================
// Command Enum (messages sent TO the Queen)
// ============================================================================

/// Commands sent to a Queen actor.
#[derive(Debug, Clone)]
pub enum QueenCommand {
    /// Assign a task from SwarmHost with context.
    Assign {
        task: Task,
        context: TaskContext,
    },
    /// Send a message from another agent (Queen-to-Queen communication).
    Message(SwarmMessage),
    /// Request graceful shutdown.
    Shutdown,
}

// ============================================================================
// Event Enum (messages sent FROM the Queen)
// ============================================================================

/// Events emitted by a Queen actor.
#[derive(Debug, Clone)]
pub enum QueenEvent {
    /// Task completed successfully.
    TaskCompleted {
        queen_id: QueenId,
        task_id: TaskId,
        result_text: String,
        cost_usd: f64,
        duration_ms: u64,
        num_turns: u32,
        session_id: Option<String>,
        quality_passed: bool,
    },
    /// Task failed with an error.
    TaskFailed {
        queen_id: QueenId,
        task_id: TaskId,
        error: String,
        cost_usd: f64,
        num_turns: u32,
    },
    /// Progress update on an in-progress task.
    Progress {
        queen_id: QueenId,
        task_id: TaskId,
        turns_completed: u32,
        cost_usd: f64,
    },
    /// Knowledge sharing from the Queen.
    Knowledge {
        queen_id: QueenId,
        key: String,
        value: serde_json::Value,
    },
    /// Queen subprocess died unexpectedly.
    ProcessDied {
        queen_id: QueenId,
        exit_code: Option<i32>,
        session_id: Option<String>,
    },
    /// Queen status changed.
    StatusChanged {
        queen_id: QueenId,
        status: QueenStatus,
    },
    /// Context was compressed by Claude Code.
    ContextCompressed {
        queen_id: QueenId,
        pre_tokens: u64,
        trigger: String,
    },
    /// Messages received from mailbox (informational).
    MessagesReceived {
        queen_id: QueenId,
        count: usize,
    },
}

// ============================================================================
// QueenHandle (cloneable handle to the Queen actor)
// ============================================================================

/// Cloneable handle to communicate with a Queen actor.
///
/// Uses tokio mpsc channels for command sending and watch channel for status.
/// All channels are cloneable, so this handle can be safely cloned and shared
/// across tasks.
#[derive(Debug, Clone)]
pub struct QueenHandle {
    /// Unique identifier for this Queen.
    id: QueenId,
    /// Spawn mode for this Queen.
    spawn_mode: SpawnMode,
    /// Command sender (mpsc allows multiple senders).
    cmd_tx: mpsc::Sender<QueenCommand>,
    /// Status receiver (watch allows multiple receivers).
    status_rx: watch::Receiver<QueenStatus>,
}

impl QueenHandle {
    /// Create a new QueenHandle with the given channels.
    ///
    /// # Arguments
    /// * `id` - Unique identifier for the Queen
    /// * `spawn_mode` - How this Queen spawns Claude Code processes
    /// * `cmd_tx` - Command channel sender
    /// * `status_rx` - Status watch channel receiver
    pub fn new(
        id: QueenId,
        spawn_mode: SpawnMode,
        cmd_tx: mpsc::Sender<QueenCommand>,
        status_rx: watch::Receiver<QueenStatus>,
    ) -> Self {
        Self {
            id,
            spawn_mode,
            cmd_tx,
            status_rx,
        }
    }

    /// Get the Queen ID.
    pub fn id(&self) -> &QueenId {
        &self.id
    }

    /// Get the spawn mode.
    pub fn spawn_mode(&self) -> SpawnMode {
        self.spawn_mode
    }

    /// Assign a task to this Queen.
    ///
    /// # Errors
    /// Returns an error if the Queen's command channel is closed.
    pub async fn assign(&self, task: Task, context: TaskContext) -> Result<()> {
        self.cmd_tx
            .send(QueenCommand::Assign { task, context })
            .await
            .context("Failed to send Assign command: Queen channel closed")?;
        Ok(())
    }

    /// Send a message to this Queen from another agent.
    ///
    /// # Errors
    /// Returns an error if the Queen's command channel is closed.
    pub async fn send_message(&self, message: SwarmMessage) -> Result<()> {
        self.cmd_tx
            .send(QueenCommand::Message(message))
            .await
            .context("Failed to send Message command: Queen channel closed")?;
        Ok(())
    }

    /// Get the current status of this Queen.
    ///
    /// This is a non-blocking read of the watch channel's current value.
    pub fn status(&self) -> QueenStatus {
        self.status_rx.borrow().clone()
    }

    /// Subscribe to status changes.
    ///
    /// Returns a new watch receiver that can be used to track status updates.
    pub fn subscribe_status(&self) -> watch::Receiver<QueenStatus> {
        self.status_rx.clone()
    }

    /// Request graceful shutdown of this Queen.
    ///
    /// # Errors
    /// Returns an error if the Queen's command channel is closed.
    pub async fn shutdown(&self) -> Result<()> {
        self.cmd_tx
            .send(QueenCommand::Shutdown)
            .await
            .context("Failed to send Shutdown command: Queen channel closed")?;
        Ok(())
    }

    /// Check if this Queen is still alive (command channel is open).
    ///
    /// Returns false if the Queen actor has dropped its receiver.
    pub fn is_alive(&self) -> bool {
        !self.cmd_tx.is_closed()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::types::{AgentId, MessageType, Visibility};
    use chrono::Utc;
    use std::collections::HashMap;

    #[tokio::test]
    async fn test_handle_creation() {
        let (cmd_tx, _cmd_rx) = mpsc::channel(10);
        let (status_tx, status_rx) = watch::channel(QueenStatus::Idle);

        let handle = QueenHandle::new(
            QueenId("Q0".to_string()),
            SpawnMode::PerTask,
            cmd_tx,
            status_rx,
        );

        assert_eq!(handle.id().0, "Q0");
        assert_eq!(handle.spawn_mode(), SpawnMode::PerTask);
        assert!(handle.is_alive());

        // Drop the status sender to avoid unused variable warning
        drop(status_tx);
    }

    #[tokio::test]
    async fn test_assign_command() {
        let (cmd_tx, mut cmd_rx) = mpsc::channel(10);
        let (_status_tx, status_rx) = watch::channel(QueenStatus::Idle);

        let handle = QueenHandle::new(
            QueenId("Q1".to_string()),
            SpawnMode::Stream,
            cmd_tx,
            status_rx,
        );

        let task = Task {
            id: TaskId("T1".to_string()),
            description: "Test task".to_string(),
            status: crate::core::types::TaskStatus::Ready,
            assigned_to: None,
            priority: 100,
            blocked_by: vec![],
            created_at: Utc::now(),
        };

        let context = TaskContext {
            knowledge: HashMap::new(),
            recent_messages: vec![],
            shared_state: HashMap::new(),
            skill_hint: None,
            knowledge_entries: vec![],
        };

        // Send assign command
        handle.assign(task.clone(), context.clone()).await.unwrap();

        // Verify command was received
        let cmd = cmd_rx.recv().await.unwrap();
        match cmd {
            QueenCommand::Assign {
                task: recv_task,
                context: recv_context,
            } => {
                assert_eq!(recv_task.id.0, "T1");
                assert_eq!(recv_task.description, "Test task");
                assert!(recv_context.knowledge.is_empty());
            }
            _ => panic!("Expected Assign command"),
        }
    }

    #[tokio::test]
    async fn test_send_message_command() {
        let (cmd_tx, mut cmd_rx) = mpsc::channel(10);
        let (_status_tx, status_rx) = watch::channel(QueenStatus::Idle);

        let handle = QueenHandle::new(
            QueenId("Q2".to_string()),
            SpawnMode::PerTask,
            cmd_tx,
            status_rx,
        );

        let message = SwarmMessage {
            id: "msg-123".to_string(),
            from: AgentId::Queen(QueenId("Q0".to_string())),
            to: AgentId::Queen(QueenId("Q2".to_string())),
            msg_type: MessageType::StatusRequest,
            payload: serde_json::json!({}),
            timestamp: Utc::now(),
            correlation_id: None,
            visibility: Visibility::default_internal(),
        };

        // Send message command
        handle.send_message(message.clone()).await.unwrap();

        // Verify command was received
        let cmd = cmd_rx.recv().await.unwrap();
        match cmd {
            QueenCommand::Message(recv_msg) => {
                assert_eq!(recv_msg.id, "msg-123");
                assert!(matches!(recv_msg.msg_type, MessageType::StatusRequest));
            }
            _ => panic!("Expected Message command"),
        }
    }

    #[tokio::test]
    async fn test_shutdown_command() {
        let (cmd_tx, mut cmd_rx) = mpsc::channel(10);
        let (_status_tx, status_rx) = watch::channel(QueenStatus::Idle);

        let handle = QueenHandle::new(
            QueenId("Q3".to_string()),
            SpawnMode::Stream,
            cmd_tx,
            status_rx,
        );

        // Send shutdown command
        handle.shutdown().await.unwrap();

        // Verify command was received
        let cmd = cmd_rx.recv().await.unwrap();
        assert!(matches!(cmd, QueenCommand::Shutdown));
    }

    #[tokio::test]
    async fn test_status_read() {
        let (cmd_tx, _cmd_rx) = mpsc::channel(10);
        let (status_tx, status_rx) = watch::channel(QueenStatus::Idle);

        let handle = QueenHandle::new(
            QueenId("Q4".to_string()),
            SpawnMode::PerTask,
            cmd_tx,
            status_rx,
        );

        // Initial status
        assert!(matches!(handle.status(), QueenStatus::Idle));

        // Update status
        status_tx
            .send(QueenStatus::Working {
                task_id: TaskId("T2".to_string()),
                progress: 0.5,
                sub_tasks: vec![],
            })
            .unwrap();

        // Read updated status
        let status = handle.status();
        match status {
            QueenStatus::Working { task_id, .. } => {
                assert_eq!(task_id.0, "T2");
            }
            _ => panic!("Expected Working status"),
        }
    }

    #[tokio::test]
    async fn test_is_alive() {
        let (cmd_tx, cmd_rx) = mpsc::channel(10);
        let (_status_tx, status_rx) = watch::channel(QueenStatus::Idle);

        let handle = QueenHandle::new(
            QueenId("Q5".to_string()),
            SpawnMode::Stream,
            cmd_tx,
            status_rx,
        );

        // Initially alive
        assert!(handle.is_alive());

        // Drop the receiver to close the channel
        drop(cmd_rx);

        // Now dead
        assert!(!handle.is_alive());
    }

    #[tokio::test]
    async fn test_handle_clone() {
        let (cmd_tx, mut cmd_rx) = mpsc::channel(10);
        let (_status_tx, status_rx) = watch::channel(QueenStatus::Idle);

        let handle1 = QueenHandle::new(
            QueenId("Q6".to_string()),
            SpawnMode::PerTask,
            cmd_tx,
            status_rx,
        );

        // Clone the handle
        let handle2 = handle1.clone();

        // Both handles should work
        assert_eq!(handle1.id().0, handle2.id().0);
        assert!(handle1.is_alive());
        assert!(handle2.is_alive());

        // Send from cloned handle
        handle2.shutdown().await.unwrap();

        // Original handle should receive
        let cmd = cmd_rx.recv().await.unwrap();
        assert!(matches!(cmd, QueenCommand::Shutdown));
    }

    #[tokio::test]
    async fn test_multiple_handles() {
        let (cmd_tx, mut cmd_rx) = mpsc::channel(10);
        let (_status_tx, status_rx) = watch::channel(QueenStatus::Idle);

        let handle1 = QueenHandle::new(
            QueenId("Q7".to_string()),
            SpawnMode::Stream,
            cmd_tx,
            status_rx,
        );

        let handle2 = handle1.clone();
        let handle3 = handle1.clone();

        // All handles send commands
        handle1.shutdown().await.unwrap();
        handle2.shutdown().await.unwrap();
        handle3.shutdown().await.unwrap();

        // All commands received
        assert!(matches!(cmd_rx.recv().await.unwrap(), QueenCommand::Shutdown));
        assert!(matches!(cmd_rx.recv().await.unwrap(), QueenCommand::Shutdown));
        assert!(matches!(cmd_rx.recv().await.unwrap(), QueenCommand::Shutdown));
    }

    #[test]
    fn test_context_compressed_event() {
        let event = QueenEvent::ContextCompressed {
            queen_id: QueenId("Q0".to_string()),
            pre_tokens: 150000,
            trigger: "automatic".to_string(),
        };

        // Verify event can be matched
        match event {
            QueenEvent::ContextCompressed {
                queen_id,
                pre_tokens,
                trigger,
            } => {
                assert_eq!(queen_id.0, "Q0");
                assert_eq!(pre_tokens, 150000);
                assert_eq!(trigger, "automatic");
            }
            _ => panic!("Expected ContextCompressed event"),
        }
    }

    #[test]
    fn test_messages_received_event() {
        let event = QueenEvent::MessagesReceived {
            queen_id: QueenId("Q0".to_string()),
            count: 3,
        };

        // Verify event can be constructed and matched
        match event {
            QueenEvent::MessagesReceived { queen_id, count } => {
                assert_eq!(queen_id.0, "Q0");
                assert_eq!(count, 3);
            }
            _ => panic!("Expected MessagesReceived event"),
        }
    }
}
