//! IPC server for Hatchery CLI ↔ Nydus communication.
//!
//! Starts a TCP listener that handles JSON requests from `hatchery` CLI.
//! Each connection is stateless: connect → request → response → close.

pub mod protocol;

use protocol::{IpcRequest, IpcResponse};
use crate::core::shared_memory::{KnowledgeEntry, MemoryState};
use crate::core::types::*;
use crate::core::validator::Validator;

use anyhow::Result;
use parking_lot::RwLock;
use std::sync::Arc;
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

/// IPC server configuration.
pub struct IpcConfig {
    pub working_dir: PathBuf,
    pub verify_cmd: Option<String>,
}

/// Start the IPC TCP listener.
///
/// Returns the port number. Spawns a tokio task that handles connections.
/// The listener binds to 127.0.0.1:0 (OS picks a free port).
pub async fn start_ipc_listener(
    memory: Arc<RwLock<MemoryState>>,
    config: IpcConfig,
) -> Result<u16> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();

    let config = Arc::new(config);

    tokio::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((stream, _addr)) => {
                    let memory = memory.clone();
                    let config = config.clone();
                    tokio::spawn(async move {
                        if let Err(e) = handle_connection(stream, memory, config).await {
                            eprintln!("[IPC] Connection error: {}", e);
                        }
                    });
                }
                Err(e) => {
                    eprintln!("[IPC] Accept error: {}", e);
                }
            }
        }
    });

    Ok(port)
}

/// Handle a single TCP connection.
async fn handle_connection(
    stream: tokio::net::TcpStream,
    memory: Arc<RwLock<MemoryState>>,
    config: Arc<IpcConfig>,
) -> Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();

    // Read exactly one request line
    let line = match lines.next_line().await? {
        Some(l) => l,
        None => return Ok(()), // Empty connection
    };

    // Parse request
    let request: IpcRequest = match serde_json::from_str(&line) {
        Ok(r) => r,
        Err(e) => {
            let resp = IpcResponse::Error {
                message: format!("Invalid request: {}", e),
            };
            let json = serde_json::to_string(&resp)?;
            writer.write_all(json.as_bytes()).await?;
            writer.write_all(b"\n").await?;
            return Ok(());
        }
    };

    // Handle request
    let response = handle_request(request, &memory, &config).await;

    // Send response
    let json = serde_json::to_string(&response)?;
    writer.write_all(json.as_bytes()).await?;
    writer.write_all(b"\n").await?;

    Ok(())
}

/// Process a single IPC request.
async fn handle_request(
    request: IpcRequest,
    memory: &Arc<RwLock<MemoryState>>,
    config: &IpcConfig,
) -> IpcResponse {
    match request {
        IpcRequest::Ping => IpcResponse::Ok {
            data: serde_json::json!("pong"),
        },

        IpcRequest::MemoryRead { key, pattern } => {
            let state = memory.read();
            let entries: Vec<&KnowledgeEntry> = if let Some(ref k) = key {
                state.knowledge.get(k).into_iter().collect()
            } else if let Some(ref p) = pattern {
                state.knowledge.values()
                    .filter(|e| e.key.contains(p))
                    .collect()
            } else {
                state.knowledge.values().collect()
            };

            let json_entries: Vec<serde_json::Value> = entries.iter().map(|e| {
                serde_json::json!({
                    "key": e.key,
                    "value": e.value,
                    "author": format!("{:?}", e.author),
                    "timestamp": e.timestamp.to_rfc3339(),
                })
            }).collect();

            IpcResponse::Ok {
                data: serde_json::json!(json_entries),
            }
        }

        IpcRequest::MemoryWrite { key, value, ttl_secs, queen_id } => {
            // Use queen_id if provided, otherwise default to Operator
            let author = if let Some(qid) = queen_id {
                AgentId::Queen(QueenId(qid))
            } else {
                AgentId::Operator
            };

            let entry = KnowledgeEntry {
                key: key.clone(),
                value,
                author,
                timestamp: chrono::Utc::now(),
                visibility: Visibility::default_internal(),
                ttl: ttl_secs.map(std::time::Duration::from_secs),
            };

            {
                let mut state = memory.write();
                state.knowledge.insert(key.clone(), entry);
                state.version += 1;
                state.metadata.last_updated = chrono::Utc::now();
            }

            IpcResponse::Ok {
                data: serde_json::json!({"written": key}),
            }
        }

        IpcRequest::MemoryList => {
            let state = memory.read();
            let keys: Vec<serde_json::Value> = state.knowledge.iter().map(|(key, entry)| {
                serde_json::json!({
                    "key": key,
                    "author": format!("{:?}", entry.author),
                    "timestamp": entry.timestamp.to_rfc3339(),
                })
            }).collect();

            IpcResponse::Ok {
                data: serde_json::json!({
                    "entries": keys,
                    "count": keys.len(),
                    "version": state.version,
                }),
            }
        }

        IpcRequest::MemoryInfo => {
            let state = memory.read();
            IpcResponse::Ok {
                data: serde_json::json!({
                    "version": state.version,
                    "knowledge_count": state.knowledge.len(),
                    "task_results_count": state.task_results.len(),
                    "created_at": state.metadata.created_at.to_rfc3339(),
                    "last_updated": state.metadata.last_updated.to_rfc3339(),
                    "swarm_id": state.metadata.swarm_id.0,
                }),
            }
        }

        IpcRequest::MailboxSend { to, message, msg_type: _, queen_id } => {
            // Store as knowledge entry (mailbox messages in shared memory)
            let key = format!("msg:{}", uuid::Uuid::new_v4());

            let author = if let Some(qid) = queen_id {
                AgentId::Queen(QueenId(qid))
            } else {
                AgentId::Operator
            };

            let entry = KnowledgeEntry {
                key: key.clone(),
                value: serde_json::json!({"to": to, "message": message}),
                author,
                timestamp: chrono::Utc::now(),
                visibility: Visibility::default_internal(),
                ttl: Some(std::time::Duration::from_secs(3600)), // 1 hour TTL for messages
            };

            {
                let mut state = memory.write();
                state.knowledge.insert(key.clone(), entry);
                state.version += 1;
                state.metadata.last_updated = chrono::Utc::now();
            }

            IpcResponse::Ok {
                data: serde_json::json!({"sent": true, "key": key}),
            }
        }

        IpcRequest::MailboxRead { from, limit } => {
            let state = memory.read();
            let limit = limit.unwrap_or(20);

            let messages: Vec<serde_json::Value> = state.knowledge.iter()
                .filter(|(k, _)| k.starts_with("msg:"))
                .filter(|(_, entry)| {
                    if let Some(ref from_filter) = from {
                        format!("{:?}", entry.author).contains(from_filter)
                    } else {
                        true
                    }
                })
                .take(limit)
                .map(|(_, entry)| {
                    serde_json::json!({
                        "key": entry.key,
                        "value": entry.value,
                        "author": format!("{:?}", entry.author),
                        "timestamp": entry.timestamp.to_rfc3339(),
                    })
                })
                .collect();

            IpcResponse::Ok {
                data: serde_json::json!(messages),
            }
        }

        IpcRequest::Validate { command } => {
            let cmd_str = command
                .or_else(|| config.verify_cmd.clone())
                .unwrap_or_else(|| "cargo check".to_string());

            let validator = Validator::command(&cmd_str, config.working_dir.clone());
            match validator.validate(&[]).await {
                Ok(result) => {
                    IpcResponse::Ok {
                        data: serde_json::json!({
                            "passed": result.passed,
                            "feedback": result.feedback,
                        }),
                    }
                }
                Err(e) => IpcResponse::Error {
                    message: format!("Validation error: {}", e),
                },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::types::NydusId;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;

    fn test_nydus_id() -> NydusId {
        NydusId("test-swarm".to_string())
    }

    async fn send_request(port: u16, request: &IpcRequest) -> Result<IpcResponse> {
        let mut stream = TcpStream::connect(format!("127.0.0.1:{}", port)).await?;

        // Send request
        let json = serde_json::to_string(request)?;
        stream.write_all(json.as_bytes()).await?;
        stream.write_all(b"\n").await?;

        // Read response
        let (reader, _writer) = stream.into_split();
        let mut lines = BufReader::new(reader).lines();
        let line = lines.next_line().await?.ok_or_else(|| anyhow::anyhow!("No response"))?;

        let response: IpcResponse = serde_json::from_str(&line)?;
        Ok(response)
    }

    #[tokio::test]
    async fn test_ping_pong() {
        let memory = Arc::new(RwLock::new(MemoryState {
            version: 0,
            knowledge: std::collections::HashMap::new(),
            task_results: std::collections::HashMap::new(),
            metadata: crate::core::shared_memory::MemoryMetadata {
                created_at: chrono::Utc::now(),
                last_updated: chrono::Utc::now(),
                swarm_id: test_nydus_id(),
            },
        }));

        let config = IpcConfig {
            working_dir: std::env::current_dir().unwrap(),
            verify_cmd: None,
        };

        let port = start_ipc_listener(memory, config).await.unwrap();

        // Give the listener a moment to start
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        let response = send_request(port, &IpcRequest::Ping).await.unwrap();

        match response {
            IpcResponse::Ok { data } => {
                assert_eq!(data, serde_json::json!("pong"));
            }
            IpcResponse::Error { message } => panic!("Unexpected error: {}", message),
        }
    }

    #[tokio::test]
    async fn test_memory_write_read() {
        let memory = Arc::new(RwLock::new(MemoryState {
            version: 0,
            knowledge: std::collections::HashMap::new(),
            task_results: std::collections::HashMap::new(),
            metadata: crate::core::shared_memory::MemoryMetadata {
                created_at: chrono::Utc::now(),
                last_updated: chrono::Utc::now(),
                swarm_id: test_nydus_id(),
            },
        }));

        let config = IpcConfig {
            working_dir: std::env::current_dir().unwrap(),
            verify_cmd: None,
        };

        let port = start_ipc_listener(memory.clone(), config).await.unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        // Write a key
        let write_req = IpcRequest::MemoryWrite {
            key: "test-key".to_string(),
            value: serde_json::json!({"data": 42}),
            ttl_secs: None,
            queen_id: Some("Q0".to_string()),
        };

        let write_resp = send_request(port, &write_req).await.unwrap();
        match write_resp {
            IpcResponse::Ok { data } => {
                assert_eq!(data["written"], "test-key");
            }
            IpcResponse::Error { message } => panic!("Write failed: {}", message),
        }

        // Read it back
        let read_req = IpcRequest::MemoryRead {
            key: Some("test-key".to_string()),
            pattern: None,
        };

        let read_resp = send_request(port, &read_req).await.unwrap();
        match read_resp {
            IpcResponse::Ok { data } => {
                let entries = data.as_array().unwrap();
                assert_eq!(entries.len(), 1);
                assert_eq!(entries[0]["key"], "test-key");
                assert_eq!(entries[0]["value"]["data"], 42);
                assert!(entries[0]["author"].as_str().unwrap().contains("Q0"));
            }
            IpcResponse::Error { message } => panic!("Read failed: {}", message),
        }
    }

    #[tokio::test]
    async fn test_memory_list() {
        let memory = Arc::new(RwLock::new(MemoryState {
            version: 0,
            knowledge: std::collections::HashMap::new(),
            task_results: std::collections::HashMap::new(),
            metadata: crate::core::shared_memory::MemoryMetadata {
                created_at: chrono::Utc::now(),
                last_updated: chrono::Utc::now(),
                swarm_id: test_nydus_id(),
            },
        }));

        let config = IpcConfig {
            working_dir: std::env::current_dir().unwrap(),
            verify_cmd: None,
        };

        let port = start_ipc_listener(memory.clone(), config).await.unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        // Write multiple entries
        for i in 0..3 {
            let write_req = IpcRequest::MemoryWrite {
                key: format!("key-{}", i),
                value: serde_json::json!(i),
                ttl_secs: None,
                queen_id: None,
            };
            send_request(port, &write_req).await.unwrap();
        }

        // List all entries
        let list_req = IpcRequest::MemoryList;
        let list_resp = send_request(port, &list_req).await.unwrap();

        match list_resp {
            IpcResponse::Ok { data } => {
                assert_eq!(data["count"], 3);
                assert_eq!(data["entries"].as_array().unwrap().len(), 3);
            }
            IpcResponse::Error { message } => panic!("List failed: {}", message),
        }
    }

    #[tokio::test]
    async fn test_memory_info() {
        let memory = Arc::new(RwLock::new(MemoryState {
            version: 0,
            knowledge: std::collections::HashMap::new(),
            task_results: std::collections::HashMap::new(),
            metadata: crate::core::shared_memory::MemoryMetadata {
                created_at: chrono::Utc::now(),
                last_updated: chrono::Utc::now(),
                swarm_id: test_nydus_id(),
            },
        }));

        let config = IpcConfig {
            working_dir: std::env::current_dir().unwrap(),
            verify_cmd: None,
        };

        let port = start_ipc_listener(memory.clone(), config).await.unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        let info_req = IpcRequest::MemoryInfo;
        let info_resp = send_request(port, &info_req).await.unwrap();

        match info_resp {
            IpcResponse::Ok { data } => {
                assert_eq!(data["swarm_id"], "test-swarm");
                assert_eq!(data["version"], 0);
                assert_eq!(data["knowledge_count"], 0);
            }
            IpcResponse::Error { message } => panic!("Info failed: {}", message),
        }
    }

    #[tokio::test]
    async fn test_invalid_request() {
        let memory = Arc::new(RwLock::new(MemoryState {
            version: 0,
            knowledge: std::collections::HashMap::new(),
            task_results: std::collections::HashMap::new(),
            metadata: crate::core::shared_memory::MemoryMetadata {
                created_at: chrono::Utc::now(),
                last_updated: chrono::Utc::now(),
                swarm_id: test_nydus_id(),
            },
        }));

        let config = IpcConfig {
            working_dir: std::env::current_dir().unwrap(),
            verify_cmd: None,
        };

        let port = start_ipc_listener(memory, config).await.unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        // Send invalid JSON
        let mut stream = TcpStream::connect(format!("127.0.0.1:{}", port)).await.unwrap();
        stream.write_all(b"not json\n").await.unwrap();

        let (reader, _writer) = stream.into_split();
        let mut lines = BufReader::new(reader).lines();
        let line = lines.next_line().await.unwrap().unwrap();

        let response: IpcResponse = serde_json::from_str(&line).unwrap();
        match response {
            IpcResponse::Error { message } => {
                assert!(message.contains("Invalid request"));
            }
            IpcResponse::Ok { .. } => panic!("Expected error response"),
        }
    }
}
