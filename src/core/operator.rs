//! Operator communication channel for Hatchery V2.
//!
//! This module provides the bidirectional communication layer between Nydus
//! and the human operator, enabling progress updates, escalations, and commands.

use anyhow::Result;
use async_trait::async_trait;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::Arc;

use crate::core::types::{AgentId, Severity, NydusId};

// ============================================================================
// Events (Up: Nydus → Operator)
// ============================================================================

/// Events emitted from Nydus to Operator.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum OperatorEvent {
    /// Progress update for a specific Nydus
    #[serde(rename = "progress")]
    Progress {
        swarm_id: NydusId,
        done: usize,
        total: usize,
    },
    /// Global progress across all Nydus nodes
    #[serde(rename = "global_progress")]
    GlobalProgress {
        done: usize,
        total: usize,
        elapsed_secs: u64,
    },
    /// Escalation from an agent requiring attention
    #[serde(rename = "escalation")]
    Escalation {
        source: AgentId,
        issue: String,
        severity: Severity,
    },
    /// Question requiring operator input
    #[serde(rename = "question")]
    Question {
        id: String,
        text: String,
        options: Vec<String>,
    },
    /// A Nydus completed its work
    #[serde(rename = "swarm_completed")]
    SwarmCompleted {
        swarm_id: NydusId,
        tasks_done: usize,
        tasks_total: usize,
        duration_secs: u64,
    },
    /// All Nydus nodes are done
    #[serde(rename = "all_complete")]
    AllComplete {
        total_tasks: usize,
        total_duration_secs: u64,
    },
    /// Error from an agent
    #[serde(rename = "error")]
    Error { source: AgentId, error: String },
    /// Log message (for verbose output)
    #[serde(rename = "log")]
    Log { level: LogLevel, message: String },
}

/// Log level for operator messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

// ============================================================================
// Commands (Down: Operator → Nydus)
// ============================================================================

/// Commands from Operator to Nydus.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum OperatorCommand {
    /// Change priority of a Nydus node
    #[serde(rename = "reprioritize")]
    Reprioritize {
        swarm_id: NydusId,
        priority: u8,
    },
    /// Cancel a Nydus node
    #[serde(rename = "cancel")]
    Cancel { swarm_id: NydusId },
    /// Answer a question
    #[serde(rename = "answer")]
    Answer {
        question_id: String,
        answer: String,
    },
    /// Send message to a specific agent
    #[serde(rename = "message")]
    Message { target: AgentId, text: String },
    /// Add/remove Queens from a Nydus node
    #[serde(rename = "scale")]
    Scale {
        swarm_id: NydusId,
        queens: usize,
    },
    /// Shutdown everything
    #[serde(rename = "shutdown_all")]
    ShutdownAll,
}

// ============================================================================
// OperatorChannel Trait
// ============================================================================

/// Bidirectional communication channel between Nydus and Operator.
#[async_trait]
pub trait OperatorChannel: Send + Sync {
    /// Emit an event to the operator.
    async fn emit(&self, event: OperatorEvent) -> Result<()>;

    /// Try to receive a command from the operator (non-blocking).
    /// Returns None if no command is available.
    async fn try_recv(&mut self) -> Option<OperatorCommand>;

    /// Check if the operator channel is connected/active.
    fn is_connected(&self) -> bool;
}

// ============================================================================
// StdoutChannel Implementation
// ============================================================================

/// Channel that emits events to stdout and receives commands from stdin.
///
/// Events are printed as `@operator:{json}` lines.
/// Commands are read as JSON lines from stdin.
pub struct StdoutChannel {
    /// Receiver for commands (stdin is read in a background thread)
    cmd_rx: tokio::sync::mpsc::UnboundedReceiver<OperatorCommand>,
    /// Whether we've started the stdin reader
    connected: bool,
}

impl StdoutChannel {
    /// Create a new StdoutChannel.
    /// Spawns a background tokio task to read commands from stdin.
    pub fn new() -> Self {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();

        // Spawn stdin reader task
        tokio::spawn(async move {
            use tokio::io::{AsyncBufReadExt, BufReader};
            let stdin = tokio::io::stdin();
            let reader = BufReader::new(stdin);
            let mut lines = reader.lines();

            while let Ok(Some(line)) = lines.next_line().await {
                if let Ok(cmd) = serde_json::from_str::<OperatorCommand>(&line) {
                    if tx.send(cmd).is_err() {
                        break;
                    }
                }
            }
        });

        Self {
            cmd_rx: rx,
            connected: true,
        }
    }
}

#[async_trait]
impl OperatorChannel for StdoutChannel {
    async fn emit(&self, event: OperatorEvent) -> Result<()> {
        let json = serde_json::to_string(&event)?;
        println!("@operator:{}", json);
        Ok(())
    }

    async fn try_recv(&mut self) -> Option<OperatorCommand> {
        self.cmd_rx.try_recv().ok()
    }

    fn is_connected(&self) -> bool {
        self.connected
    }
}

impl Default for StdoutChannel {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// NullChannel Implementation (for testing)
// ============================================================================

/// No-op channel for testing.
///
/// Commands can be pre-loaded via `push_command()`, and emitted events
/// can be inspected via `emitted_events()`.
pub struct NullChannel {
    /// Pre-loaded commands (for testing)
    commands: Arc<Mutex<VecDeque<OperatorCommand>>>,
    /// Collected events (for assertions in tests)
    events: Arc<Mutex<Vec<OperatorEvent>>>,
}

impl NullChannel {
    /// Create a new NullChannel.
    pub fn new() -> Self {
        Self {
            commands: Arc::new(Mutex::new(VecDeque::new())),
            events: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Push a command to be received later (for testing).
    pub fn push_command(&self, cmd: OperatorCommand) {
        self.commands.lock().push_back(cmd);
    }

    /// Get all emitted events (for assertions).
    pub fn emitted_events(&self) -> Vec<OperatorEvent> {
        self.events.lock().clone()
    }
}

#[async_trait]
impl OperatorChannel for NullChannel {
    async fn emit(&self, event: OperatorEvent) -> Result<()> {
        self.events.lock().push(event);
        Ok(())
    }

    async fn try_recv(&mut self) -> Option<OperatorCommand> {
        self.commands.lock().pop_front()
    }

    fn is_connected(&self) -> bool {
        true
    }
}

impl Default for NullChannel {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// PipeChannel Implementation (stub for nested Nydus)
// ============================================================================

/// Placeholder for nested Nydus communication.
///
/// Will be implemented in Phase 6 when full nesting is needed.
pub struct PipeChannel;

impl PipeChannel {
    /// Create a new PipeChannel (currently a stub).
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl OperatorChannel for PipeChannel {
    async fn emit(&self, _event: OperatorEvent) -> Result<()> {
        Ok(())
    }

    async fn try_recv(&mut self) -> Option<OperatorCommand> {
        None
    }

    fn is_connected(&self) -> bool {
        false
    }
}

impl Default for PipeChannel {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_null_channel_emit_and_collect_events() {
        let channel = NullChannel::new();

        let event1 = OperatorEvent::Progress {
            swarm_id: NydusId("swarm1".to_string()),
            done: 5,
            total: 10,
        };
        let event2 = OperatorEvent::GlobalProgress {
            done: 50,
            total: 100,
            elapsed_secs: 120,
        };

        channel.emit(event1.clone()).await.unwrap();
        channel.emit(event2.clone()).await.unwrap();

        let events = channel.emitted_events();
        assert_eq!(events.len(), 2);

        // Verify the events match
        match &events[0] {
            OperatorEvent::Progress {
                swarm_id,
                done,
                total,
            } => {
                assert_eq!(swarm_id.0, "swarm1");
                assert_eq!(*done, 5);
                assert_eq!(*total, 10);
            }
            _ => panic!("Expected Progress event"),
        }

        match &events[1] {
            OperatorEvent::GlobalProgress {
                done,
                total,
                elapsed_secs,
            } => {
                assert_eq!(*done, 50);
                assert_eq!(*total, 100);
                assert_eq!(*elapsed_secs, 120);
            }
            _ => panic!("Expected GlobalProgress event"),
        }
    }

    #[tokio::test]
    async fn test_null_channel_push_command_and_try_recv() {
        let mut channel = NullChannel::new();

        let cmd1 = OperatorCommand::Cancel {
            swarm_id: NydusId("swarm2".to_string()),
        };
        let cmd2 = OperatorCommand::ShutdownAll;

        channel.push_command(cmd1);
        channel.push_command(cmd2);

        // Retrieve commands in order
        let received1 = channel.try_recv().await;
        assert!(received1.is_some());
        match received1.unwrap() {
            OperatorCommand::Cancel { swarm_id } => {
                assert_eq!(swarm_id.0, "swarm2");
            }
            _ => panic!("Expected Cancel command"),
        }

        let received2 = channel.try_recv().await;
        assert!(received2.is_some());
        match received2.unwrap() {
            OperatorCommand::ShutdownAll => {}
            _ => panic!("Expected ShutdownAll command"),
        }

        // No more commands
        let received3 = channel.try_recv().await;
        assert!(received3.is_none());
    }

    #[tokio::test]
    async fn test_pipe_channel_is_not_connected() {
        let channel = PipeChannel::new();
        assert!(!channel.is_connected());
    }

    #[tokio::test]
    async fn test_operator_event_serialization_roundtrip() {
        let event = OperatorEvent::Escalation {
            source: AgentId::Queen(crate::core::types::QueenId("Q0".to_string())),
            issue: "Task blocked".to_string(),
            severity: Severity::High,
        };

        let json = serde_json::to_string(&event).unwrap();
        let deserialized: OperatorEvent = serde_json::from_str(&json).unwrap();

        match deserialized {
            OperatorEvent::Escalation {
                source,
                issue,
                severity,
            } => {
                match source {
                    AgentId::Queen(qid) => assert_eq!(qid.0, "Q0"),
                    _ => panic!("Expected Queen agent ID"),
                }
                assert_eq!(issue, "Task blocked");
                match severity {
                    Severity::High => {}
                    _ => panic!("Expected High severity"),
                }
            }
            _ => panic!("Expected Escalation event"),
        }
    }

    #[tokio::test]
    async fn test_operator_command_serialization_roundtrip() {
        let command = OperatorCommand::Reprioritize {
            swarm_id: NydusId("swarm3".to_string()),
            priority: 255,
        };

        let json = serde_json::to_string(&command).unwrap();
        let deserialized: OperatorCommand = serde_json::from_str(&json).unwrap();

        match deserialized {
            OperatorCommand::Reprioritize {
                swarm_id,
                priority,
            } => {
                assert_eq!(swarm_id.0, "swarm3");
                assert_eq!(priority, 255);
            }
            _ => panic!("Expected Reprioritize command"),
        }
    }

    #[tokio::test]
    async fn test_null_channel_multiple_events_ordering() {
        let channel = NullChannel::new();

        // Emit events in specific order
        for i in 0..5 {
            let event = OperatorEvent::Log {
                level: LogLevel::Info,
                message: format!("Message {}", i),
            };
            channel.emit(event).await.unwrap();
        }

        let events = channel.emitted_events();
        assert_eq!(events.len(), 5);

        // Verify ordering
        for (i, event) in events.iter().enumerate() {
            match event {
                OperatorEvent::Log { level, message } => {
                    match level {
                        LogLevel::Info => {}
                        _ => panic!("Expected Info log level"),
                    }
                    assert_eq!(message, &format!("Message {}", i));
                }
                _ => panic!("Expected Log event"),
            }
        }
    }

    #[tokio::test]
    async fn test_null_channel_is_connected() {
        let channel = NullChannel::new();
        assert!(channel.is_connected());
    }

    #[tokio::test]
    async fn test_pipe_channel_emit_and_try_recv() {
        let mut channel = PipeChannel::new();

        // Emitting should not error
        let event = OperatorEvent::Log {
            level: LogLevel::Debug,
            message: "Test".to_string(),
        };
        channel.emit(event).await.unwrap();

        // Receiving should return None
        assert!(channel.try_recv().await.is_none());
    }
}
