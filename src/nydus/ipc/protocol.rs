//! IPC protocol types for Hatchery CLI ↔ Nydus communication.

use serde::{Deserialize, Serialize};

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
