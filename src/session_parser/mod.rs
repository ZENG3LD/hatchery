//! SessionParser trait — abstracts how agent session logs are parsed.
//!
//! Overseer is the Claude Code implementation. This trait allows swapping between:
//! - Claude Code JSONL parsing (via overseer)
//! - API response parsing (for direct API calls)
//! - Mock parsers for testing

use crate::core::types::AgentId;
use anyhow::Result;
use serde_json::Value;
use std::path::PathBuf;

pub mod claude_code;

/// A parsed session event.
#[derive(Debug, Clone)]
pub struct ParsedEvent {
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub event_type: ParsedEventType,
    pub raw: Value,
}

#[derive(Debug, Clone)]
pub enum ParsedEventType {
    ToolUse { tool_name: String, input: Value },
    ToolResult {
        tool_name: String,
        output: Value,
        is_error: bool,
    },
    AssistantMessage { text: String },
    UserMessage { text: String },
    SystemEvent { event: String },
    Progress { turns: u32, cost_usd: f64 },
    Custom(String),
}

/// Summary of a parsed session.
#[derive(Debug, Clone)]
pub struct SessionSummary {
    pub agent_id: AgentId,
    pub session_id: String,
    pub total_turns: u32,
    pub total_cost_usd: f64,
    pub tools_used: Vec<String>,
    pub files_modified: Vec<String>,
    pub errors: Vec<String>,
    pub duration_secs: f64,
}

/// A file change extracted from a session.
#[derive(Debug, Clone)]
pub struct FileChange {
    pub path: String,
    pub change_type: FileChangeType,
    pub lines_added: usize,
    pub lines_removed: usize,
}

#[derive(Debug, Clone)]
pub enum FileChangeType {
    Created,
    Modified,
    Deleted,
}

/// SessionParser defines how agent session logs are parsed.
pub trait SessionParser: Send + Sync {
    /// Parse a session log file into structured events.
    fn parse_session(&self, path: &PathBuf) -> Result<Vec<ParsedEvent>>;

    /// Summarize a session from its events.
    fn summarize(&self, events: &[ParsedEvent], agent_id: AgentId) -> Result<SessionSummary>;

    /// Extract file modifications from session events.
    fn extract_file_changes(&self, events: &[ParsedEvent]) -> Result<Vec<FileChange>>;

    /// Get the name/format this parser handles.
    fn format_name(&self) -> &str;
}
