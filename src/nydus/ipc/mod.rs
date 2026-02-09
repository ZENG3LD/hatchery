//! IPC server for Hatchery CLI ↔ Nydus communication.
//!
//! Starts a TCP listener that handles JSON requests from `hatchery` CLI.
//! Each connection is stateless: connect → request → response → close.

pub mod protocol;

use protocol::{IpcRequest, IpcResponse, QueenStatusSnapshot, InjectRequest};
use crate::core::shared_memory::{KnowledgeEntry, MemoryState};
use crate::core::types::*;
use crate::core::validator::Validator;
use crate::nydus::mailbox::SwarmMailbox;

use anyhow::Result;
use parking_lot::{RwLock, Mutex};
use std::sync::Arc;
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::sync::mpsc;

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
    queen_snapshots: Arc<parking_lot::RwLock<Vec<QueenStatusSnapshot>>>,
    memory: Arc<RwLock<MemoryState>>,
    mailbox: Arc<Mutex<SwarmMailbox>>,
    inject_tx: mpsc::Sender<InjectRequest>,
    message_delivery_tx: mpsc::Sender<protocol::MessageDeliveryNotification>,
    config: IpcConfig,
    shutdown_tx: tokio::sync::watch::Sender<bool>,
    started_at: std::time::Instant,
    keep_alive: bool,
) -> Result<u16> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();

    let config = Arc::new(config);
    let shutdown_tx = Arc::new(shutdown_tx);
    let started_at = Arc::new(started_at);

    tokio::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((stream, _addr)) => {
                    let memory = memory.clone();
                    let mailbox = mailbox.clone();
                    let queen_snapshots = queen_snapshots.clone();
                    let inject_tx = inject_tx.clone();
                    let message_delivery_tx = message_delivery_tx.clone();
                    let config = config.clone();
                    let shutdown_tx = shutdown_tx.clone();
                    let started_at = started_at.clone();
                    tokio::spawn(async move {
                        if let Err(e) = handle_connection(
                            stream,
                            memory,
                            mailbox,
                            queen_snapshots,
                            inject_tx,
                            message_delivery_tx,
                            config,
                            shutdown_tx,
                            started_at,
                            keep_alive,
                        ).await {
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
    mailbox: Arc<Mutex<SwarmMailbox>>,
    queen_snapshots: Arc<parking_lot::RwLock<Vec<QueenStatusSnapshot>>>,
    inject_tx: mpsc::Sender<InjectRequest>,
    message_delivery_tx: mpsc::Sender<protocol::MessageDeliveryNotification>,
    config: Arc<IpcConfig>,
    shutdown_tx: Arc<tokio::sync::watch::Sender<bool>>,
    started_at: Arc<std::time::Instant>,
    keep_alive: bool,
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
    let response = handle_request(
        request,
        &memory,
        &mailbox,
        &queen_snapshots,
        &inject_tx,
        &message_delivery_tx,
        &config,
        &shutdown_tx,
        &started_at,
        keep_alive,
    ).await;

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
    mailbox: &Arc<Mutex<SwarmMailbox>>,
    queen_snapshots: &Arc<parking_lot::RwLock<Vec<QueenStatusSnapshot>>>,
    inject_tx: &mpsc::Sender<InjectRequest>,
    message_delivery_tx: &mpsc::Sender<protocol::MessageDeliveryNotification>,
    config: &IpcConfig,
    shutdown_tx: &Arc<tokio::sync::watch::Sender<bool>>,
    started_at: &Arc<std::time::Instant>,
    keep_alive: bool,
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

        IpcRequest::MailboxSend { to, message, msg_type, queen_id } => {
            // Parse sender (from)
            let from = if let Some(qid) = queen_id {
                AgentId::Queen(QueenId(qid))
            } else {
                AgentId::Operator
            };

            // Parse recipient (to)
            let to_agent = parse_agent_id(&to);
            if to_agent.is_none() {
                return IpcResponse::Error {
                    message: format!("Invalid 'to' field: '{}'. Expected format: 'queen:Q0', 'nydus', 'operator', or 'validator'", to),
                };
            }
            let to_agent = to_agent.unwrap();

            // Determine message type
            let message_type = if let Some(mt) = msg_type {
                match mt.as_str() {
                    "status_request" => MessageType::StatusRequest,
                    "status_report" => MessageType::StatusReport,
                    "knowledge" => MessageType::Knowledge,
                    "knowledge_query" => MessageType::KnowledgeQuery,
                    "escalation" => MessageType::Escalation,
                    "shutdown" => MessageType::Shutdown,
                    "memory_ref" => MessageType::MemoryRef,
                    other => MessageType::Custom(other.to_string()),
                }
            } else {
                MessageType::Custom("operator_message".to_string())
            };

            // Create SwarmMessage
            let message_id = uuid::Uuid::new_v4().to_string();
            let swarm_message = SwarmMessage {
                id: message_id.clone(),
                from,
                to: to_agent.clone(),
                msg_type: message_type,
                payload: serde_json::json!({ "message": message }),
                timestamp: chrono::Utc::now(),
                correlation_id: None,
                visibility: Visibility::default_internal(),
            };

            // Route through SwarmMailbox
            {
                let mut mb = mailbox.lock();
                mb.send(swarm_message);
            }

            // If message is sent to a queen, notify Nydus for potential delivery
            if let AgentId::Queen(qid) = to_agent {
                let notification = protocol::MessageDeliveryNotification {
                    queen_id: qid,
                    message_id: message_id.clone(),
                };
                // Best-effort send (don't fail if channel full)
                let _ = message_delivery_tx.try_send(notification);
            }

            IpcResponse::Ok {
                data: serde_json::json!({
                    "sent": true,
                    "message_id": message_id
                }),
            }
        }

        IpcRequest::MailboxRead { from, limit } => {
            let limit = limit.unwrap_or(20);

            let messages: Vec<serde_json::Value> = if let Some(ref from_str) = from {
                // Reading from a specific agent's inbox (e.g., "queen:Q0")
                let agent_id = parse_agent_id(from_str);
                if agent_id.is_none() {
                    return IpcResponse::Error {
                        message: format!("Invalid 'from' field: '{}'. Expected format: 'queen:Q0', 'nydus', 'operator', or 'validator'", from_str),
                    };
                }

                let mut mb = mailbox.lock();
                let mut msgs = Vec::new();

                match agent_id.unwrap() {
                    AgentId::Queen(qid) => {
                        // Read from queen's inbox
                        for _ in 0..limit {
                            if let Some(msg) = mb.recv_queen(&qid) {
                                msgs.push(swarm_message_to_json(&msg));
                            } else {
                                break;
                            }
                        }
                    }
                    AgentId::Nydus(_) => {
                        // Read from Nydus (host) inbox
                        for _ in 0..limit {
                            if let Some(msg) = mb.recv_host() {
                                msgs.push(swarm_message_to_json(&msg));
                            } else {
                                break;
                            }
                        }
                    }
                    AgentId::Validator => {
                        // Read from Validator inbox
                        for _ in 0..limit {
                            if let Some(msg) = mb.recv_validator() {
                                msgs.push(swarm_message_to_json(&msg));
                            } else {
                                break;
                            }
                        }
                    }
                    AgentId::Operator => {
                        // Read from outbox (operator sees messages TO operator)
                        let drained = mb.drain_outbox();
                        msgs = drained.iter().take(limit).map(swarm_message_to_json).collect();
                    }
                }

                msgs
            } else {
                // Default: read operator's outbox (messages sent to operator)
                let mut mb = mailbox.lock();
                let drained = mb.drain_outbox();
                drained.iter().take(limit).map(swarm_message_to_json).collect()
            };

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

        IpcRequest::QueenStatus { queen_id } => {
            let snapshots = queen_snapshots.read();

            let filtered_queens: Vec<QueenStatusSnapshot> = if let Some(ref qid) = queen_id {
                // Filter for specific queen
                snapshots
                    .iter()
                    .filter(|snapshot| snapshot.id == *qid)
                    .cloned()
                    .collect()
            } else {
                // Return all queens
                snapshots.clone()
            };

            IpcResponse::Ok {
                data: serde_json::json!({
                    "queens": filtered_queens
                }),
            }
        }

        IpcRequest::InjectTask { queen_id, prompt, priority, task_id } => {
            // Parse queen_id if provided
            let queen_opt = queen_id.map(|qid| QueenId(qid));

            // Generate task_id if not provided
            let task_id = task_id.unwrap_or_else(|| format!("injected-{}", uuid::Uuid::new_v4()));

            // Default priority
            let priority = priority.unwrap_or(128);

            // Create oneshot channel for response
            let (response_tx, response_rx) = tokio::sync::oneshot::channel();

            // Create InjectRequest
            let inject_req = InjectRequest {
                queen_id: queen_opt,
                prompt,
                priority,
                task_id: task_id.clone(),
                response_tx,
            };

            // Send to Nydus via channel
            if let Err(e) = inject_tx.send(inject_req).await {
                return IpcResponse::Error {
                    message: format!("Failed to inject task: {}", e),
                };
            }

            // Wait for response from Nydus
            match response_rx.await {
                Ok(inject_response) => {
                    IpcResponse::Ok {
                        data: serde_json::to_value(&inject_response).unwrap_or(serde_json::json!({})),
                    }
                }
                Err(e) => {
                    IpcResponse::Error {
                        message: format!("Failed to receive inject response: {}", e),
                    }
                }
            }
        }

        IpcRequest::Shutdown => {
            // Send shutdown signal to Nydus via watch channel
            if let Err(e) = shutdown_tx.send(true) {
                return IpcResponse::Error {
                    message: format!("Failed to send shutdown signal: {}", e),
                };
            }

            IpcResponse::Ok {
                data: serde_json::json!("Swarm shutting down"),
            }
        }

        IpcRequest::SwarmStatus => {
            // Get task stats from snapshots (we don't have direct access to TaskDag here)
            // Instead, we compute stats from queen snapshots
            let snapshots = queen_snapshots.read();

            let queens_alive = snapshots.iter().filter(|q| q.is_alive).count();
            let queens_idle = snapshots.iter().filter(|q| q.is_alive && q.status == "idle").count();

            // For task stats, we need to query from memory
            // In a real implementation, we'd pass a separate Arc<RwLock<TaskDagStats>>
            // For now, we'll use placeholder values from memory or return what we know
            let state = memory.read();
            let total_tasks = state.task_results.len();

            // Count completed/failed from task_results
            let mut completed = 0;
            let mut failed = 0;
            for result in state.task_results.values() {
                if let Some(status) = result.get("status").and_then(|s| s.as_str()) {
                    match status {
                        "completed" => completed += 1,
                        "failed" => failed += 1,
                        _ => {}
                    }
                }
            }

            let in_progress = snapshots.iter().filter(|q| q.status == "working").count();

            let uptime_secs = started_at.elapsed().as_secs();

            IpcResponse::Ok {
                data: serde_json::json!({
                    "total_tasks": total_tasks,
                    "completed": completed,
                    "failed": failed,
                    "in_progress": in_progress,
                    "queens_alive": queens_alive,
                    "queens_idle": queens_idle,
                    "uptime_secs": uptime_secs,
                    "keep_alive": keep_alive,
                }),
            }
        }
    }
}

/// Parse agent ID from string format.
///
/// Supported formats:
/// - "queen:Q0" -> AgentId::Queen(QueenId("Q0"))
/// - "nydus" -> AgentId::Nydus(NydusId::default())
/// - "operator" -> AgentId::Operator
/// - "validator" -> AgentId::Validator
fn parse_agent_id(s: &str) -> Option<AgentId> {
    if s.starts_with("queen:") {
        let queen_id = s.strip_prefix("queen:")?;
        Some(AgentId::Queen(QueenId(queen_id.to_string())))
    } else if s == "nydus" {
        Some(AgentId::Nydus(NydusId::default()))
    } else if s == "operator" {
        Some(AgentId::Operator)
    } else if s == "validator" {
        Some(AgentId::Validator)
    } else {
        None
    }
}

/// Convert SwarmMessage to JSON for IPC response.
fn swarm_message_to_json(msg: &SwarmMessage) -> serde_json::Value {
    let from_str = match &msg.from {
        AgentId::Queen(qid) => format!("queen:{}", qid.0),
        AgentId::Nydus(nid) => format!("nydus:{}", nid.0),
        AgentId::Validator => "validator".to_string(),
        AgentId::Operator => "operator".to_string(),
    };

    let to_str = match &msg.to {
        AgentId::Queen(qid) => format!("queen:{}", qid.0),
        AgentId::Nydus(nid) => format!("nydus:{}", nid.0),
        AgentId::Validator => "validator".to_string(),
        AgentId::Operator => "operator".to_string(),
    };

    let msg_type_str = match &msg.msg_type {
        MessageType::TaskAssignment => "task_assignment".to_string(),
        MessageType::TaskResult => "task_result".to_string(),
        MessageType::TaskProgress => "task_progress".to_string(),
        MessageType::StatusRequest => "status_request".to_string(),
        MessageType::StatusReport => "status_report".to_string(),
        MessageType::Knowledge => "knowledge".to_string(),
        MessageType::KnowledgeQuery => "knowledge_query".to_string(),
        MessageType::Escalation => "escalation".to_string(),
        MessageType::Shutdown => "shutdown".to_string(),
        MessageType::MemoryRef => "memory_ref".to_string(),
        MessageType::Custom(s) => s.clone(),
    };

    serde_json::json!({
        "id": msg.id,
        "from": from_str,
        "to": to_str,
        "msg_type": msg_type_str,
        "payload": msg.payload,
        "timestamp": msg.timestamp.to_rfc3339(),
    })
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

        let event_log = Arc::new(crate::nydus::mailbox::event_log::SqliteEventLog::in_memory().unwrap());
        let mailbox = Arc::new(Mutex::new(SwarmMailbox::new(event_log)));
        let queen_snapshots = Arc::new(parking_lot::RwLock::new(Vec::new()));
        let (inject_tx, _inject_rx) = mpsc::channel(32);

        let config = IpcConfig {
            working_dir: std::env::current_dir().unwrap(),
            verify_cmd: None,
        };

        let (message_delivery_tx, _message_delivery_rx) = mpsc::channel(32);
        let (shutdown_tx, _shutdown_rx) = tokio::sync::watch::channel(false);
        let port = start_ipc_listener(
            queen_snapshots.clone(),
            memory,
            mailbox,
            inject_tx,
            message_delivery_tx,
            config,
            shutdown_tx,
            std::time::Instant::now(),
            false,
        ).await.unwrap();

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

        let event_log = Arc::new(crate::nydus::mailbox::event_log::SqliteEventLog::in_memory().unwrap());
        let mailbox = Arc::new(Mutex::new(SwarmMailbox::new(event_log)));
        let queen_snapshots = Arc::new(parking_lot::RwLock::new(Vec::new()));
        let (inject_tx, _inject_rx) = mpsc::channel(32);

        let config = IpcConfig {
            working_dir: std::env::current_dir().unwrap(),
            verify_cmd: None,
        };

        let (message_delivery_tx, _message_delivery_rx) = mpsc::channel(32);
        let (shutdown_tx, _shutdown_rx) = tokio::sync::watch::channel(false);
        let port = start_ipc_listener(
            queen_snapshots.clone(),
            memory.clone(),
            mailbox,
            inject_tx,
            message_delivery_tx,
            config,
            shutdown_tx,
            std::time::Instant::now(),
            false,
        ).await.unwrap();
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

        let event_log = Arc::new(crate::nydus::mailbox::event_log::SqliteEventLog::in_memory().unwrap());
        let mailbox = Arc::new(Mutex::new(SwarmMailbox::new(event_log)));
        let queen_snapshots = Arc::new(parking_lot::RwLock::new(Vec::new()));
        let (inject_tx, _inject_rx) = mpsc::channel(32);

        let config = IpcConfig {
            working_dir: std::env::current_dir().unwrap(),
            verify_cmd: None,
        };

        let (message_delivery_tx, _message_delivery_rx) = mpsc::channel(32);
        let (shutdown_tx, _shutdown_rx) = tokio::sync::watch::channel(false);
        let port = start_ipc_listener(
            queen_snapshots.clone(),
            memory.clone(),
            mailbox,
            inject_tx,
            message_delivery_tx,
            config,
            shutdown_tx,
            std::time::Instant::now(),
            false,
        ).await.unwrap();
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

        let event_log = Arc::new(crate::nydus::mailbox::event_log::SqliteEventLog::in_memory().unwrap());
        let mailbox = Arc::new(Mutex::new(SwarmMailbox::new(event_log)));
        let queen_snapshots = Arc::new(parking_lot::RwLock::new(Vec::new()));
        let (inject_tx, _inject_rx) = mpsc::channel(32);

        let config = IpcConfig {
            working_dir: std::env::current_dir().unwrap(),
            verify_cmd: None,
        };

        let (message_delivery_tx, _message_delivery_rx) = mpsc::channel(32);
        let (shutdown_tx, _shutdown_rx) = tokio::sync::watch::channel(false);
        let port = start_ipc_listener(
            queen_snapshots.clone(),
            memory.clone(),
            mailbox,
            inject_tx,
            message_delivery_tx,
            config,
            shutdown_tx,
            std::time::Instant::now(),
            false,
        ).await.unwrap();
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

        let event_log = Arc::new(crate::nydus::mailbox::event_log::SqliteEventLog::in_memory().unwrap());
        let mailbox = Arc::new(Mutex::new(SwarmMailbox::new(event_log)));
        let queen_snapshots = Arc::new(parking_lot::RwLock::new(Vec::new()));
        let (inject_tx, _inject_rx) = mpsc::channel(32);

        let config = IpcConfig {
            working_dir: std::env::current_dir().unwrap(),
            verify_cmd: None,
        };

        let (message_delivery_tx, _message_delivery_rx) = mpsc::channel(32);
        let (shutdown_tx, _shutdown_rx) = tokio::sync::watch::channel(false);
        let port = start_ipc_listener(
            queen_snapshots.clone(),
            memory,
            mailbox,
            inject_tx,
            message_delivery_tx,
            config,
            shutdown_tx,
            std::time::Instant::now(),
            false,
        ).await.unwrap();
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
