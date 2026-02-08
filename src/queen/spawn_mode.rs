//! Spawn mode configuration and Claude Code NDJSON wire types.

use serde::{Deserialize, Serialize};

/// How to spawn Claude Code subprocesses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpawnMode {
    /// Long-lived subprocess using `--input-format stream-json`.
    /// One process, send NDJSON messages to stdin, read NDJSON from stdout.
    Stream,
    /// Spawn a new process per task, using `--resume <session_id>`.
    /// Each task gets a fresh process with context via session resumption.
    PerTask,
}

impl Default for SpawnMode {
    fn default() -> Self {
        Self::PerTask
    }
}

impl std::fmt::Display for SpawnMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Stream => write!(f, "stream"),
            Self::PerTask => write!(f, "per-task"),
        }
    }
}

impl std::str::FromStr for SpawnMode {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "stream" => Ok(Self::Stream),
            "per-task" | "pertask" | "per_task" => Ok(Self::PerTask),
            _ => Err(format!(
                "Unknown spawn mode: '{}'. Expected 'stream' or 'per-task'",
                s
            )),
        }
    }
}

// --- NDJSON types for stdin (Stream mode) ---

/// NDJSON message sent to Claude Code stdin in Stream mode.
#[derive(Debug, Clone, Serialize)]
pub struct StreamInput {
    #[serde(rename = "type")]
    pub msg_type: String,
    pub message: StreamMessage,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_tool_use_id: Option<String>,
}

/// Message payload for StreamInput.
#[derive(Debug, Clone, Serialize)]
pub struct StreamMessage {
    pub role: String,
    pub content: String,
}

impl StreamInput {
    /// Create a user message for task assignment.
    pub fn user_message(content: &str, session_id: Option<&str>) -> Self {
        Self {
            msg_type: "user".to_string(),
            message: StreamMessage {
                role: "user".to_string(),
                content: content.to_string(),
            },
            session_id: session_id.map(|s| s.to_string()),
            parent_tool_use_id: None,
        }
    }
}

// --- NDJSON types for stdout (both modes) ---

/// NDJSON event read from Claude Code stdout.
/// Covers all event types: system, assistant, user, result, stream_event.
#[derive(Debug, Clone, Deserialize)]
pub struct ClaudeEvent {
    /// Event type: "system", "assistant", "user", "result", "stream_event"
    #[serde(rename = "type")]
    pub event_type: String,

    /// Subtype (e.g. "init" for system, "success"/"error_*" for result)
    #[serde(default)]
    pub subtype: Option<String>,

    /// Session ID (present on system init and result events)
    #[serde(default)]
    pub session_id: Option<String>,

    /// Message content (present on assistant and user events)
    #[serde(default)]
    pub message: Option<serde_json::Value>,

    /// Final result text (present on result events)
    #[serde(default)]
    pub result: Option<String>,

    /// Total API cost in USD (present on result events)
    #[serde(default)]
    pub cost_usd: Option<f64>,

    /// Total cost field (alternative name used in some versions)
    #[serde(default)]
    pub total_cost_usd: Option<f64>,

    /// Duration in milliseconds (present on result events)
    #[serde(default)]
    pub duration_ms: Option<u64>,

    /// Number of agent turns (present on result events)
    #[serde(default)]
    pub num_turns: Option<u32>,

    /// Whether this is an error result
    #[serde(default)]
    pub is_error: Option<bool>,

    /// Model used (present on system init)
    #[serde(default)]
    pub model: Option<String>,

    /// Structured output (when --json-schema is used)
    #[serde(default)]
    pub structured_output: Option<serde_json::Value>,

    /// Compact metadata (present on compact_boundary system events)
    #[serde(default)]
    pub compact_metadata: Option<serde_json::Value>,
}

impl ClaudeEvent {
    /// Check if this is the final result event.
    pub fn is_result(&self) -> bool {
        self.event_type == "result"
    }

    /// Check if this is a system init event.
    pub fn is_system_init(&self) -> bool {
        self.event_type == "system" && self.subtype.as_deref() == Some("init")
    }

    /// Check if this is an assistant message.
    pub fn is_assistant(&self) -> bool {
        self.event_type == "assistant"
    }

    /// Check if this result indicates success.
    pub fn is_success(&self) -> bool {
        self.is_result() && self.subtype.as_deref() == Some("success")
    }

    /// Get the cost (tries both field names).
    pub fn cost(&self) -> Option<f64> {
        self.cost_usd.or(self.total_cost_usd)
    }

    /// Extract tool use blocks from an assistant message.
    pub fn tool_uses(&self) -> Vec<serde_json::Value> {
        self.message
            .as_ref()
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_array())
            .map(|arr| {
                arr.iter()
                    .filter(|block| block.get("type").and_then(|t| t.as_str()) == Some("tool_use"))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Check if this is a compact_boundary system event.
    pub fn is_compact_boundary(&self) -> bool {
        self.event_type == "system" && self.subtype.as_deref() == Some("compact_boundary")
    }

    /// Extract pre_tokens from compact_metadata.
    pub fn pre_tokens(&self) -> Option<u64> {
        self.compact_metadata
            .as_ref()
            .and_then(|m| m.get("preTokens"))
            .and_then(|v| v.as_u64())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spawn_mode_parse() {
        assert_eq!("stream".parse::<SpawnMode>().unwrap(), SpawnMode::Stream);
        assert_eq!("per-task".parse::<SpawnMode>().unwrap(), SpawnMode::PerTask);
        assert_eq!("per_task".parse::<SpawnMode>().unwrap(), SpawnMode::PerTask);
        assert!("invalid".parse::<SpawnMode>().is_err());
    }

    #[test]
    fn test_stream_input_serialization() {
        let input = StreamInput::user_message("Hello", Some("sess-123"));
        let json = serde_json::to_string(&input).unwrap();
        assert!(json.contains("\"type\":\"user\""));
        assert!(json.contains("\"role\":\"user\""));
        assert!(json.contains("\"content\":\"Hello\""));
        assert!(json.contains("\"session_id\":\"sess-123\""));
    }

    #[test]
    fn test_stream_input_no_session() {
        let input = StreamInput::user_message("Hi", None);
        let json = serde_json::to_string(&input).unwrap();
        assert!(!json.contains("session_id"));
    }

    #[test]
    fn test_claude_event_result() {
        let json = r#"{"type":"result","subtype":"success","session_id":"abc-123","result":"Done","cost_usd":0.15,"duration_ms":5000,"num_turns":3}"#;
        let event: ClaudeEvent = serde_json::from_str(json).unwrap();
        assert!(event.is_result());
        assert!(event.is_success());
        assert_eq!(event.session_id.as_deref(), Some("abc-123"));
        assert_eq!(event.result.as_deref(), Some("Done"));
        assert_eq!(event.cost(), Some(0.15));
        assert_eq!(event.duration_ms, Some(5000));
        assert_eq!(event.num_turns, Some(3));
    }

    #[test]
    fn test_claude_event_system_init() {
        let json = r#"{"type":"system","subtype":"init","session_id":"sess-456","model":"claude-sonnet-4-5-20250929"}"#;
        let event: ClaudeEvent = serde_json::from_str(json).unwrap();
        assert!(event.is_system_init());
        assert!(!event.is_result());
        assert_eq!(event.model.as_deref(), Some("claude-sonnet-4-5-20250929"));
    }

    #[test]
    fn test_claude_event_assistant() {
        let json = r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"Hello!"}]}}"#;
        let event: ClaudeEvent = serde_json::from_str(json).unwrap();
        assert!(event.is_assistant());
        assert!(event.tool_uses().is_empty());
    }

    #[test]
    fn test_claude_event_tool_use() {
        let json = r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"Let me read that."},{"type":"tool_use","id":"toolu_123","name":"Read","input":{"file_path":"/test.rs"}}]}}"#;
        let event: ClaudeEvent = serde_json::from_str(json).unwrap();
        let tools = event.tool_uses();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], "Read");
    }

    #[test]
    fn test_claude_event_total_cost_usd() {
        let json = r#"{"type":"result","subtype":"success","total_cost_usd":2.50}"#;
        let event: ClaudeEvent = serde_json::from_str(json).unwrap();
        assert_eq!(event.cost(), Some(2.50));
    }

    #[test]
    fn test_claude_event_error_result() {
        let json =
            r#"{"type":"result","subtype":"error_max_turns","is_error":true,"num_turns":20}"#;
        let event: ClaudeEvent = serde_json::from_str(json).unwrap();
        assert!(event.is_result());
        assert!(!event.is_success());
        assert_eq!(event.is_error, Some(true));
    }

    #[test]
    fn test_compact_boundary_detection() {
        let json = r#"{"type":"system","subtype":"compact_boundary","compact_metadata":{"preTokens":150000,"postTokens":50000,"compressionRatio":0.33}}"#;
        let event: ClaudeEvent = serde_json::from_str(json).unwrap();
        assert!(event.is_compact_boundary());
        assert_eq!(event.pre_tokens(), Some(150000));
    }

    #[test]
    fn test_compact_boundary_detection_missing_metadata() {
        let json = r#"{"type":"system","subtype":"compact_boundary"}"#;
        let event: ClaudeEvent = serde_json::from_str(json).unwrap();
        assert!(event.is_compact_boundary());
        assert_eq!(event.pre_tokens(), None);
    }
}
