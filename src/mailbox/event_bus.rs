//! EventBus — Hot-path IPC using tokio channels for V2 Queen architecture.
//!
//! This replaces SwarmMailbox for hot-path IPC. All actors (Queen, Nydus, Validator)
//! send events to a central EventBus using tokio mpsc channels. The bus optionally
//! logs events to SqliteEventLog via a fire-and-forget audit writer task.

use tokio::sync::{mpsc, broadcast};
use chrono::{DateTime, Utc};
use serde::{Serialize, Deserialize};
use crate::mailbox::event_log::SqliteEventLog;
use crate::core::types::*;
use crate::queen::handle::QueenEvent;

// ============================================================================
// AuditEntry
// ============================================================================

/// Audit log entry for fire-and-forget logging.
///
/// This is a simplified representation of an event for durable storage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    /// When the event occurred
    pub timestamp: DateTime<Utc>,
    /// Source agent ID (as string)
    pub from: String,
    /// Destination agent ID (as string)
    pub to: String,
    /// Event type (e.g., "StatusUpdate", "TaskResult")
    pub event_type: String,
    /// Event payload (flexible JSON)
    pub payload: serde_json::Value,
}

impl AuditEntry {
    /// Convert an AuditEntry to a SwarmMessage for logging to SqliteEventLog.
    pub fn to_swarm_message(&self) -> SwarmMessage {
        // Parse AgentIds from strings (fallback to Operator if parsing fails)
        let from = serde_json::from_str::<AgentId>(&format!("\"{}\"", self.from))
            .unwrap_or(AgentId::Operator);
        let to = serde_json::from_str::<AgentId>(&format!("\"{}\"", self.to))
            .unwrap_or(AgentId::Operator);

        // Map event_type string to MessageType
        let msg_type = match self.event_type.as_str() {
            "TaskCompleted" => MessageType::TaskResult,
            "TaskFailed" => MessageType::TaskResult,
            "Progress" => MessageType::TaskProgress,
            "Knowledge" => MessageType::Knowledge,
            "ProcessDied" => MessageType::Custom("ProcessDied".to_string()),
            "StatusChanged" => MessageType::StatusReport,
            "ContextCompressed" => MessageType::Custom("ContextCompressed".to_string()),
            "MessagesReceived" => MessageType::Custom("MessagesReceived".to_string()),
            _ => MessageType::Custom(self.event_type.clone()),
        };

        SwarmMessage {
            id: uuid::Uuid::new_v4().to_string(),
            from,
            to,
            msg_type,
            payload: self.payload.clone(),
            timestamp: self.timestamp,
            correlation_id: None,
            visibility: Visibility::default_internal(),
        }
    }
}

// ============================================================================
// EventBus
// ============================================================================

/// EventBus — Central event routing using tokio channels.
///
/// This replaces SwarmMailbox for hot-path IPC between actors.
/// - Actors send events via mpsc::Sender<QueenEvent>
/// - Main loop receives events via mpsc::Receiver<QueenEvent>
/// - Shutdown is broadcast to all actors
/// - Optional audit writer logs events to SqliteEventLog asynchronously
pub struct EventBus {
    /// Receiver for merged events from all actors
    event_rx: mpsc::Receiver<QueenEvent>,
    /// Sender that can be cloned and shared with actors
    event_tx: mpsc::Sender<QueenEvent>,
    /// Broadcast sender for shutdown signal
    shutdown_tx: broadcast::Sender<()>,
    /// Optional audit channel (fire-and-forget)
    audit_tx: Option<mpsc::UnboundedSender<AuditEntry>>,
}

impl EventBus {
    /// Create a new EventBus without audit logging.
    ///
    /// # Arguments
    /// - `capacity`: Bounded channel capacity for event queue
    pub fn new(capacity: usize) -> Self {
        let (event_tx, event_rx) = mpsc::channel(capacity);
        let (shutdown_tx, _) = broadcast::channel(16);

        Self {
            event_rx,
            event_tx,
            shutdown_tx,
            audit_tx: None,
        }
    }

    /// Create a new EventBus with audit logging enabled.
    ///
    /// Spawns a background task that drains audit entries and logs them to SqliteEventLog.
    ///
    /// # Arguments
    /// - `capacity`: Bounded channel capacity for event queue
    /// - `event_log`: SqliteEventLog for durable storage
    pub fn with_audit(capacity: usize, event_log: SqliteEventLog) -> Self {
        let (event_tx, event_rx) = mpsc::channel(capacity);
        let (shutdown_tx, _) = broadcast::channel(16);
        let (audit_tx, audit_rx) = mpsc::unbounded_channel();

        // Spawn audit writer task
        tokio::spawn(audit_writer(audit_rx, event_log));

        Self {
            event_rx,
            event_tx,
            shutdown_tx,
            audit_tx: Some(audit_tx),
        }
    }

    /// Get a cloned sender for sending events to the bus.
    ///
    /// This sender can be shared with actors (Queen, Nydus, Validator).
    pub fn event_sender(&self) -> mpsc::Sender<QueenEvent> {
        self.event_tx.clone()
    }

    /// Subscribe to shutdown signals.
    ///
    /// Returns a broadcast receiver that will receive a signal when shutdown() is called.
    pub fn shutdown_receiver(&self) -> broadcast::Receiver<()> {
        self.shutdown_tx.subscribe()
    }

    /// Broadcast shutdown signal to all actors.
    ///
    /// This sends a signal on the shutdown broadcast channel.
    /// Does not fail if no receivers are listening.
    pub fn shutdown(&self) {
        let _ = self.shutdown_tx.send(());
    }

    /// Fire-and-forget audit logging.
    ///
    /// Sends an audit entry to the audit writer task.
    /// If audit is not enabled, this is a no-op.
    ///
    /// # Arguments
    /// - `entry`: The audit entry to log
    pub fn audit(&self, entry: AuditEntry) {
        if let Some(tx) = &self.audit_tx {
            let _ = tx.send(entry);
        }
    }

    /// Receive the next event from the bus (async).
    ///
    /// Returns None if all event senders have been dropped.
    pub async fn recv_event(&mut self) -> Option<QueenEvent> {
        self.event_rx.recv().await
    }

    /// Helper to convert QueenEvent to AuditEntry for logging.
    ///
    /// This is used internally by the bus to log events.
    pub fn event_to_audit_entry(event: &QueenEvent) -> AuditEntry {
        let timestamp = Utc::now();

        match event {
            QueenEvent::TaskCompleted {
                queen_id,
                task_id,
                result_text,
                cost_usd,
                duration_ms,
                num_turns,
                session_id,
                quality_passed,
            } => {
                AuditEntry {
                    timestamp,
                    from: format!("Queen({})", queen_id.0),
                    to: "Nydus".to_string(),
                    event_type: "TaskCompleted".to_string(),
                    payload: serde_json::json!({
                        "task_id": task_id,
                        "result_text": result_text,
                        "cost_usd": cost_usd,
                        "duration_ms": duration_ms,
                        "num_turns": num_turns,
                        "session_id": session_id,
                        "quality_passed": quality_passed,
                    }),
                }
            }
            QueenEvent::TaskFailed {
                queen_id,
                task_id,
                error,
                cost_usd,
                num_turns,
            } => {
                AuditEntry {
                    timestamp,
                    from: format!("Queen({})", queen_id.0),
                    to: "Nydus".to_string(),
                    event_type: "TaskFailed".to_string(),
                    payload: serde_json::json!({
                        "task_id": task_id,
                        "error": error,
                        "cost_usd": cost_usd,
                        "num_turns": num_turns,
                    }),
                }
            }
            QueenEvent::Progress {
                queen_id,
                task_id,
                turns_completed,
                cost_usd,
            } => {
                AuditEntry {
                    timestamp,
                    from: format!("Queen({})", queen_id.0),
                    to: "Nydus".to_string(),
                    event_type: "Progress".to_string(),
                    payload: serde_json::json!({
                        "task_id": task_id,
                        "turns_completed": turns_completed,
                        "cost_usd": cost_usd,
                    }),
                }
            }
            QueenEvent::Knowledge {
                queen_id,
                key,
                value,
            } => {
                AuditEntry {
                    timestamp,
                    from: format!("Queen({})", queen_id.0),
                    to: "Nydus".to_string(),
                    event_type: "Knowledge".to_string(),
                    payload: serde_json::json!({
                        "key": key,
                        "value": value,
                    }),
                }
            }
            QueenEvent::ProcessDied {
                queen_id,
                exit_code,
                session_id,
            } => {
                AuditEntry {
                    timestamp,
                    from: format!("Queen({})", queen_id.0),
                    to: "Nydus".to_string(),
                    event_type: "ProcessDied".to_string(),
                    payload: serde_json::json!({
                        "exit_code": exit_code,
                        "session_id": session_id,
                    }),
                }
            }
            QueenEvent::StatusChanged {
                queen_id,
                status,
            } => {
                AuditEntry {
                    timestamp,
                    from: format!("Queen({})", queen_id.0),
                    to: "Nydus".to_string(),
                    event_type: "StatusChanged".to_string(),
                    payload: serde_json::to_value(status).unwrap_or(serde_json::Value::Null),
                }
            }
            QueenEvent::ContextCompressed {
                queen_id,
                pre_tokens,
                trigger,
            } => {
                AuditEntry {
                    timestamp,
                    from: format!("Queen({})", queen_id.0),
                    to: "Nydus".to_string(),
                    event_type: "ContextCompressed".to_string(),
                    payload: serde_json::json!({
                        "pre_tokens": pre_tokens,
                        "trigger": trigger,
                    }),
                }
            }

            QueenEvent::MessagesReceived { queen_id, count } => {
                AuditEntry {
                    timestamp,
                    from: format!("Queen({})", queen_id.0),
                    to: "Nydus".to_string(),
                    event_type: "MessagesReceived".to_string(),
                    payload: serde_json::json!({
                        "count": count,
                    }),
                }
            }
        }
    }
}

// ============================================================================
// Audit Writer Task
// ============================================================================

/// Background task that drains audit entries and logs them to SqliteEventLog.
///
/// This runs until the audit channel is closed (when EventBus is dropped).
async fn audit_writer(
    mut audit_rx: mpsc::UnboundedReceiver<AuditEntry>,
    event_log: SqliteEventLog,
) {
    while let Some(entry) = audit_rx.recv().await {
        let msg = entry.to_swarm_message();
        event_log.log(&msg);
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn test_event_bus_send_receive() {
        let mut bus = EventBus::new(100);
        let sender = bus.event_sender();

        let queen_id = QueenId("Q0".into());
        let event = QueenEvent::Progress {
            queen_id: queen_id.clone(),
            task_id: TaskId("T1".into()),
            turns_completed: 5,
            cost_usd: 0.05,
        };

        sender.send(event).await.unwrap();

        let received = bus.recv_event().await.unwrap();
        match received {
            QueenEvent::Progress { queen_id: qid, .. } => {
                assert_eq!(qid.0, "Q0");
            }
            _ => panic!("Expected Progress event"),
        }
    }

    #[tokio::test]
    async fn test_event_bus_multiple_senders() {
        let mut bus = EventBus::new(100);
        let sender1 = bus.event_sender();
        let sender2 = bus.event_sender();

        let event1 = QueenEvent::Progress {
            queen_id: QueenId("Q0".into()),
            task_id: TaskId("T1".into()),
            turns_completed: 3,
            cost_usd: 0.03,
        };
        let event2 = QueenEvent::Progress {
            queen_id: QueenId("Q1".into()),
            task_id: TaskId("T2".into()),
            turns_completed: 5,
            cost_usd: 0.05,
        };

        sender1.send(event1).await.unwrap();
        sender2.send(event2).await.unwrap();

        // Should receive both events
        let received1 = bus.recv_event().await.unwrap();
        let received2 = bus.recv_event().await.unwrap();

        assert!(matches!(received1, QueenEvent::Progress { .. }));
        assert!(matches!(received2, QueenEvent::Progress { .. }));
    }

    #[tokio::test]
    async fn test_shutdown_broadcast() {
        let bus = EventBus::new(100);
        let mut rx1 = bus.shutdown_receiver();
        let mut rx2 = bus.shutdown_receiver();

        bus.shutdown();

        // Both receivers should get the signal
        assert!(rx1.recv().await.is_ok());
        assert!(rx2.recv().await.is_ok());
    }

    #[tokio::test]
    async fn test_event_bus_with_audit() {
        // We can't easily verify audit writes in tests without Arc wrapping SqliteEventLog
        // This test just verifies the bus works with audit enabled
        let event_log = SqliteEventLog::in_memory().unwrap();
        let mut bus = EventBus::with_audit(100, event_log);
        let sender = bus.event_sender();

        let event = QueenEvent::StatusChanged {
            queen_id: QueenId("Q0".into()),
            status: QueenStatus::Idle,
        };

        // Send event and audit it
        sender.send(event.clone()).await.unwrap();
        let audit_entry = EventBus::event_to_audit_entry(&event);
        bus.audit(audit_entry);

        // Give audit writer a moment to process
        tokio::time::sleep(Duration::from_millis(50)).await;

        // We receive the event successfully
        let received = bus.recv_event().await;
        assert!(received.is_some());
    }

    #[tokio::test]
    async fn test_audit_entry_conversion() {
        let entry = AuditEntry {
            timestamp: Utc::now(),
            from: "Queen(Q0)".to_string(),
            to: "Nydus".to_string(),
            event_type: "StatusChanged".to_string(),
            payload: serde_json::json!({"test": true}),
        };

        let msg = entry.to_swarm_message();
        // Check the message was created (MessageType doesn't implement PartialEq)
        assert!(matches!(msg.msg_type, MessageType::StatusReport));
        assert_eq!(msg.payload, serde_json::json!({"test": true}));
    }

    #[tokio::test]
    async fn test_event_to_audit_entry() {
        let event = QueenEvent::Knowledge {
            queen_id: QueenId("Q0".into()),
            key: "memory_usage".to_string(),
            value: serde_json::json!({"usage": "high"}),
        };

        let entry = EventBus::event_to_audit_entry(&event);
        assert_eq!(entry.event_type, "Knowledge");
        assert_eq!(entry.from, "Queen(Q0)");
        assert_eq!(entry.payload["key"].as_str().unwrap(), "memory_usage");
    }

    #[tokio::test]
    async fn test_audit_without_logging() {
        let mut bus = EventBus::new(100); // No audit
        let entry = AuditEntry {
            timestamp: Utc::now(),
            from: "Q0".to_string(),
            to: "Host".to_string(),
            event_type: "Test".to_string(),
            payload: serde_json::json!({}),
        };

        // Should be a no-op
        bus.audit(entry);

        // Bus should still work
        let sender = bus.event_sender();
        let event = QueenEvent::Progress {
            queen_id: QueenId("Q0".into()),
            task_id: TaskId("T1".into()),
            turns_completed: 1,
            cost_usd: 0.01,
        };
        sender.send(event).await.unwrap();
        assert!(bus.recv_event().await.is_some());
    }

    #[tokio::test]
    async fn test_task_result_event() {
        let mut bus = EventBus::new(100);
        let sender = bus.event_sender();

        let event = QueenEvent::TaskCompleted {
            queen_id: QueenId("Q0".into()),
            task_id: TaskId("T1".into()),
            result_text: "Success".to_string(),
            cost_usd: 0.10,
            duration_ms: 10000,
            num_turns: 5,
            session_id: Some("session-123".to_string()),
            quality_passed: true,
        };

        sender.send(event).await.unwrap();
        let received = bus.recv_event().await.unwrap();

        match received {
            QueenEvent::TaskCompleted { task_id, .. } => {
                assert_eq!(task_id.0, "T1");
            }
            _ => panic!("Expected TaskCompleted event"),
        }
    }

    #[tokio::test]
    async fn test_channel_closure() {
        let mut bus = EventBus::new(100);
        let sender = bus.event_sender();

        drop(sender); // Drop the external sender

        // Note: The bus itself holds event_tx internally, so the channel
        // won't close until the bus is dropped. Test with a timeout instead.
        let result = tokio::time::timeout(
            std::time::Duration::from_millis(100),
            bus.recv_event()
        ).await;

        // The channel is NOT closed because bus holds event_tx internally
        // This is expected behavior — recv_event() would block
        assert!(result.is_err(), "Expected timeout, but recv_event returned");
    }

    #[tokio::test]
    async fn test_multiple_audit_entries() {
        let event_log = SqliteEventLog::in_memory().unwrap();
        let bus = EventBus::with_audit(100, event_log);

        for i in 0..5 {
            let entry = AuditEntry {
                timestamp: Utc::now(),
                from: format!("Queen(Q{})", i),
                to: "Nydus".to_string(),
                event_type: "Progress".to_string(),
                payload: serde_json::json!({
                    "task_id": format!("T{}", i),
                    "turns_completed": i,
                    "cost_usd": i as f64 * 0.01,
                }),
            };
            bus.audit(entry);
        }

        // Give audit writer time to process
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Test passes if audit doesn't crash - we can't easily verify counts
        // without wrapping SqliteEventLog in Arc
    }
}
