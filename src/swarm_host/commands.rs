//! @hatchery: command parser.
//!
//! Parses commands from worker/coordinator output:
//! - `@hatchery:knowledge key=value`
//! - `@hatchery:result task_id=X status=success|failed`
//! - `@hatchery:message to=W2|coordinator|all text="..."`
//! - `@hatchery:query key=pattern`

use regex::Regex;
use std::sync::LazyLock;

/// Parsed hatchery command.
#[derive(Debug, Clone, PartialEq)]
pub enum HatcheryCommand {
    /// Write knowledge: `@hatchery:knowledge key=value`
    Knowledge {
        key: String,
        value: serde_json::Value,
    },
    /// Report task result: `@hatchery:result task_id=X status=success`
    Result {
        task_id: usize,
        status: String,
        message: Option<String>,
    },
    /// Send message: `@hatchery:message to=W2 text="..."`
    Message {
        to: String,
        text: String,
    },
    /// Query knowledge: `@hatchery:query api.*`
    Query {
        pattern: String,
    },
}

static CMD_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)@hatchery:(\w+)\s+(.+)").unwrap()
});

static KV_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(\w[\w.]*)\s*=\s*(?:'([^']*)'|"([^"]*)"|(\S+))"#).unwrap()
});

/// Extract all @hatchery: commands from a text block.
/// Returns (commands, remaining_text_without_commands).
pub fn parse_commands(text: &str) -> Vec<HatcheryCommand> {
    let mut commands = Vec::new();

    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(caps) = CMD_RE.captures(trimmed) {
            let cmd_type = caps.get(1).unwrap().as_str().to_lowercase();
            let args = caps.get(2).unwrap().as_str();

            match cmd_type.as_str() {
                "knowledge" => {
                    if let Some(cmd) = parse_knowledge(args) {
                        commands.push(cmd);
                    }
                }
                "result" => {
                    if let Some(cmd) = parse_result(args) {
                        commands.push(cmd);
                    }
                }
                "message" => {
                    if let Some(cmd) = parse_message(args) {
                        commands.push(cmd);
                    }
                }
                "query" => {
                    commands.push(HatcheryCommand::Query {
                        pattern: args.trim().to_string(),
                    });
                }
                _ => {
                    // Unknown command — ignore
                }
            }
        }
    }

    commands
}

/// Check if a line contains an @hatchery: command.
pub fn is_command(text: &str) -> bool {
    CMD_RE.is_match(text.trim())
}

fn parse_knowledge(args: &str) -> Option<HatcheryCommand> {
    // Try key=value format
    if let Some(pos) = args.find('=') {
        let key = args[..pos].trim().to_string();
        let raw_value = args[pos + 1..].trim();

        // Try to parse as JSON first
        let value = if raw_value.starts_with('{') || raw_value.starts_with('[') || raw_value.starts_with('\'') {
            let clean = raw_value.trim_matches('\'');
            serde_json::from_str(clean).unwrap_or_else(|_| serde_json::Value::String(raw_value.to_string()))
        } else {
            // Try number
            if let Ok(n) = raw_value.parse::<f64>() {
                serde_json::json!(n)
            } else {
                serde_json::Value::String(raw_value.trim_matches('"').to_string())
            }
        };

        Some(HatcheryCommand::Knowledge { key, value })
    } else {
        None
    }
}

fn parse_result(args: &str) -> Option<HatcheryCommand> {
    let kvs = extract_kvs(args);
    let task_id = kvs.get("task_id")?.parse::<usize>().ok()?;
    let status = kvs.get("status")?.to_string();
    let message = kvs.get("message").cloned();
    Some(HatcheryCommand::Result { task_id, status, message })
}

fn parse_message(args: &str) -> Option<HatcheryCommand> {
    let kvs = extract_kvs(args);
    let to = kvs.get("to")?.to_string();
    let text = kvs.get("text")?.to_string();
    Some(HatcheryCommand::Message { to, text })
}

/// Extract key=value pairs from a string.
fn extract_kvs(input: &str) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    for caps in KV_RE.captures_iter(input) {
        let key = caps.get(1).unwrap().as_str().to_string();
        let value = caps
            .get(2)
            .or_else(|| caps.get(3))
            .or_else(|| caps.get(4))
            .map(|m| m.as_str().to_string())
            .unwrap_or_default();
        map.insert(key, value);
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_knowledge_string() {
        let cmds = parse_commands("@hatchery:knowledge api.auth.method=bearer");
        assert_eq!(cmds.len(), 1);
        match &cmds[0] {
            HatcheryCommand::Knowledge { key, value } => {
                assert_eq!(key, "api.auth.method");
                assert_eq!(value, &serde_json::json!("bearer"));
            }
            _ => panic!("wrong command type"),
        }
    }

    #[test]
    fn test_parse_knowledge_json() {
        let cmds = parse_commands(r#"@hatchery:knowledge api.config='{"timeout": 30}'"#);
        assert_eq!(cmds.len(), 1);
        match &cmds[0] {
            HatcheryCommand::Knowledge { key, value } => {
                assert_eq!(key, "api.config");
                assert_eq!(value["timeout"], 30);
            }
            _ => panic!("wrong command type"),
        }
    }

    #[test]
    fn test_parse_result() {
        let cmds = parse_commands("@hatchery:result task_id=3 status=success");
        assert_eq!(cmds.len(), 1);
        match &cmds[0] {
            HatcheryCommand::Result { task_id, status, .. } => {
                assert_eq!(*task_id, 3);
                assert_eq!(status, "success");
            }
            _ => panic!("wrong command type"),
        }
    }

    #[test]
    fn test_parse_message() {
        let cmds = parse_commands(r#"@hatchery:message to=W2 text="Found auth pattern""#);
        assert_eq!(cmds.len(), 1);
        match &cmds[0] {
            HatcheryCommand::Message { to, text } => {
                assert_eq!(to, "W2");
                assert_eq!(text, "Found auth pattern");
            }
            _ => panic!("wrong command type"),
        }
    }

    #[test]
    fn test_parse_query() {
        let cmds = parse_commands("@hatchery:query api.*");
        assert_eq!(cmds.len(), 1);
        match &cmds[0] {
            HatcheryCommand::Query { pattern } => {
                assert_eq!(pattern, "api.*");
            }
            _ => panic!("wrong command type"),
        }
    }

    #[test]
    fn test_case_insensitive() {
        let cmds = parse_commands("@HATCHERY:KNOWLEDGE api.url=https://example.com");
        assert_eq!(cmds.len(), 1);
    }

    #[test]
    fn test_multi_command() {
        let text = r#"Some regular output text
@hatchery:knowledge api.auth=bearer
More regular text
@hatchery:result task_id=1 status=success
@hatchery:message to=all text="done with auth"
"#;
        let cmds = parse_commands(text);
        assert_eq!(cmds.len(), 3);
    }

    #[test]
    fn test_no_commands() {
        let cmds = parse_commands("Just regular output with no commands");
        assert!(cmds.is_empty());
    }
}
