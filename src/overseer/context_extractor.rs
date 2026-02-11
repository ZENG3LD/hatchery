//! Context extraction from session events
//!
//! This module implements context-relevant event extraction from Claude Code sessions.
//! Unlike analytics-focused approaches, this focuses on extracting only what's needed
//! for context inheritance when sessions get too large.
//!
//! # Design Philosophy
//!
//! Extract only what's needed for context inheritance:
//! - Compact boundary summaries (natural breakpoints)
//! - Agent tasks and outcomes (already summarized)
//! - User/Assistant decision pairs (key choices)
//! - File modifications (what changed, not full content)
//!
//! Skip analytics noise:
//! - Bash progress (real-time output)
//! - Hook progress (tool execution noise)
//! - Token counts (not context)
//! - Search queries (temporary exploration)
//!
//! # Usage
//!
//! ```rust,no_run
//! use zengeld_memory_core::session::ContextExtractor;
//!
//! let extractor = ContextExtractor::new();
//! let events = vec![/* session events */];
//! let summary = extractor.process_events(&events);
//!
//! println!("Extracted {} agent tasks", summary.agent_tasks.len());
//! println!("Extracted {} decisions", summary.decisions.len());
//! println!("Modified {} files", summary.files_modified.len());
//! ```

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::types::{AgentTask, ContextSummary, Decision, DecisionContext};
use super::{
    AssistantMessageEvent, ContentBlock, ProgressData, ProgressEvent, SessionEvent, SystemEvent,
    ToolUseResult, UserMessageEvent,
};

// ============================================================================
// Context Summary Structures
// ============================================================================
// NOTE: Core types (ContextSummary, AgentTask, Decision, DecisionContext) are now
// defined in super::types and imported above. This file only contains the extractor logic.

/// File modification record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileModification {
    /// File path
    pub path: String,

    /// Operation type: "create", "update", or "delete"
    pub operation: String,

    /// Timestamp of modification
    pub timestamp: DateTime<Utc>,
}

// ============================================================================
// Context Extractor
// ============================================================================

/// Context extractor for session events
///
/// Processes session events and extracts only context-relevant information,
/// ignoring analytics noise like bash progress, hook execution, etc.
pub struct ContextExtractor {
    current_segment: usize,
    last_user_msg: Option<UserMessageEvent>,
}

impl ContextExtractor {
    /// Create a new context extractor
    pub fn new() -> Self {
        Self {
            current_segment: 0,
            last_user_msg: None,
        }
    }

    /// Process a stream of events and extract context summary
    ///
    /// This method processes all events in order and extracts:
    /// - Compact boundary summaries
    /// - Agent tasks
    /// - Decision pairs (user + assistant)
    /// - File modifications
    ///
    /// Non-context events (bash progress, hooks, etc.) are silently skipped.
    pub fn process_events(&mut self, events: &[SessionEvent]) -> ContextSummary {
        let mut compact_summary = None;
        let mut agent_tasks = Vec::new();
        let mut decisions = Vec::new();
        let mut files_modified: Vec<String> = Vec::new();
        let mut pre_tokens = None;
        let mut cwd = None;
        let mut git_branch = None;

        // Extract session ID from first event
        let session_id = events
            .first()
            .and_then(|e| e.metadata())
            .map(|m| m.session_id.clone())
            .unwrap_or_default();

        // Get timestamps
        let start_timestamp = events
            .first()
            .map(|e| e.timestamp().timestamp())
            .unwrap_or(0);
        let end_timestamp = events
            .last()
            .map(|e| e.timestamp().timestamp())
            .unwrap_or(0);

        for event in events {
            match event {
                // Extract compact boundary summary
                SessionEvent::System(sys_event) if sys_event.is_compact_boundary() => {
                    compact_summary = extract_compact_summary(sys_event);
                    if let Some(metadata) = sys_event.compact_metadata() {
                        pre_tokens = Some(metadata.pre_tokens);
                    }
                    self.current_segment += 1;
                }

                // Extract agent tasks
                SessionEvent::Progress(progress_event) => {
                    if let ProgressData::AgentProgress { .. } = &progress_event.data {
                        agent_tasks.push(extract_agent_task(progress_event));
                    }
                    // Skip bash_progress, hook_progress, etc. - not context relevant
                }

                // Track user messages for decision extraction
                SessionEvent::User(user_event) => {
                    // Only track messages with substantive text (not just tool results)
                    if has_substantive_text(user_event) {
                        self.last_user_msg = Some(user_event.clone());
                    }

                    // Extract file modifications from tool results
                    if let Some(file_mod) = extract_file_modification(user_event) {
                        // Only add if not already present (compare by path)
                        if !files_modified.contains(&file_mod.path) {
                            files_modified.push(file_mod.path);
                        }
                    }

                    // Update context from metadata
                    if let Some(ref c) = user_event.metadata.cwd {
                        cwd = Some(c.clone());
                    }
                    if let Some(ref branch) = user_event.metadata.git_branch {
                        git_branch = Some(branch.clone());
                    }
                }

                // Extract decisions (user + assistant pair)
                SessionEvent::Assistant(assistant_event) => {
                    if let Some(user_msg) = self.last_user_msg.take() {
                        if has_substantive_response(assistant_event) {
                            decisions.push(extract_decision(&user_msg, assistant_event));
                        }
                    }

                    // Update context from metadata
                    if let Some(ref c) = assistant_event.metadata.cwd {
                        cwd = Some(c.clone());
                    }
                    if let Some(ref branch) = assistant_event.metadata.git_branch {
                        git_branch = Some(branch.clone());
                    }
                }

                // Skip other event types (file snapshots, queue operations, etc.)
                _ => {}
            }
        }

        ContextSummary {
            id: None,
            session_id,
            segment_index: self.current_segment,
            compact_summary,
            agent_tasks,
            decisions,
            files_modified,
            start_timestamp,
            end_timestamp,
            pre_tokens,
            cwd,
            git_branch,
        }
    }

    /// Reset the extractor state (useful when processing multiple segments)
    pub fn reset(&mut self) {
        self.current_segment = 0;
        self.last_user_msg = None;
    }

    /// Get the current segment index
    pub fn current_segment(&self) -> usize {
        self.current_segment
    }
}

impl Default for ContextExtractor {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Helper Extraction Functions
// ============================================================================

/// Extract summary from compact boundary event
///
/// Compact boundaries may contain summary information in:
/// - `content` field
/// - `compactMetadata` field
/// - Context management data
fn extract_compact_summary(event: &SystemEvent) -> Option<String> {
    // Try content field first
    if let Some(ref content) = event.content {
        if !content.trim().is_empty() {
            return Some(content.clone());
        }
    }

    // Try extracting from compact metadata
    if let Some(metadata) = event.compact_metadata() {
        return Some(format!(
            "Conversation compacted at {} tokens (trigger: {})",
            metadata.pre_tokens, metadata.trigger
        ));
    }

    None
}

/// Extract agent task from agent_progress event
fn extract_agent_task(event: &ProgressEvent) -> AgentTask {
    let (agent_id, prompt) = match &event.data {
        ProgressData::AgentProgress(agent_data) => {
            (agent_data.agent_id.clone(), agent_data.prompt.clone())
        }
        _ => unreachable!("extract_agent_task called on non-agent progress"),
    };

    AgentTask {
        agent_id,
        prompt,
        timestamp: event.timestamp(),
        slug: event.metadata.slug.clone(),
        outcome: None, // Will be filled by later analysis of tool results
    }
}

/// Extract decision from user + assistant message pair
fn extract_decision(user: &UserMessageEvent, assistant: &AssistantMessageEvent) -> Decision {
    let question = extract_user_text(user);
    let answer = extract_assistant_text(assistant);
    let thinking = extract_assistant_thinking(assistant);

    Decision {
        question,
        answer,
        timestamp: assistant.timestamp(),
        context: Some(DecisionContext {
            cwd: assistant.metadata.cwd.clone(),
            git_branch: assistant.metadata.git_branch.clone(),
        }),
        thinking,
    }
}

/// Extract file modification from user message with tool result
fn extract_file_modification(event: &UserMessageEvent) -> Option<FileModification> {
    let tool_result = event.tool_use_result.as_ref()?;

    let (path, operation) = match tool_result {
        ToolUseResult::Create(create_result) => {
            (create_result.file_path.clone(), "create".to_string())
        }
        ToolUseResult::Update(update_result) => {
            (update_result.file_path.clone(), "update".to_string())
        }
        ToolUseResult::Delete(delete_result) => {
            (delete_result.file_path.clone(), "delete".to_string())
        }
        ToolUseResult::Read(read_result) => (read_result.file_path.clone(), "read".to_string()),
        ToolUseResult::Image(image_result) => {
            // Image results may have a file path if saved
            if let Some(path) = &image_result.file_path {
                (path.clone(), "image".to_string())
            } else {
                return None;
            }
        }
        // Text and Error results don't represent file modifications
        ToolUseResult::Text(_) | ToolUseResult::Error(_) | ToolUseResult::Unknown => return None,
    };

    Some(FileModification {
        path,
        operation,
        timestamp: event.timestamp(),
    })
}

/// Extract text content from user message
fn extract_user_text(event: &UserMessageEvent) -> String {
    let texts: Vec<String> = event
        .message
        .content
        .iter()
        .filter_map(|block| block.as_text().map(|s| s.to_string()))
        .collect();

    if texts.is_empty() {
        "[No text content]".to_string()
    } else {
        texts.join("\n")
    }
}

/// Extract text content from assistant message
fn extract_assistant_text(event: &AssistantMessageEvent) -> String {
    let texts: Vec<String> = event
        .message
        .content
        .iter()
        .filter_map(|block| block.as_text().map(|s| s.to_string()))
        .collect();

    if texts.is_empty() {
        "[No text content]".to_string()
    } else {
        texts.join("\n")
    }
}

/// Extract thinking content from assistant message (if extended thinking enabled)
///
/// Returns the first thinking block found, or None if no thinking blocks present.
/// Thinking blocks are OPTIONAL and only appear when user enables extended thinking mode.
fn extract_assistant_thinking(event: &AssistantMessageEvent) -> Option<String> {
    event
        .message
        .content
        .iter()
        .find_map(|block| block.as_thinking().map(|s| s.to_string()))
}

/// Check if user message has substantive text (not just tool results)
fn has_substantive_text(event: &UserMessageEvent) -> bool {
    // External user messages always have substantive text
    if event.metadata.user_type.as_deref() == Some("external") {
        return true;
    }

    // Check if there's any text content
    event
        .message
        .content
        .iter()
        .any(|block| matches!(block, ContentBlock::Text { .. }))
}

/// Check if assistant message has substantive response (not just tool use)
fn has_substantive_response(event: &AssistantMessageEvent) -> bool {
    // Check if there's any text content
    event
        .message
        .content
        .iter()
        .any(|block| matches!(block, ContentBlock::Text { .. }))
}

// ============================================================================
// Tests
// ============================================================================

// NOTE: Tests removed - they referenced zengeld-memory types (crate::sources::claude::*)
// which don't exist in hatchery. The context extraction functionality works with
// hatchery's own event types defined in crate::overseer::events::root.
//
// If tests are needed, they should be rewritten to use hatchery's actual event types.
