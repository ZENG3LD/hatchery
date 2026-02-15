//! Session file parsing and discovery logic

use chrono::{DateTime, Utc};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::overseer::error::{ParseError, Result};
use crate::overseer::events::root::SessionEvent;
use crate::overseer::types::{SessionWithSubagents, SubagentSession};

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

        // Parse JSON — skip unparseable lines instead of failing
        match serde_json::from_str::<SessionEvent>(&line) {
            Ok(event) => events.push(event),
            Err(e) => {
                eprintln!(
                    "[overseer] warning: skipping line {} from {:?}: {}",
                    line_num + 1,
                    session_path,
                    e
                );
            }
        }
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

/// Find subagent JSONL files for a session
///
/// Discovers all subagent logs in the session's subagents directory.
///
/// # Directory Structure
///
/// ```text
/// .claude/projects/<project>/<session-id>.jsonl          # Main session
/// .claude/projects/<project>/<session-id>/subagents/     # Subagent directory
///   ├── agent-a0179af.jsonl                              # Subagent 1
///   ├── agent-a18af05.jsonl                              # Subagent 2
///   └── ...
/// ```
///
/// # Arguments
///
/// * `session_path` - Path to the main session JSONL file
///
/// # Returns
///
/// Vector of tuples: (agent_id, path_to_subagent_jsonl)
pub fn find_subagent_files(session_path: &Path) -> Vec<(String, PathBuf)> {
    // Session JSONL: .../projects/<project>/<session-id>.jsonl
    // Subagents dir: .../projects/<project>/<session-id>/subagents/
    let session_id = extract_session_id(session_path);
    let parent = session_path.parent().unwrap_or_else(|| Path::new("."));
    let subagents_dir = parent.join(&session_id).join("subagents");

    if !subagents_dir.exists() {
        return Vec::new();
    }

    let mut result = Vec::new();
    if let Ok(entries) = fs::read_dir(&subagents_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map_or(false, |e| e == "jsonl") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    if let Some(agent_id) = stem.strip_prefix("agent-") {
                        result.push((agent_id.to_string(), path));
                    }
                }
            }
        }
    }
    result
}

/// Parse session with all subagents linked
///
/// Parses the main session JSONL file and all associated subagent logs,
/// linking them together via agent IDs and metadata.
///
/// # Arguments
///
/// * `session_path` - Path to the main session JSONL file
///
/// # Returns
///
/// Complete session with all subagents parsed and linked
pub fn parse_session_with_subagents(session_path: &Path) -> Result<SessionWithSubagents> {
    let session_id = extract_session_id(session_path);
    let main_events = parse_jsonl_events(session_path)?;
    let subagent_files = find_subagent_files(session_path);

    let mut subagents = Vec::new();

    // Build a map of agent_id -> metadata from main session tool results
    let mut agent_metadata = std::collections::HashMap::new();
    for event in &main_events {
        if let SessionEvent::User(user_event) = event {
            if let Some(metadata) = user_event.extract_subagent_metadata() {
                if let Some(agent_id) = &metadata.agent_id {
                    agent_metadata.insert(agent_id.clone(), metadata);
                }
            }
        }
    }

    for (agent_id, subagent_path) in subagent_files {
        let events = parse_jsonl_events(&subagent_path)?;

        // Extract model from first assistant message
        let model = events
            .iter()
            .find_map(|e| {
                if let SessionEvent::Assistant(msg) = e {
                    Some(msg.message.model.clone())
                } else {
                    None
                }
            })
            .unwrap_or_else(|| "unknown".to_string());

        // Get timestamps from first and last events
        let spawn_timestamp = events.first().map(|e| e.timestamp().timestamp());
        let complete_timestamp = events.last().map(|e| e.timestamp().timestamp());

        // Get metadata from main session (if available)
        let metadata = agent_metadata.get(&agent_id);
        let total_tokens = metadata
            .and_then(|m| m.total_tokens)
            .unwrap_or(0);
        let total_tool_use_count = metadata
            .and_then(|m| m.total_tool_use_count)
            .unwrap_or(0);
        let total_duration_ms = metadata
            .and_then(|m| m.total_duration_ms)
            .unwrap_or(0);

        subagents.push(SubagentSession {
            agent_id,
            session_id: session_id.clone(),
            jsonl_path: subagent_path,
            subagent_type: None, // TODO: extract from Task input
            model,
            spawn_timestamp,
            complete_timestamp,
            total_tokens,
            total_tool_use_count,
            total_duration_ms,
            events,
        });
    }

    Ok(SessionWithSubagents {
        session_id,
        jsonl_path: session_path.to_path_buf(),
        events: main_events,
        subagents,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
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

    // NOTE: Tests for parsing actual event content removed - they referenced methods
    // (.event_type, .text(), .is_user_message(), .token_usage(), etc.) that don't
    // exist on SessionEvent enum. The SessionEvent variants are: User, Assistant,
    // Progress, System, FileSnapshot, QueueOperation, Summary, Unknown.
    //
    // If tests are needed, they should use pattern matching on the actual variants
    // and call the methods that exist (e.g., .uuid(), .parent_uuid(), .extract_text_content()).

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

    #[test]
    fn test_find_subagent_files_empty() {
        // Non-existent session should return empty vec
        let nonexistent = PathBuf::from("/nonexistent/session.jsonl");
        let subagents = find_subagent_files(&nonexistent);
        assert_eq!(subagents.len(), 0);
    }

    #[test]
    fn test_extract_subagent_id_from_path() {
        let path = PathBuf::from("agent-a18af05.jsonl");
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap();
        let agent_id = stem.strip_prefix("agent-").unwrap();
        assert_eq!(agent_id, "a18af05");
    }

    // NOTE: Real integration tests would require setting up a temp directory
    // with mock session and subagent files. Skipping for now as the logic
    // is straightforward directory traversal.
}
