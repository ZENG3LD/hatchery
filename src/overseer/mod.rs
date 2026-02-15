//! Claude Code session parsing and processing
//!
//! This module contains Claude-specific types and parsing logic for handling
//! Claude Code JSONL session files.
//!
//! Copied from zengeld-memory with database dependencies removed.

pub mod analyzer;
pub mod context_extractor;
pub mod discovery;
pub mod error;
pub mod events;
pub mod types;

// Re-export error types
pub use error::{ParseError, Result};

// Re-export Claude-specific types
pub use types::{
    AgentActivity, AgentSummary, AgentTask, ContextSummary, ConversationEdge, Decision,
    DecisionContext, FileChange, SessionSegment, SessionWithSubagents, SubagentSession,
    ToolActivity,
};

// Re-export typed event structures
pub use events::{
    AssistantMessage, AssistantMessageEvent, CompactMetadata, ContentBlock, EventMetadata,
    FileHistorySnapshot, MessageContent, ProgressData, ProgressEvent, QueueOperationEvent,
    SessionEvent, Snapshot, SystemEvent, TokenUsage, ToolUseResult, ToolUseResultMetadata,
    UserMessageEvent,
};

// Re-export context extraction types and extractor
pub use context_extractor::{ContextExtractor, FileModification};

// Re-export discovery functions
pub use discovery::{
    extract_session_id, find_session_files, find_subagent_files, parse_jsonl_events,
    parse_session_with_subagents,
};

// Re-export analyzer types and functions
pub use analyzer::{
    discover_segments, ImportStats, ParsedSegment, ParserConfig, SegmentBoundary, SegmentParser,
    SegmentStats,
};
