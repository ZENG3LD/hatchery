//! Session file parsing and discovery logic

use chrono::{DateTime, Utc};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::infestation_pit::error::{ParseError, Result};
use crate::infestation_pit::events::root::SessionEvent;

// ============================================================================
// Local Types (no dependency on crate::types)
// ============================================================================

/// Session file metadata
///
/// Local struct to avoid dependency on crate::types module.
#[derive(Debug, Clone)]
pub struct SessionFileInfo {
    /// Full path to the session file
    pub path: PathBuf,

    /// File size in bytes
    pub size_bytes: u64,

    /// Last modification time
    pub modified: Option<DateTime<Utc>>,

    /// Session ID (extracted from filename)
    pub session_id: Option<String>,
}

/// Find all session JSONL files in a Claude Code project directory
///
/// This function walks the `.claude/projects/[encoded-path]/` directory
/// structure and collects all `.jsonl` session files with their metadata.
///
/// # Arguments
///
/// * `project_path` - Path to the project root (e.g., `C:\Users\...\nemo`)
///
/// # Returns
///
/// A vector of `SessionFileInfo` structs containing file paths, sizes,
/// modification times, and extracted session IDs.
pub fn find_session_files(_project_path: &Path) -> Result<Vec<SessionFileInfo>> {
    // Get Claude data directory (typically ~/.claude)
    let claude_dir = get_claude_data_dir()?;

    // Sessions are in ~/.claude/projects/[encoded-path]/*.jsonl
    let projects_dir = claude_dir.join("projects");

    if !projects_dir.exists() {
        tracing::warn!(
            "Claude projects directory does not exist: {:?}",
            projects_dir
        );
        return Ok(Vec::new());
    }

    let mut files = Vec::new();

    // Walk all project directories
    let project_entries = fs::read_dir(&projects_dir).map_err(|e| {
        ParseError::Io(std::io::Error::new(
            e.kind(),
            format!(
                "Failed to read projects directory {projects_dir:?}: {e}"
            ),
        ))
    })?;

    for project_entry in project_entries.flatten() {
        let project_path = project_entry.path();
        if !project_path.is_dir() {
            continue;
        }

        // Walk all .jsonl files in this project directory
        if let Ok(session_entries) = fs::read_dir(&project_path) {
            for entry in session_entries.flatten() {
                let path = entry.path();

                // Check if file has .jsonl extension
                if path.extension().is_none_or(|e| e != "jsonl") {
                    continue;
                }

                // Get file metadata
                if let Ok(metadata) = fs::metadata(&path) {
                    let size_bytes = metadata.len();
                    let modified = metadata
                        .modified()
                        .ok()
                        .and_then(|st| {
                            st.duration_since(SystemTime::UNIX_EPOCH)
                                .ok()
                                .and_then(|d| DateTime::from_timestamp(d.as_secs() as i64, 0))
                        });

                    let session_id = Some(extract_session_id(&path));

                    files.push(SessionFileInfo {
                        path,
                        size_bytes,
                        modified,
                        session_id,
                    });
                }
            }
        }
    }

    tracing::info!(
        "Found {} Claude JSONL session files in {:?}",
        files.len(),
        projects_dir
    );

    Ok(files)
}

/// Parse JSONL events from a session file
///
/// Reads a session JSONL file line by line and deserializes each line
/// into a `SessionEvent` struct.
///
/// # Arguments
///
/// * `session_path` - Path to the `.jsonl` session file
///
/// # Returns
///
/// A vector of `SessionEvent` structs, one per line in the file.
///
/// # Errors
///
/// Returns an error if:
/// - The file cannot be opened
/// - A line cannot be parsed as valid JSON
/// - JSON structure doesn't match expected event format
pub fn parse_jsonl_events(session_path: &Path) -> Result<Vec<SessionEvent>> {
    let file = fs::File::open(session_path).map_err(|e| {
        ParseError::Io(std::io::Error::new(
            e.kind(),
            format!("Failed to open session file {session_path:?}: {e}"),
        ))
    })?;

    let reader = BufReader::new(file);
    let mut events = Vec::new();

    for (line_num, line_result) in reader.lines().enumerate() {
        let line = line_result.map_err(|e| {
            ParseError::Io(std::io::Error::new(
                e.kind(),
                format!(
                    "Failed to read line {} from {:?}: {}",
                    line_num + 1,
                    session_path,
                    e
                ),
            ))
        })?;

        // Skip empty lines
        if line.trim().is_empty() {
            continue;
        }

        // Parse JSON
        let event: SessionEvent = serde_json::from_str(&line).map_err(|e| {
            ParseError::InvalidSession(format!(
                "Failed to parse line {} from {:?}: {}",
                line_num + 1,
                session_path,
                e
            ))
        })?;

        events.push(event);
    }

    tracing::debug!("Parsed {} events from {:?}", events.len(), session_path);

    Ok(events)
}

/// Extract session ID from a session file path
///
/// The session ID is the filename without the `.jsonl` extension.
///
/// # Arguments
///
/// * `path` - Path to the session file
///
/// # Returns
///
/// The session ID as a string. If extraction fails, returns "unknown".
///
/// # Examples
///
/// ```
/// # use std::path::Path;
/// # use zengeld_memory_core::sources::claude::extract_session_id;
/// let path = Path::new("C:\\Users\\User\\.claude\\projects\\abc123\\session_12345.jsonl");
/// assert_eq!(extract_session_id(path), "session_12345");
/// ```
pub fn extract_session_id(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .map(String::from)
        .unwrap_or_else(|| "unknown".to_string())
}

/// Get Claude Code data directory
///
/// Checks for:
/// 1. `CLAUDE_HOME` environment variable
/// 2. Standard location: `~/.claude`
///
/// # Errors
///
/// Returns an error if no data directory can be determined.
fn get_claude_data_dir() -> Result<std::path::PathBuf> {
    // Check CLAUDE_HOME env var first
    if let Ok(claude_home) = std::env::var("CLAUDE_HOME") {
        return Ok(std::path::PathBuf::from(claude_home));
    }

    // Standard Claude Code location
    dirs::home_dir()
        .map(|h| h.join(".claude"))
        .ok_or_else(|| ParseError::Other("Cannot determine home directory".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::PathBuf;
    use tempfile::NamedTempFile;

    #[test]
    fn test_extract_session_id() {
        let path = PathBuf::from("C:\\Users\\User\\.claude\\projects\\abc\\session_123.jsonl");
        assert_eq!(extract_session_id(&path), "session_123");

        let path = PathBuf::from("/home/user/.claude/projects/xyz/chat-2024.jsonl");
        assert_eq!(extract_session_id(&path), "chat-2024");

        let path = PathBuf::from("unknown");
        assert_eq!(extract_session_id(&path), "unknown");
    }

    #[test]
    fn test_extract_session_id_windows_path() {
        let path =
            PathBuf::from("C:\\Users\\VA PC\\.claude\\projects\\encoded\\session_abc123.jsonl");
        assert_eq!(extract_session_id(&path), "session_abc123");
    }

    #[test]
    fn test_parse_jsonl_events_nonexistent() {
        let nonexistent = PathBuf::from("/nonexistent/file.jsonl");
        let result = parse_jsonl_events(&nonexistent);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_jsonl_events_empty_file() {
        let temp_file = NamedTempFile::new().unwrap();

        // Parse empty file
        let events = parse_jsonl_events(temp_file.path()).unwrap();
        assert_eq!(events.len(), 0);
    }

    #[test]
    fn test_parse_jsonl_events_single_event() {
        let mut temp_file = NamedTempFile::new().unwrap();

        // Write a single event
        writeln!(
            temp_file,
            r#"{{"type":"userMessage","text":"Hello","timestamp":"2024-01-24T10:00:00Z"}}"#
        )
        .unwrap();
        temp_file.flush().unwrap();

        // Parse the file
        let events = parse_jsonl_events(temp_file.path()).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "userMessage");
        assert_eq!(events[0].text(), Some("Hello"));
    }

    #[test]
    fn test_parse_jsonl_events_multiple_events() {
        let mut temp_file = NamedTempFile::new().unwrap();

        // Write multiple events
        writeln!(temp_file, r#"{{"type":"userMessage","text":"Hello"}}"#).unwrap();
        writeln!(
            temp_file,
            r#"{{"type":"assistantMessage","content":"Hi there"}}"#
        )
        .unwrap();
        writeln!(temp_file, r#"{{"type":"conversationStart"}}"#).unwrap();
        temp_file.flush().unwrap();

        // Parse the file
        let events = parse_jsonl_events(temp_file.path()).unwrap();
        assert_eq!(events.len(), 3);
        assert!(events[0].is_user_message());
        assert!(events[1].is_assistant_message());
        assert!(!events[2].is_message());
    }

    #[test]
    fn test_parse_jsonl_events_skip_empty_lines() {
        let mut temp_file = NamedTempFile::new().unwrap();

        // Write events with empty lines
        writeln!(temp_file, r#"{{"type":"userMessage","text":"Hello"}}"#).unwrap();
        writeln!(temp_file, "").unwrap();
        writeln!(temp_file, r#"{{"type":"assistantMessage","text":"Hi"}}"#).unwrap();
        writeln!(temp_file, "   ").unwrap();
        temp_file.flush().unwrap();

        // Parse the file
        let events = parse_jsonl_events(temp_file.path()).unwrap();
        assert_eq!(events.len(), 2); // Empty lines should be skipped
    }

    #[test]
    fn test_parse_jsonl_events_with_token_usage() {
        let mut temp_file = NamedTempFile::new().unwrap();

        // Write event with token usage
        writeln!(temp_file, r#"{{"type":"assistantMessage","text":"Response","usage":{{"input_tokens":100,"output_tokens":50,"cache_creation_input_tokens":10,"cache_read_input_tokens":5}}}}"#).unwrap();
        temp_file.flush().unwrap();

        // Parse the file
        let events = parse_jsonl_events(temp_file.path()).unwrap();
        assert_eq!(events.len(), 1);

        let usage = events[0].token_usage().unwrap();
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 50);
        assert_eq!(usage.cache_creation_input_tokens, 10);
        assert_eq!(usage.cache_read_input_tokens, 5);
        assert_eq!(usage.total(), 150);
        assert_eq!(usage.total_input(), 115);
    }

    #[test]
    fn test_get_claude_data_dir() {
        // This will use the actual environment
        let result = get_claude_data_dir();
        // We can't assert much here without mocking the environment
        // but we can at least check it doesn't panic
        let _ = result;
    }

    #[test]
    fn test_get_claude_data_dir_with_env() {
        // Test with CLAUDE_HOME set
        std::env::set_var("CLAUDE_HOME", "/custom/claude/path");
        let result = get_claude_data_dir().unwrap();
        assert_eq!(result, PathBuf::from("/custom/claude/path"));
        std::env::remove_var("CLAUDE_HOME");
    }
}
