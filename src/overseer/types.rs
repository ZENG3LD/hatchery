//! Claude-specific types for session parsing and context extraction
//!
//! This module contains all Claude Code specific data structures that are not
//! applicable to other AI session sources (Codex, Gemini, etc.).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// ============================================================================
// Subagent Session Types
// ============================================================================

/// Parsed subagent session
///
/// Represents a complete subagent session (Task tool execution) with all
/// events, metadata, and linked to parent session.
#[derive(Debug, Clone)]
pub struct SubagentSession {
    /// Agent ID (7-char hex, e.g., "a18af05")
    pub agent_id: String,

    /// Parent session ID (same as main session)
    pub session_id: String,

    /// Path to subagent JSONL file
    pub jsonl_path: PathBuf,

    /// Subagent type (if extracted from Task input)
    pub subagent_type: Option<String>,

    /// Model used by subagent
    pub model: String,

    /// Spawn timestamp (Unix epoch)
    pub spawn_timestamp: Option<i64>,

    /// Complete timestamp (Unix epoch)
    pub complete_timestamp: Option<i64>,

    /// Total tokens from toolUseResult metadata
    pub total_tokens: u64,

    /// Total tool use count from toolUseResult metadata
    pub total_tool_use_count: u64,

    /// Total duration in milliseconds from toolUseResult metadata
    pub total_duration_ms: u64,

    /// All events from subagent JSONL
    pub events: Vec<crate::overseer::events::root::SessionEvent>,
}

/// Main session with linked subagents
///
/// Combines a main session with all its subagent sessions,
/// providing a complete view of the work performed.
#[derive(Debug)]
pub struct SessionWithSubagents {
    /// Session ID
    pub session_id: String,

    /// Path to main session JSONL
    pub jsonl_path: PathBuf,

    /// All events from main session
    pub events: Vec<crate::overseer::events::root::SessionEvent>,

    /// Linked subagent sessions
    pub subagents: Vec<SubagentSession>,
}

// ============================================================================
// Session Segment Types
// ============================================================================

/// Session segment bounded by compact boundaries
///
/// A segment represents a portion of a Claude Code session between two compact
/// boundary events. Claude automatically creates compact boundaries when the
/// context window gets too large (~150k-160k tokens).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSegment {
    /// Unique segment ID (auto-generated in DB)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<i64>,

    /// Session this segment belongs to
    pub session_id: String,

    /// Segment index in chronological order (0, 1, 2...)
    pub segment_index: i32,

    /// Unix timestamp when segment started
    pub start_timestamp: i64,

    /// Unix timestamp when segment ended
    pub end_timestamp: i64,

    /// UUID of first event in segment
    pub start_uuid: String,

    /// UUID of compact_boundary event
    pub end_uuid: String,

    /// Token count before compaction (~150k-160k)
    pub pre_tokens: Option<i64>,

    /// Compaction trigger type ('auto' or 'manual')
    pub trigger: Option<String>,

    /// Message counts
    pub message_count: i32,
    pub user_message_count: i32,
    pub assistant_message_count: i32,
    pub tool_use_count: i32,

    /// Aggregated token usage
    pub total_input_tokens: i64,
    pub total_output_tokens: i64,
    pub total_cache_write_tokens: i64,
    pub total_cache_read_tokens: i64,
    pub estimated_cost_usd: f64,

    /// Context snapshot at segment boundary
    pub cwd: Option<String>,
    pub git_branch: Option<String>,
    pub claude_version: Option<String>,
}

// ============================================================================
// Agent Activity Types
// ============================================================================

/// Agent activity record
///
/// Represents a subagent spawned during a Claude Code session. Agents are
/// autonomous workers that can be delegated tasks to work in parallel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentActivity {
    /// Unique activity ID (auto-generated in DB)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<i64>,

    /// Session this activity belongs to
    pub session_id: String,

    /// Segment ID (foreign key)
    pub segment_id: i64,

    /// Short agent ID (e.g., "afbe84d")
    pub agent_id: String,

    /// Agent slug (e.g., "pure-fluttering-pearl")
    pub agent_slug: Option<String>,

    /// Agent task prompt
    pub prompt: String,

    /// Unix timestamp when agent was spawned
    pub spawn_timestamp: i64,

    /// UUID of spawn event
    pub spawn_uuid: String,

    /// Parent tool use ID that spawned this agent
    pub parent_tool_use_id: Option<String>,

    /// UUID of result event
    pub result_uuid: Option<String>,

    /// Unix timestamp of result
    pub result_timestamp: Option<i64>,

    /// Whether the agent succeeded
    pub success: Option<bool>,

    /// Error message if failed
    pub error_message: Option<String>,

    /// Path to subagent file (if available)
    pub subagent_file: Option<String>,

    /// Token usage (if parsed from subagent file)
    pub total_input_tokens: Option<i64>,
    pub total_output_tokens: Option<i64>,
    pub estimated_cost_usd: Option<f64>,
}

// ============================================================================
// Tool Activity Types
// ============================================================================

/// Tool activity record
///
/// Represents a tool invocation (Read, Write, Edit, Bash, etc.) during a
/// Claude Code session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolActivity {
    /// Unique activity ID (auto-generated in DB)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<i64>,

    /// Session this activity belongs to
    pub session_id: String,

    /// Segment ID (foreign key)
    pub segment_id: i64,

    /// Unique tool use ID (e.g., "toolu_01...")
    pub tool_use_id: String,

    /// Tool name (Bash, Read, Write, Edit, etc.)
    pub tool_name: String,

    /// Unix timestamp when tool was invoked
    pub invocation_timestamp: i64,

    /// UUID of invocation event
    pub invocation_uuid: String,

    /// Unix timestamp of result
    pub result_timestamp: Option<i64>,

    /// UUID of result event
    pub result_uuid: Option<String>,

    /// Duration in milliseconds
    pub duration_ms: Option<i64>,

    /// Tool parameters (JSON serialized)
    pub parameters: Option<String>,

    /// Result type ('success', 'error', 'timeout')
    pub result_type: Option<String>,

    /// First 500 chars of result
    pub result_summary: Option<String>,

    /// File path (if tool modified a file)
    pub file_path: Option<String>,

    /// Operation type ('create', 'update', 'read', 'delete')
    pub operation_type: Option<String>,
}

// ============================================================================
// File Change Types
// ============================================================================

/// File change record
///
/// Represents a file modification made during a Claude Code session through
/// tool usage (Write, Edit, etc.).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileChange {
    /// Unique change ID (auto-generated in DB)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<i64>,

    /// Session this change belongs to
    pub session_id: String,

    /// Segment ID (foreign key)
    pub segment_id: i64,

    /// Tool activity that made the change (optional foreign key)
    pub tool_activity_id: Option<i64>,

    /// File path
    pub file_path: String,

    /// Operation type ('create', 'update', 'delete')
    pub operation: String,

    /// Unix timestamp when change occurred
    pub timestamp: i64,

    /// UUID of message that made the change
    pub message_uuid: String,

    /// SHA256 hash of previous content
    pub previous_content_hash: Option<String>,

    /// SHA256 hash of new content
    pub new_content_hash: Option<String>,

    /// File size in bytes
    pub size_bytes: Option<i64>,
}

// ============================================================================
// Conversation Graph Types
// ============================================================================

/// Conversation graph edge
///
/// Represents a node in the conversation graph, with parent relationships
/// for building the conversation tree structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationEdge {
    /// UUID of this message
    pub uuid: String,

    /// UUID of parent message
    pub parent_uuid: Option<String>,

    /// UUID of logical parent (for compact boundaries)
    pub logical_parent_uuid: Option<String>,

    /// Event type ('user', 'assistant', 'progress', 'system')
    pub event_type: String,

    /// Unix timestamp
    pub timestamp: i64,

    /// Session ID
    pub session_id: String,

    /// Segment ID (optional foreign key)
    pub segment_id: Option<i64>,

    /// Whether this is a sidechain (agent session)
    pub is_sidechain: bool,
}

// ============================================================================
// Context Extraction Types
// ============================================================================

/// Context summary for a session segment
///
/// Contains extracted context from a segment that's suitable for inclusion
/// when starting a new session or providing context to an AI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextSummary {
    /// Unique summary ID (auto-generated in DB)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<i64>,

    /// Session this summary belongs to
    pub session_id: String,

    /// Segment index
    pub segment_index: usize,

    /// Extracted context from compact boundary
    pub compact_summary: Option<String>,

    /// Agent tasks in this segment
    pub agent_tasks: Vec<AgentTask>,

    /// Key decisions in this segment
    pub decisions: Vec<Decision>,

    /// Files modified in this segment
    pub files_modified: Vec<String>,

    /// Segment start timestamp
    pub start_timestamp: i64,

    /// Segment end timestamp
    pub end_timestamp: i64,

    /// Token count before compaction (if segment ended with compact boundary)
    pub pre_tokens: Option<u64>,

    /// Working directory context
    pub cwd: Option<String>,

    /// Git branch context
    pub git_branch: Option<String>,
}

/// Agent task extracted for context
///
/// Represents a delegated task to a subagent, suitable for including in
/// context summaries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTask {
    /// Agent ID
    pub agent_id: String,

    /// Task prompt/description
    pub prompt: String,

    /// Timestamp when agent was spawned
    pub timestamp: DateTime<Utc>,

    /// Agent slug (for finding subagent file)
    pub slug: Option<String>,

    /// Task outcome (if available)
    pub outcome: Option<String>,
}

/// Decision point in session
///
/// Represents a key user question and Claude's answer, suitable for
/// including in context summaries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Decision {
    /// User question/prompt
    pub question: String,

    /// Assistant answer/response
    pub answer: String,

    /// Timestamp of the decision
    pub timestamp: DateTime<Utc>,

    /// Context (cwd, git branch, etc.)
    pub context: Option<DecisionContext>,

    /// Thinking/reasoning process (optional - only when extended thinking enabled)
    ///
    /// Contains Claude's full reasoning process if extended thinking mode was enabled
    /// for this session. This is OPTIONAL and will be None for most sessions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<String>,
}

/// Context information for a decision
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionContext {
    pub cwd: Option<String>,
    pub git_branch: Option<String>,
}

/// Summary of agent activity extracted from session events
///
/// Higher-level summary of agent work, typically generated by AI analysis
/// of the raw agent activity records.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSummary {
    /// Name of the agent
    pub agent_name: String,

    /// What the agent worked on
    pub tasks_performed: Vec<String>,

    /// Key outcomes or deliverables
    pub outcomes: Vec<String>,
}

