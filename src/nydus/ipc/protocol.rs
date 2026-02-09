//! IPC protocol types for Hatchery CLI ↔ Nydus communication.

use serde::{Deserialize, Serialize};

/// Snapshot of Queen status for IPC queries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueenStatusSnapshot {
    /// Queen ID (e.g., "Q0")
    pub id: String,
    /// Current status as string ("idle", "working", "blocked", "failed", "completed", "dead")
    pub status: String,
    /// Task ID if working/blocked/failed/completed
    pub task_id: Option<String>,
    /// Progress (0.0-1.0) if working
    pub progress: Option<f32>,
    /// Spawn mode ("stream" or "per_task")
    pub spawn_mode: String,
    /// Is the Queen process still alive?
    pub is_alive: bool,
}

/// Request from CLI to Nydus (sent as JSON line over TCP).
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "cmd")]
pub enum IpcRequest {
    /// Read from shared memory.
    MemoryRead {
        key: Option<String>,
        pattern: Option<String>,
    },
    /// Write to shared memory.
    MemoryWrite {
        key: String,
        value: serde_json::Value,
        #[serde(default)]
        ttl_secs: Option<u64>,
        /// Optional queen_id for author attribution
        #[serde(default)]
        queen_id: Option<String>,
    },
    /// List all keys in shared memory.
    MemoryList,
    /// Get memory metadata/info.
    MemoryInfo,
    /// Send a message to another agent.
    MailboxSend {
        to: String,
        message: String,
        #[serde(default)]
        msg_type: Option<String>,
        /// Optional queen_id for sender identification
        #[serde(default)]
        queen_id: Option<String>,
    },
    /// Read messages from mailbox.
    MailboxRead {
        from: Option<String>,
        #[serde(default)]
        limit: Option<usize>,
    },
    /// Run validation command.
    Validate {
        command: Option<String>,
    },
    /// Health check.
    Ping,
    /// Query status of one or all queens.
    QueenStatus {
        /// Optional queen ID filter (e.g., "Q0"). If None, returns all queens.
        queen_id: Option<String>,
    },
    /// Inject a new task into the running swarm.
    InjectTask {
        /// Target queen ID (e.g., "Q0"). If None, scheduler assigns to next idle queen.
        queen_id: Option<String>,
        /// Task description/prompt.
        prompt: String,
        /// Priority (0-255, higher = more important). Default: 128.
        #[serde(default)]
        priority: Option<u8>,
        /// Custom task ID. If None, auto-generated as "injected-{uuid}".
        #[serde(default)]
        task_id: Option<String>,
    },
    /// Gracefully shutdown the swarm.
    Shutdown,
    /// Get overall swarm status.
    SwarmStatus,
}

/// Request to inject a task into the swarm (sent from IPC handler to Nydus).
#[derive(Debug)]
pub struct InjectRequest {
    /// Target queen (if specified)
    pub queen_id: Option<crate::core::types::QueenId>,
    /// Task prompt/description
    pub prompt: String,
    /// Task priority
    pub priority: u8,
    /// Task ID
    pub task_id: String,
    /// Response channel
    pub response_tx: tokio::sync::oneshot::Sender<InjectResponse>,
}

/// Response to an inject request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InjectResponse {
    /// The task ID that was created
    pub task_id: String,
    /// Which queen it was assigned to (if immediate assignment)
    pub assigned_to: Option<String>,
    /// Status: "assigned", "queued", or "error"
    pub status: String,
}

/// Notification that a message was sent to a queen's inbox.
/// Sent from IPC handler to Nydus to trigger message delivery.
#[derive(Debug, Clone)]
pub struct MessageDeliveryNotification {
    /// Target queen ID
    pub queen_id: crate::core::types::QueenId,
    /// Message ID (for logging/debugging)
    pub message_id: String,
}

/// Response from Nydus to CLI.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "status")]
pub enum IpcResponse {
    /// Successful response with data.
    Ok { data: serde_json::Value },
    /// Error response.
    Error { message: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ping_serialization() {
        let req = IpcRequest::Ping;
        let json = serde_json::to_string(&req).unwrap();
        let decoded: IpcRequest = serde_json::from_str(&json).unwrap();

        match decoded {
            IpcRequest::Ping => {},
            _ => panic!("Expected Ping variant"),
        }
    }

    #[test]
    fn test_memory_read_serialization() {
        let req = IpcRequest::MemoryRead {
            key: Some("test-key".to_string()),
            pattern: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        let decoded: IpcRequest = serde_json::from_str(&json).unwrap();

        match decoded {
            IpcRequest::MemoryRead { key, pattern } => {
                assert_eq!(key, Some("test-key".to_string()));
                assert_eq!(pattern, None);
            },
            _ => panic!("Expected MemoryRead variant"),
        }
    }

    #[test]
    fn test_memory_write_serialization() {
        let req = IpcRequest::MemoryWrite {
            key: "my-key".to_string(),
            value: serde_json::json!({"data": 42}),
            ttl_secs: Some(3600),
            queen_id: Some("Q0".to_string()),
        };
        let json = serde_json::to_string(&req).unwrap();
        let decoded: IpcRequest = serde_json::from_str(&json).unwrap();

        match decoded {
            IpcRequest::MemoryWrite { key, value, ttl_secs, queen_id } => {
                assert_eq!(key, "my-key");
                assert_eq!(value, serde_json::json!({"data": 42}));
                assert_eq!(ttl_secs, Some(3600));
                assert_eq!(queen_id, Some("Q0".to_string()));
            },
            _ => panic!("Expected MemoryWrite variant"),
        }
    }

    #[test]
    fn test_memory_list_serialization() {
        let req = IpcRequest::MemoryList;
        let json = serde_json::to_string(&req).unwrap();
        let decoded: IpcRequest = serde_json::from_str(&json).unwrap();

        match decoded {
            IpcRequest::MemoryList => {},
            _ => panic!("Expected MemoryList variant"),
        }
    }

    #[test]
    fn test_mailbox_send_serialization() {
        let req = IpcRequest::MailboxSend {
            to: "Q1".to_string(),
            message: "Hello".to_string(),
            msg_type: Some("notification".to_string()),
            queen_id: Some("Q0".to_string()),
        };
        let json = serde_json::to_string(&req).unwrap();
        let decoded: IpcRequest = serde_json::from_str(&json).unwrap();

        match decoded {
            IpcRequest::MailboxSend { to, message, msg_type, queen_id } => {
                assert_eq!(to, "Q1");
                assert_eq!(message, "Hello");
                assert_eq!(msg_type, Some("notification".to_string()));
                assert_eq!(queen_id, Some("Q0".to_string()));
            },
            _ => panic!("Expected MailboxSend variant"),
        }
    }

    #[test]
    fn test_validate_serialization() {
        let req = IpcRequest::Validate {
            command: Some("cargo check".to_string()),
        };
        let json = serde_json::to_string(&req).unwrap();
        let decoded: IpcRequest = serde_json::from_str(&json).unwrap();

        match decoded {
            IpcRequest::Validate { command } => {
                assert_eq!(command, Some("cargo check".to_string()));
            },
            _ => panic!("Expected Validate variant"),
        }
    }

    #[test]
    fn test_response_ok_serialization() {
        let resp = IpcResponse::Ok {
            data: serde_json::json!({"result": "success"}),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let decoded: IpcResponse = serde_json::from_str(&json).unwrap();

        match decoded {
            IpcResponse::Ok { data } => {
                assert_eq!(data, serde_json::json!({"result": "success"}));
            },
            IpcResponse::Error { .. } => panic!("Expected Ok variant"),
        }
    }

    #[test]
    fn test_response_error_serialization() {
        let resp = IpcResponse::Error {
            message: "Something went wrong".to_string(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let decoded: IpcResponse = serde_json::from_str(&json).unwrap();

        match decoded {
            IpcResponse::Error { message } => {
                assert_eq!(message, "Something went wrong");
            },
            IpcResponse::Ok { .. } => panic!("Expected Error variant"),
        }
    }

    #[test]
    fn test_round_trip_all_variants() {
        let requests = vec![
            IpcRequest::Ping,
            IpcRequest::MemoryRead {
                key: None,
                pattern: Some("user:*".to_string()),
            },
            IpcRequest::MemoryWrite {
                key: "test".to_string(),
                value: serde_json::json!(123),
                ttl_secs: None,
                queen_id: None,
            },
            IpcRequest::MemoryList,
            IpcRequest::MemoryInfo,
            IpcRequest::MailboxSend {
                to: "target".to_string(),
                message: "msg".to_string(),
                msg_type: None,
                queen_id: None,
            },
            IpcRequest::MailboxRead {
                from: None,
                limit: Some(10),
            },
            IpcRequest::Validate { command: None },
        ];

        for req in requests {
            let json = serde_json::to_string(&req).unwrap();
            let _decoded: IpcRequest = serde_json::from_str(&json).unwrap();
        }
    }
}
