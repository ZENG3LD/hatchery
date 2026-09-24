//! Claude Code session parser — wraps the existing overseer module.

use super::{
    FileChange, FileChangeType, ParsedEvent, ParsedEventType, SessionParser, SessionSummary,
};
use crate::core::types::AgentId;
use anyhow::Result;
use std::path::PathBuf;

/// Claude Code JSONL session parser.
///
/// Wraps the existing `overseer` module to implement the SessionParser trait.
pub struct ClaudeCodeParser {
    _config: crate::overseer::analyzer::ParserConfig,
}

impl ClaudeCodeParser {
    pub fn new() -> Self {
        Self {
            _config: crate::overseer::analyzer::ParserConfig::default(),
        }
    }

    pub fn with_config(config: crate::overseer::analyzer::ParserConfig) -> Self {
        Self { _config: config }
    }
}

impl SessionParser for ClaudeCodeParser {
    fn parse_session(&self, path: &PathBuf) -> Result<Vec<ParsedEvent>> {
        let events = crate::overseer::discovery::parse_jsonl_events(path)?;
        let mut parsed = Vec::new();

        for event in events {
            let parsed_event = match &event {
                crate::overseer::events::SessionEvent::Assistant(msg) => {
                    let text = msg
                        .message
                        .content
                        .iter()
                        .filter_map(|c| match c {
                            crate::overseer::events::ContentBlock::Text(t) => Some(t.text.as_str()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("\n");

                    ParsedEvent {
                        timestamp: msg.metadata.timestamp,
                        event_type: ParsedEventType::AssistantMessage { text },
                        raw: serde_json::to_value(&event).unwrap_or_default(),
                    }
                }
                crate::overseer::events::SessionEvent::User(msg) => {
                    let text = msg
                        .message
                        .content
                        .iter()
                        .filter_map(|c| match c {
                            crate::overseer::events::ContentBlock::Text(t) => Some(t.text.as_str()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("\n");

                    ParsedEvent {
                        timestamp: msg.metadata.timestamp,
                        event_type: ParsedEventType::UserMessage { text },
                        raw: serde_json::to_value(&event).unwrap_or_default(),
                    }
                }
                crate::overseer::events::SessionEvent::System(sys) => ParsedEvent {
                    timestamp: sys.timestamp,
                    event_type: ParsedEventType::SystemEvent {
                        event: format!("{:?}", sys.subtype),
                    },
                    raw: serde_json::to_value(&event).unwrap_or_default(),
                },
                crate::overseer::events::SessionEvent::Progress(prog) => {
                    let (turns, cost) = match &prog.data {
                        crate::overseer::events::ProgressData::AgentProgress(_) => (1, 0.0),
                        _ => (0, 0.0),
                    };
                    ParsedEvent {
                        timestamp: prog.metadata.timestamp,
                        event_type: ParsedEventType::Progress {
                            turns,
                            cost_usd: cost,
                        },
                        raw: serde_json::to_value(&event).unwrap_or_default(),
                    }
                }
                _ => ParsedEvent {
                    timestamp: chrono::Utc::now(),
                    event_type: ParsedEventType::Custom("unknown".to_string()),
                    raw: serde_json::to_value(&event).unwrap_or_default(),
                },
            };
            parsed.push(parsed_event);
        }

        Ok(parsed)
    }

    fn summarize(&self, events: &[ParsedEvent], agent_id: AgentId) -> Result<SessionSummary> {
        let mut tools_used = Vec::new();
        let mut files_modified = Vec::new();
        let mut errors = Vec::new();
        let mut total_cost = 0.0;

        for event in events {
            match &event.event_type {
                ParsedEventType::ToolUse { tool_name, .. } => {
                    if !tools_used.contains(tool_name) {
                        tools_used.push(tool_name.clone());
                    }
                }
                ParsedEventType::ToolResult {
                    is_error, output, ..
                } => {
                    if *is_error {
                        errors.push(output.to_string());
                    }
                }
                ParsedEventType::Progress { cost_usd, .. } => {
                    total_cost = *cost_usd;
                }
                _ => {}
            }
        }

        // Extract file changes
        let changes = self.extract_file_changes(events)?;
        for change in &changes {
            if !files_modified.contains(&change.path) {
                files_modified.push(change.path.clone());
            }
        }

        Ok(SessionSummary {
            agent_id,
            session_id: String::new(),
            total_turns: events.len() as u32,
            total_cost_usd: total_cost,
            tools_used,
            files_modified,
            errors,
            duration_secs: 0.0,
        })
    }

    fn extract_file_changes(&self, events: &[ParsedEvent]) -> Result<Vec<FileChange>> {
        let mut changes = Vec::new();

        for event in events {
            if let ParsedEventType::ToolUse { tool_name, input } = &event.event_type {
                match tool_name.as_str() {
                    "Write" | "Edit" | "MultiEdit" => {
                        if let Some(path) = input.get("file_path").and_then(|v| v.as_str()) {
                            changes.push(FileChange {
                                path: path.to_string(),
                                change_type: if tool_name == "Write" {
                                    FileChangeType::Created
                                } else {
                                    FileChangeType::Modified
                                },
                                lines_added: 0,
                                lines_removed: 0,
                            });
                        }
                    }
                    _ => {}
                }
            }
        }

        Ok(changes)
    }

    fn format_name(&self) -> &str {
        "claude-code-jsonl"
    }
}
