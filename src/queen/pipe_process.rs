//! Pipe-based process wrapper for Claude Code CLI.
//!
//! This is a simplified copy of zengeld-hub's PipeProcess, adapted for Hatchery's needs.
//! Unlike zengeld-hub which supports multiple CLI tools (Claude, Codex, Gemini),
//! this module ONLY supports Claude Code.
//!
//! Key features:
//! - Spawns `claude -p` with stdin/stdout pipes for headless NDJSON streaming
//! - Background reader thread with mpsc channel for non-blocking output
//! - Windows-specific command building via `cmd /C "claude ..."`
//! - Environment variable support for passing API keys

use std::io::Write;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use serde_json::Value;

/// Find the path to Claude Code's cli.js for direct node execution on Windows.
///
/// Checks in order:
/// 1. CLAUDE_CLI_JS environment variable
/// 2. %APPDATA%\npm\node_modules\@anthropic-ai\claude-code\cli.js
/// 3. Returns None (fallback to "claude" command)
fn find_claude_cli_js() -> Option<std::path::PathBuf> {
    // 1. Check environment variable
    if let Ok(path) = std::env::var("CLAUDE_CLI_JS") {
        let p = std::path::PathBuf::from(path);
        if p.exists() {
            return Some(p);
        }
    }

    // 2. Check standard npm global location
    if let Ok(appdata) = std::env::var("APPDATA") {
        let cli_path = std::path::PathBuf::from(appdata)
            .join("npm")
            .join("node_modules")
            .join("@anthropic-ai")
            .join("claude-code")
            .join("cli.js");
        if cli_path.exists() {
            return Some(cli_path);
        }
    }

    // 3. Fallback
    None
}

/// Options for PipeProcess spawning (Claude Code specific flags).
#[derive(Debug, Clone, Default)]
pub struct PipeProcessOptions {
    /// Content to append to the system prompt via --append-system-prompt.
    /// This content goes into the system prompt and is NEVER compressed or ignored — highest priority.
    pub append_system_prompt: Option<String>,

    /// Resume an existing session via --resume <session-id>.
    pub resume_session_id: Option<String>,

    /// Model override via --model.
    pub model: Option<String>,

    /// Input format override via --input-format (e.g. "stream-json")
    pub input_format: Option<String>,

    /// Output format override via --output-format (default: "stream-json")
    pub output_format: Option<String>,

    /// Max agent turns via --max-turns
    pub max_turns: Option<u32>,

    /// Max budget in USD via --max-budget-usd
    pub max_budget_usd: Option<f64>,

    /// Allowed tools whitelist via --allowedTools
    pub allowed_tools: Option<String>,

    /// If true, pass --no-project to Claude CLI to prevent loading CLAUDE.md
    pub no_project: Option<bool>,

    /// Setting sources for Claude Code (e.g., "user" to disable project CLAUDE.md).
    /// Maps to --setting-sources flag. None = default (all sources).
    pub setting_sources: Option<String>,
}

/// A pipe-based process for Claude Code CLI execution.
///
/// Spawns `claude -p --output-format stream-json` and provides:
/// - Non-blocking output reading via `try_recv()`
/// - Stdin writing via `write()`
/// - Process lifecycle management
pub struct PipeProcess {
    child: Child,
    stdin: Option<std::process::ChildStdin>,
    output_rx: Receiver<String>,
    /// Temp prompt file to clean up on drop.
    prompt_file: Option<std::path::PathBuf>,
}

impl PipeProcess {
    /// Spawn Claude Code CLI process with stdin/stdout pipes.
    ///
    /// Launch command: `claude -p --output-format stream-json --verbose --dangerously-skip-permissions "prompt"`
    pub fn new(
        working_dir: &std::path::Path,
        initial_prompt: &str,
    ) -> Result<Self, std::io::Error> {
        Self::new_with_options(working_dir, initial_prompt, PipeProcessOptions::default(), vec![])
    }

    /// Spawn Claude Code CLI process with custom options and environment variables.
    ///
    /// # Arguments
    /// * `working_dir` - Working directory for the claude process
    /// * `initial_prompt` - Initial prompt to pass to claude
    /// * `options` - Claude-specific CLI options (model, resume, etc.)
    /// * `envs` - Environment variables to set (e.g. API keys)
    pub fn new_with_options(
        working_dir: &std::path::Path,
        initial_prompt: &str,
        options: PipeProcessOptions,
        envs: Vec<(String, String)>,
    ) -> Result<Self, std::io::Error> {
        // Write prompt to temp file and pass via @path reference.
        // This avoids all escaping/length issues with command-line arguments.
        let prompt_dir = working_dir.join(".hatchery").join("prompts");
        std::fs::create_dir_all(&prompt_dir)?;

        let prompt_file = prompt_dir.join(format!("prompt_{}.md", std::process::id()));
        std::fs::write(&prompt_file, initial_prompt)?;

        let prompt_ref = format!("@{}", prompt_file.display());

        let mut cmd = Self::build_command(&prompt_ref, &options);
        cmd.current_dir(working_dir);
        cmd.stdin(Stdio::null());  // per-task mode: no stdin needed, prompt is via @path
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        // Apply environment variables
        for (key, value) in envs {
            cmd.env(key, value);
        }

        eprintln!("[PipeProcess] Spawning with prompt file: {}", prompt_file.display());

        let mut child = cmd.spawn()?;

        let stdin = child.stdin.take();
        let stdout = child.stdout.take()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::Other, "no stdout"))?;

        // Spawn stderr reader for diagnostics
        if let Some(stderr) = child.stderr.take() {
            thread::spawn(move || {
                use std::io::{BufRead, BufReader};
                let reader = BufReader::new(stderr);
                for line in reader.lines() {
                    match line {
                        Ok(line) => eprintln!("[PipeProcess stderr] {}", line),
                        Err(_) => break,
                    }
                }
            });
        }

        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            Self::reader_thread(stdout, tx);
        });

        Ok(Self {
            child,
            stdin,
            output_rx: rx,
            prompt_file: Some(prompt_file),
        })
    }

    /// Build the Command for spawning Claude Code.
    ///
    /// On Windows, uses node + cli.js directly (if found) to avoid cmd /C quote escaping issues.
    /// Falls back to "claude" command if cli.js not found.
    ///
    /// Windows: `node %APPDATA%\npm\...\cli.js -p --output-format stream-json prompt`
    /// Unix: `claude -p --output-format stream-json prompt`
    fn build_command(prompt: &str, options: &PipeProcessOptions) -> Command {
        if cfg!(windows) {
            let output_format = options.output_format.as_deref().unwrap_or("stream-json");

            // Try to find cli.js for direct node execution
            let mut cmd = if let Some(cli_js) = find_claude_cli_js() {
                let mut c = Command::new("node");
                c.arg(cli_js);
                c
            } else {
                // Fallback to claude command
                Command::new("claude")
            };

            // Add core flags
            cmd.arg("-p");
            cmd.arg("--output-format");
            cmd.arg(output_format);
            cmd.arg("--verbose");
            cmd.arg("--dangerously-skip-permissions");

            // Append optional flags
            if let Some(ref system_prompt) = options.append_system_prompt {
                cmd.arg("--append-system-prompt");
                cmd.arg(system_prompt);
            }
            if let Some(ref session_id) = options.resume_session_id {
                cmd.arg("--resume");
                cmd.arg(session_id);
            }
            if let Some(ref model) = options.model {
                cmd.arg("--model");
                cmd.arg(model);
            }
            if let Some(ref input_format) = options.input_format {
                cmd.arg("--input-format");
                cmd.arg(input_format);
            }
            if let Some(max_turns) = options.max_turns {
                cmd.arg("--max-turns");
                cmd.arg(max_turns.to_string());
            }
            if let Some(max_budget) = options.max_budget_usd {
                cmd.arg("--max-budget-usd");
                cmd.arg(max_budget.to_string());
            }
            if let Some(ref allowed_tools) = options.allowed_tools {
                cmd.arg("--allowedTools");
                cmd.arg(allowed_tools);
            }
            if options.no_project.unwrap_or(false) {
                cmd.arg("--no-project");
            }
            if let Some(ref sources) = options.setting_sources {
                cmd.arg("--setting-sources");
                cmd.arg(sources);
            }

            // Append prompt
            cmd.arg(prompt);
            cmd
        } else {
            // Unix: use individual args
            let mut cmd = Command::new("claude");
            cmd.arg("-p");
            cmd.arg("--output-format");
            cmd.arg(options.output_format.as_deref().unwrap_or("stream-json"));
            cmd.arg("--verbose");
            cmd.arg("--dangerously-skip-permissions");

            if let Some(ref system_prompt) = options.append_system_prompt {
                cmd.arg("--append-system-prompt");
                cmd.arg(system_prompt);
            }
            if let Some(ref session_id) = options.resume_session_id {
                cmd.arg("--resume");
                cmd.arg(session_id);
            }
            if let Some(ref model) = options.model {
                cmd.arg("--model");
                cmd.arg(model);
            }
            if let Some(ref input_format) = options.input_format {
                cmd.arg("--input-format");
                cmd.arg(input_format);
            }
            if let Some(max_turns) = options.max_turns {
                cmd.arg("--max-turns");
                cmd.arg(max_turns.to_string());
            }
            if let Some(max_budget) = options.max_budget_usd {
                cmd.arg("--max-budget-usd");
                cmd.arg(max_budget.to_string());
            }
            if let Some(ref allowed_tools) = options.allowed_tools {
                cmd.arg("--allowedTools");
                cmd.arg(allowed_tools);
            }
            if options.no_project.unwrap_or(false) {
                cmd.arg("--no-project");
            }
            if let Some(ref sources) = options.setting_sources {
                cmd.arg("--setting-sources");
                cmd.arg(sources);
            }

            cmd.arg(prompt);
            cmd
        }
    }

    /// Background reader thread — reads stdout line-by-line and sends via channel.
    fn reader_thread(stdout: std::process::ChildStdout, tx: Sender<String>) {
        use std::io::{BufRead, BufReader};

        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            match line {
                Ok(line) => {
                    if tx.send(format!("{}\n", line)).is_err() {
                        // Receiver dropped — exit thread
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    }

    /// Try to receive output (non-blocking).
    ///
    /// Returns `Some(line)` if output is available, `None` otherwise.
    pub fn try_recv(&self) -> Option<String> {
        self.output_rx.try_recv().ok()
    }

    /// Write input to the process stdin.
    pub fn write(&mut self, data: &str) -> Result<(), std::io::Error> {
        if let Some(stdin) = &mut self.stdin {
            stdin.write_all(data.as_bytes())?;
            stdin.flush()?;
        }
        Ok(())
    }

    /// Check if the process is still running.
    pub fn is_running(&mut self) -> bool {
        self.child.try_wait().ok().flatten().is_none()
    }

    /// Kill the process.
    pub fn kill(&mut self) -> Result<(), std::io::Error> {
        self.child.kill()
    }
}

impl Drop for PipeProcess {
    fn drop(&mut self) {
        // Clean up temp prompt file
        if let Some(ref path) = self.prompt_file {
            let _ = std::fs::remove_file(path);
        }
    }
}

// ============================================================================
// NDJSON Parsing
// ============================================================================

/// Truncate a string to at most `max` bytes on a char boundary.
fn truncate_str(s: &str, max: usize) -> &str {
    if s.len() <= max {
        s
    } else {
        let mut end = max;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        &s[..end]
    }
}

/// Unified event type from Claude Code's NDJSON stream.
///
/// Parsed from `claude -p --output-format stream-json` output.
#[derive(Debug, Clone)]
pub enum CliEvent {
    /// Session initialized (model, session ID, tools list).
    SessionStart {
        session_id: String,
        model: String,
        tools: Vec<String>,
    },

    /// Assistant text (complete message or streaming delta).
    AssistantText {
        text: String,
        is_delta: bool,
    },

    /// Tool call initiated by the assistant.
    ToolCallStart {
        id: String,
        name: String,
        input: Value,
    },

    /// Tool call result returned.
    ToolCallResult {
        id: String,
        output: String,
        is_error: bool,
        duration_ms: Option<u64>,
    },

    /// Thinking/reasoning content.
    Thinking {
        text: String,
    },

    /// Turn completed with token usage.
    TurnComplete {
        input_tokens: u64,
        output_tokens: u64,
    },

    /// Session ended.
    SessionEnd {
        result: String,
        cost_usd: Option<f64>,
        is_error: bool,
    },

    /// Error event.
    Error {
        message: String,
    },
}

/// Claude Code stream-json parser.
///
/// Parses NDJSON output from: `claude -p "prompt" --output-format stream-json --verbose`
///
/// Event types:
/// - "system" — session initialization
/// - "assistant" — assistant messages (text/tool_use blocks)
/// - "user" — tool results
/// - "result" — final session result
/// - "stream_event" — token-level streaming deltas
pub struct ClaudeNdjsonParser {
    session_id: Option<String>,
}

impl ClaudeNdjsonParser {
    pub fn new() -> Self {
        Self { session_id: None }
    }

    /// Parse a single NDJSON line into zero or more events.
    pub fn parse_line(&mut self, line: &str) -> Vec<CliEvent> {
        let line = line.trim();
        if line.is_empty() {
            return vec![];
        }

        let v: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => {
                return vec![CliEvent::Error {
                    message: format!("invalid JSON: {}", truncate_str(line, 100)),
                }]
            }
        };

        let mut events = Vec::new();

        match v.get("type").and_then(|t| t.as_str()) {
            Some("system") => {
                // Init event — session start
                let sid = v
                    .get("session_id")
                    .and_then(|s| s.as_str())
                    .unwrap_or("")
                    .to_string();
                let model = v
                    .get("model")
                    .and_then(|s| s.as_str())
                    .unwrap_or("unknown")
                    .to_string();
                let tools = v
                    .get("tools")
                    .and_then(|t| t.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();
                self.session_id = Some(sid.clone());
                events.push(CliEvent::SessionStart {
                    session_id: sid,
                    model,
                    tools,
                });
            }
            Some("assistant") => {
                // Assistant message — can contain text blocks and tool_use blocks
                if let Some(content) = v.pointer("/message/content").and_then(|c| c.as_array()) {
                    for block in content {
                        match block.get("type").and_then(|t| t.as_str()) {
                            Some("text") => {
                                if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                                    events.push(CliEvent::AssistantText {
                                        text: text.to_string(),
                                        is_delta: false,
                                    });
                                }
                            }
                            Some("tool_use") => {
                                let id = block
                                    .get("id")
                                    .and_then(|s| s.as_str())
                                    .unwrap_or("")
                                    .to_string();
                                let name = block
                                    .get("name")
                                    .and_then(|s| s.as_str())
                                    .unwrap_or("")
                                    .to_string();
                                let input = block.get("input").cloned().unwrap_or(Value::Null);
                                events.push(CliEvent::ToolCallStart { id, name, input });
                            }
                            Some("thinking") => {
                                if let Some(text) = block.get("thinking").and_then(|t| t.as_str()) {
                                    events.push(CliEvent::Thinking {
                                        text: text.to_string(),
                                    });
                                }
                            }
                            _ => {}
                        }
                    }
                }
                // Extract usage
                if let Some(usage) = v.pointer("/message/usage") {
                    let input = usage.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
                    let output = usage.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
                    if input > 0 || output > 0 {
                        events.push(CliEvent::TurnComplete {
                            input_tokens: input,
                            output_tokens: output,
                        });
                    }
                }
            }
            Some("user") => {
                // Tool results
                if let Some(content) = v.pointer("/message/content").and_then(|c| c.as_array()) {
                    for block in content {
                        if block.get("type").and_then(|t| t.as_str()) == Some("tool_result") {
                            let id = block
                                .get("tool_use_id")
                                .and_then(|s| s.as_str())
                                .unwrap_or("")
                                .to_string();
                            let output = block
                                .get("content")
                                .and_then(|s| s.as_str())
                                .unwrap_or("")
                                .to_string();
                            let is_error = block
                                .get("is_error")
                                .and_then(|b| b.as_bool())
                                .unwrap_or(false);
                            let duration_ms = v
                                .pointer("/tool_use_result/durationMs")
                                .and_then(|d| d.as_u64());
                            events.push(CliEvent::ToolCallResult {
                                id,
                                output,
                                is_error,
                                duration_ms,
                            });
                        }
                    }
                }
            }
            Some("result") => {
                let result_text = v
                    .get("result")
                    .and_then(|s| s.as_str())
                    .unwrap_or("")
                    .to_string();
                let cost = v.get("total_cost_usd").and_then(|c| c.as_f64());
                let is_error = v.get("is_error").and_then(|b| b.as_bool()).unwrap_or(false);
                events.push(CliEvent::SessionEnd {
                    result: result_text,
                    cost_usd: cost,
                    is_error,
                });
            }
            Some("stream_event") => {
                // Token-level streaming delta
                if let Some(delta_text) = v.pointer("/event/delta/text") {
                    if let Some(text) = delta_text.as_str() {
                        events.push(CliEvent::AssistantText {
                            text: text.to_string(),
                            is_delta: true,
                        });
                    }
                }
            }
            _ => {}
        }

        events
    }

    /// Get the session ID if known.
    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }
}

impl Default for ClaudeNdjsonParser {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Stream Command Builder (for StreamQueen)
// ============================================================================

/// Build a Command for Claude Code in stream mode (--input-format stream-json).
///
/// This is used by StreamQueen which keeps the process alive and sends messages via stdin.
/// Unlike PipeProcess which uses `-p "prompt"` (PerTask mode), this builds a command
/// for long-lived stdin/stdout streaming without an initial prompt.
///
/// # Arguments
/// * `working_dir` - Working directory for the claude process
/// * `options` - Claude-specific CLI options (model, max_turns, etc.)
/// * `envs` - Environment variables to set (e.g. Hatchery IPC vars)
///
/// # Returns
/// A fully configured `std::process::Command` ready to spawn.
pub fn build_stream_command(
    working_dir: &std::path::Path,
    options: &PipeProcessOptions,
    envs: &[(String, String)],
) -> Command {
    let mut cmd = if cfg!(windows) {
        // Try to find cli.js for direct node execution
        let mut c = if let Some(cli_js) = find_claude_cli_js() {
            let mut cmd = Command::new("node");
            cmd.arg(cli_js);
            cmd
        } else {
            // Fallback to claude command
            Command::new("claude")
        };

        // Add core flags
        c.arg("-p");
        c.arg("--input-format");
        c.arg("stream-json");
        c.arg("--output-format");
        c.arg("stream-json");
        c.arg("--verbose");
        c.arg("--dangerously-skip-permissions");

        // Append optional flags
        if let Some(ref model) = options.model {
            c.arg("--model");
            c.arg(model);
        }
        if let Some(max_turns) = options.max_turns {
            c.arg("--max-turns");
            c.arg(max_turns.to_string());
        }
        if let Some(max_budget) = options.max_budget_usd {
            c.arg("--max-budget-usd");
            c.arg(max_budget.to_string());
        }
        if let Some(ref system_prompt) = options.append_system_prompt {
            c.arg("--append-system-prompt");
            c.arg(system_prompt);
        }
        if let Some(ref tools) = options.allowed_tools {
            c.arg("--allowedTools");
            c.arg(tools);
        }
        if options.no_project.unwrap_or(false) {
            c.arg("--no-project");
        }
        if let Some(ref sources) = options.setting_sources {
            c.arg("--setting-sources");
            c.arg(sources);
        }
        c
    } else {
        // Unix: use individual args
        let mut cmd = Command::new("claude");
        cmd.args([
            "-p",
            "--input-format",
            "stream-json",
            "--output-format",
            "stream-json",
            "--verbose",
            "--dangerously-skip-permissions",
        ]);

        if let Some(ref model) = options.model {
            cmd.args(["--model", model]);
        }
        if let Some(max_turns) = options.max_turns {
            cmd.args(["--max-turns", &max_turns.to_string()]);
        }
        if let Some(max_budget) = options.max_budget_usd {
            cmd.args(["--max-budget-usd", &max_budget.to_string()]);
        }
        if let Some(ref prompt) = options.append_system_prompt {
            cmd.args(["--append-system-prompt", prompt]);
        }
        if let Some(ref tools) = options.allowed_tools {
            cmd.args(["--allowedTools", tools]);
        }
        if options.no_project.unwrap_or(false) {
            cmd.arg("--no-project");
        }
        if let Some(ref sources) = options.setting_sources {
            cmd.args(["--setting-sources", sources]);
        }
        cmd
    };

    // Set working directory
    cmd.current_dir(working_dir);

    // Apply environment variables
    for (k, v) in envs {
        cmd.env(k, v);
    }

    // Configure pipes
    cmd.stdin(Stdio::piped());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    cmd
}
