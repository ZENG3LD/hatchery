//! V2 Core types for Hatchery swarm orchestration.
//!
//! This module contains the type definitions for the V2 architecture which introduces
//! the Queen trait, Nydus coordination, and hierarchical task management.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

// ============================================================================
// ID Newtypes
// ============================================================================

/// Unique identifier for a task in the V2 system.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TaskId(pub String);

impl Default for TaskId {
    fn default() -> Self {
        TaskId(String::new())
    }
}

/// Unique identifier for a Queen agent.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct QueenId(pub String); // e.g., "Q0", "Q1", "L2.0.Q0"

/// Unique identifier for a Nydus coordinator.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NydusId(pub String);

impl Default for NydusId {
    fn default() -> Self {
        NydusId(String::new())
    }
}

/// Unique identifier for a worker sub-agent.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WorkerId(pub String);

/// Unique identifier for an Overlord agent.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OverlordId(pub String);

impl std::fmt::Display for OverlordId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Unique identifier for an Overmind agent.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OvermindId(pub String);

impl std::fmt::Display for OvermindId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ============================================================================
// Enums
// ============================================================================

/// Current status of a Queen agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum QueenStatus {
    /// Queen is idle and ready to accept tasks
    Idle,
    /// Queen is actively working on a task
    Working {
        task_id: TaskId,
        progress: f32,
    },
    /// Queen is blocked waiting for dependencies or external input
    Blocked { task_id: TaskId, reason: String },
    /// Queen encountered an error while working on a task
    Failed { task_id: TaskId, error: String },
    /// Queen successfully completed a task
    Completed { task_id: TaskId },
    /// Queen is unresponsive or crashed
    Dead,
}

/// Overall status of a task in the system.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TaskStatus {
    /// Task is blocked by dependencies
    Blocked,
    /// Task is ready to be assigned
    Ready,
    /// Task has been assigned to a Queen
    Assigned,
    /// Task is currently being worked on
    InProgress,
    /// Task implementation is complete, awaiting validation
    Validating,
    /// Task completed successfully
    Completed,
    /// Task failed
    Failed,
}

/// Type of message being sent between agents.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MessageType {
    /// Assigning a task to a Queen
    TaskAssignment,
    /// Returning the result of a completed task
    TaskResult,
    /// Progress update on an in-progress task
    TaskProgress,
    /// Request for status information
    StatusRequest,
    /// Status report response
    StatusReport,
    /// Sharing knowledge/context
    Knowledge,
    /// Querying for knowledge
    KnowledgeQuery,
    /// Escalating an issue up the hierarchy
    Escalation,
    /// Shutdown command
    Shutdown,
    /// Reference to SharedMemory entry (payload: {"key": "..."})
    MemoryRef,
    /// Custom message type
    Custom(String),
}

/// Agent identifier for message routing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AgentId {
    /// A Nydus coordinator
    Nydus(NydusId),
    /// A Queen worker agent
    Queen(QueenId),
    /// An Overlord merge validator
    Overlord(OverlordId),
    /// An Overmind strategic coordinator
    Overmind(OvermindId),
    /// The validator agent
    Validator,
    /// The human operator
    Operator,
}

// ============================================================================
// Structs
// ============================================================================

/// Controls visibility of a message to different agent types.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Visibility {
    /// Whether the message is visible to agent-level code
    pub agent_visible: bool,
    /// Whether the message is visible to coordinators (Nydus)
    pub coordinator_visible: bool,
    /// Whether the message is visible to the user/operator
    pub user_visible: bool,
}

impl Visibility {
    /// Default visibility for internal agent communication.
    /// Visible to agents and coordinators, but not to the user.
    pub fn default_internal() -> Self {
        Visibility {
            agent_visible: true,
            coordinator_visible: true,
            user_visible: false,
        }
    }
}

/// Context provided to a Queen when assigning a task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskContext {
    /// Shared knowledge base accessible to the Queen
    pub knowledge: HashMap<String, serde_json::Value>,
    /// Recent relevant messages from the swarm
    pub recent_messages: Vec<SwarmMessage>,
    /// Shared state that can be updated during execution
    pub shared_state: HashMap<String, String>,
    /// Optional hint for which skill/pattern to use (e.g., "carousel", "ralph")
    pub skill_hint: Option<String>,
    /// Formatted knowledge entries for prompt (from other Queens)
    pub knowledge_entries: Vec<String>,
    /// Information about other tasks being worked on by other Queens (for scope awareness)
    pub other_tasks_summary: Option<String>,
    /// Feedback from previous rejection(s), used to guide the Queen on retry
    pub rejection_feedback: Option<Vec<String>>,
}

/// Result of a completed task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskResult {
    /// Final status of the task
    pub status: TaskStatus,
    /// Output/result description
    pub output: String,
    /// List of artifacts produced (file paths, URLs, etc.)
    pub artifacts: Vec<String>,
    /// How long the task took to complete
    pub duration: Duration,
    /// Git commit SHA if changes were committed
    pub git_sha: Option<String>,
}

/// A task in the V2 system (separate from V1 Task).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    /// Unique identifier for this task
    pub id: TaskId,
    /// Human-readable description of the task
    pub description: String,
    /// Current status of the task
    pub status: TaskStatus,
    /// Which Queen is assigned to this task (if any)
    pub assigned_to: Option<QueenId>,
    /// Priority level (0-255, higher = more important)
    pub priority: u8,
    /// List of task IDs that must complete before this task can start
    pub blocked_by: Vec<TaskId>,
    /// When this task was created
    pub created_at: DateTime<Utc>,
}

/// A message passed between agents in the swarm.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmMessage {
    /// Unique message ID
    pub id: String,
    /// Sender agent ID
    pub from: AgentId,
    /// Recipient agent ID
    pub to: AgentId,
    /// Type of message
    pub msg_type: MessageType,
    /// Message payload (flexible JSON structure)
    pub payload: serde_json::Value,
    /// When the message was created
    pub timestamp: DateTime<Utc>,
    /// Optional correlation ID for request/response tracking
    pub correlation_id: Option<String>,
    /// Visibility settings for this message
    pub visibility: Visibility,
}

impl SwarmMessage {
    /// Create a status report message.
    pub fn status_report(from: AgentId, to: AgentId, status: QueenStatus) -> Self {
        SwarmMessage {
            id: uuid::Uuid::new_v4().to_string(),
            from,
            to,
            msg_type: MessageType::StatusReport,
            payload: serde_json::to_value(&status).unwrap_or(serde_json::Value::Null),
            timestamp: Utc::now(),
            correlation_id: None,
            visibility: Visibility::default_internal(),
        }
    }

    /// Create a task result message.
    pub fn task_result(from: AgentId, to: AgentId, task_id: TaskId, result: TaskResult) -> Self {
        let mut payload = serde_json::Map::new();
        payload.insert("task_id".to_string(), serde_json::to_value(&task_id).unwrap());
        payload.insert("result".to_string(), serde_json::to_value(&result).unwrap());

        SwarmMessage {
            id: uuid::Uuid::new_v4().to_string(),
            from,
            to,
            msg_type: MessageType::TaskResult,
            payload: serde_json::Value::Object(payload),
            timestamp: Utc::now(),
            correlation_id: None,
            visibility: Visibility::default_internal(),
        }
    }

    /// Create a knowledge-sharing message.
    pub fn knowledge(from: AgentId, to: AgentId, key: String, value: serde_json::Value) -> Self {
        let mut payload = serde_json::Map::new();
        payload.insert("key".to_string(), serde_json::Value::String(key));
        payload.insert("value".to_string(), value);

        SwarmMessage {
            id: uuid::Uuid::new_v4().to_string(),
            from,
            to,
            msg_type: MessageType::Knowledge,
            payload: serde_json::Value::Object(payload),
            timestamp: Utc::now(),
            correlation_id: None,
            visibility: Visibility::default_internal(),
        }
    }
}
