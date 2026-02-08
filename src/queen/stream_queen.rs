//! StreamQueen: Long-lived subprocess actor for Hatchery V3.
//!
//! Spawns ONE `claude` process using `--input-format stream-json --output-format stream-json`
//! and keeps it alive, sending NDJSON messages to stdin for each task.
//!
//! This is the "Stream" spawn mode implementation, providing efficient task execution
//! by reusing the same Claude Code process across multiple tasks.

use crate::core::types::{QueenId, QueenStatus, Task, TaskContext, TaskId};
use crate::queen::completion::{CompletionConfig, CompletionDetector, CompletionSignal, CompletionVerdict};
use crate::queen::handle::{QueenCommand, QueenEvent, QueenHandle};
use crate::queen::spawn_mode::{ClaudeEvent, SpawnMode, StreamInput};
use anyhow::{Context as _, Result};
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::{broadcast, mpsc, watch};

// ============================================================================
// Configuration
// ============================================================================

/// Configuration for a StreamQueen.
#[derive(Debug, Clone)]
pub struct StreamQueenConfig {
    /// Unique identifier for this Queen.
    pub id: QueenId,
    /// Model name: "sonnet", "opus", "haiku".
    pub model: String,
    /// Working directory for the subprocess.
    pub working_dir: PathBuf,
    /// Maximum number of agent turns (maps to --max-turns).
    pub max_turns: Option<u32>,
    /// Maximum budget in USD (maps to --max-budget-usd).
    pub max_budget_usd: Option<f64>,
    /// System prompt to append (maps to --append-system-prompt).
    pub system_prompt: Option<String>,
    /// Allowed tools (maps to --allowedTools).
    pub allowed_tools: Option<String>,
    /// Completion detection configuration.
    pub completion: CompletionConfig,
}

impl StreamQueenConfig {
    /// Create a new StreamQueenConfig with sensible defaults.
    pub fn new(id: QueenId, working_dir: PathBuf) -> Self {
        Self {
            id,
            model: "sonnet".to_string(),
            working_dir,
            max_turns: None,
            max_budget_usd: None,
            system_prompt: None,
            allowed_tools: None,
            completion: CompletionConfig::default(),
        }
    }
}

// ============================================================================
// Public spawn() Function
// ============================================================================

/// Spawn a StreamQueen actor.
///
/// Returns a cloneable `QueenHandle` for sending commands and a `JoinHandle`
/// for the actor task.
///
/// # Arguments
/// * `config` - Configuration for this Queen
/// * `event_tx` - Channel for sending events back to the SwarmHost
/// * `shutdown_rx` - Broadcast receiver for shutdown signal
///
/// # Errors
/// Returns an error if:
/// - The Claude Code process fails to spawn
/// - The subprocess stdin/stdout cannot be captured
pub fn spawn(
    config: StreamQueenConfig,
    event_tx: mpsc::Sender<QueenEvent>,
    shutdown_rx: broadcast::Receiver<()>,
) -> Result<(QueenHandle, tokio::task::JoinHandle<()>)> {
    // Build the command
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

    // Add model flag
    if !config.model.is_empty() {
        cmd.args(["--model", &config.model]);
    }

    // Add optional flags
    if let Some(max_turns) = config.max_turns {
        cmd.args(["--max-turns", &max_turns.to_string()]);
    }
    if let Some(max_budget) = config.max_budget_usd {
        cmd.args(["--max-budget-usd", &max_budget.to_string()]);
    }
    if let Some(ref prompt) = config.system_prompt {
        cmd.args(["--append-system-prompt", prompt]);
    }
    if let Some(ref tools) = config.allowed_tools {
        cmd.args(["--allowedTools", tools]);
    }

    // Set working directory and pipes
    cmd.current_dir(&config.working_dir);
    cmd.stdin(std::process::Stdio::piped());
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    // Spawn the child process
    let mut child = cmd.spawn().context("Failed to spawn Claude Code subprocess")?;

    // Extract stdin/stdout
    let stdin = child
        .stdin
        .take()
        .context("Failed to capture subprocess stdin")?;
    let stdout = child
        .stdout
        .take()
        .context("Failed to capture subprocess stdout")?;
    let stderr = child.stderr.take();

    // Create channels
    let (cmd_tx, cmd_rx) = mpsc::channel::<QueenCommand>(64);
    let (status_tx, status_rx) = watch::channel(QueenStatus::Idle);
    let (stdin_tx, stdin_rx) = mpsc::channel::<String>(64);

    // Build handle
    let handle = QueenHandle::new(
        config.id.clone(),
        SpawnMode::Stream,
        cmd_tx,
        status_rx,
    );

    // Spawn stderr reader task (log to eprintln)
    if let Some(stderr) = stderr {
        tokio::spawn(async move {
            let mut reader = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                eprintln!("[StreamQueen stderr] {}", line);
            }
        });
    }

    // Spawn the main actor task
    let completion_detector = CompletionDetector::new(config.completion);
    let join_handle = tokio::spawn(run_actor(
        config.id,
        child,
        stdin,
        stdout,
        cmd_rx,
        event_tx,
        status_tx,
        stdin_tx,
        stdin_rx,
        shutdown_rx,
        completion_detector,
    ));

    Ok((handle, join_handle))
}

// ============================================================================
// Main Actor Loop
// ============================================================================

#[allow(clippy::too_many_arguments)]
async fn run_actor(
    id: QueenId,
    mut child: Child,
    stdin: ChildStdin,
    stdout: ChildStdout,
    mut cmd_rx: mpsc::Receiver<QueenCommand>,
    event_tx: mpsc::Sender<QueenEvent>,
    status_tx: watch::Sender<QueenStatus>,
    stdin_tx: mpsc::Sender<String>,
    stdin_rx: mpsc::Receiver<String>,
    mut shutdown_rx: broadcast::Receiver<()>,
    completion_detector: CompletionDetector,
) {
    // Internal state
    let mut session_id: Option<String> = None;
    let mut current_task: Option<(TaskId, String)> = None;
    let mut turn_count: u32 = 0;
    let mut accumulated_cost: f64 = 0.0;

    // Channel for receiving parsed events from stdout
    let (stdout_event_tx, mut stdout_event_rx) = mpsc::channel::<ClaudeEvent>(256);

    // Spawn stdin writer task
    tokio::spawn(stdin_writer(stdin, stdin_rx));

    // Spawn stdout reader task
    tokio::spawn(stdout_reader(stdout, stdout_event_tx));

    // Main select loop
    loop {
        tokio::select! {
            // Command from SwarmHost
            Some(cmd) = cmd_rx.recv() => {
                match cmd {
                    QueenCommand::Assign { task, context } => {
                        // Build prompt from task + context
                        let prompt = format_task_prompt(&task, &context);
                        current_task = Some((task.id.clone(), task.description.clone()));
                        turn_count = 0;
                        accumulated_cost = 0.0;

                        // Update status
                        let _ = status_tx.send(QueenStatus::Working {
                            task_id: task.id.clone(),
                            progress: 0.0,
                            sub_tasks: vec![],
                        });
                        let _ = event_tx.send(QueenEvent::StatusChanged {
                            queen_id: id.clone(),
                            status: QueenStatus::Working {
                                task_id: task.id,
                                progress: 0.0,
                                sub_tasks: vec![],
                            },
                        }).await;

                        // Send NDJSON to stdin
                        let input = StreamInput::user_message(&prompt, session_id.as_deref());
                        if let Ok(json) = serde_json::to_string(&input) {
                            let _ = stdin_tx.send(json).await;
                        }
                    }
                    QueenCommand::Message(msg) => {
                        // Queen-to-Queen message: send as user message
                        let content = serde_json::to_string(&msg.payload).unwrap_or_default();
                        let input = StreamInput::user_message(&content, session_id.as_deref());
                        if let Ok(json) = serde_json::to_string(&input) {
                            let _ = stdin_tx.send(json).await;
                        }
                    }
                    QueenCommand::Shutdown => {
                        // Drop stdin to signal EOF
                        drop(stdin_tx);
                        break;
                    }
                }
            }

            // Event from stdout
            Some(event) = stdout_event_rx.recv() => {
                // Capture session_id from system init
                if event.is_system_init() {
                    if let Some(sid) = &event.session_id {
                        session_id = Some(sid.clone());
                    }
                }

                // Count turns from assistant events
                if event.is_assistant() {
                    turn_count += 1;
                    accumulated_cost = event.cost().unwrap_or(accumulated_cost);

                    // Emit progress
                    if let Some((ref tid, _)) = current_task {
                        let _ = event_tx.send(QueenEvent::Progress {
                            queen_id: id.clone(),
                            task_id: tid.clone(),
                            turns_completed: turn_count,
                            cost_usd: accumulated_cost,
                        }).await;
                    }
                }

                // Handle result event
                if event.is_result() {
                    let cost = event.cost().unwrap_or(0.0);
                    let duration = event.duration_ms.unwrap_or(0);
                    let turns = event.num_turns.unwrap_or(turn_count);

                    let signal = CompletionSignal::ResultEvent {
                        subtype: event.subtype.clone().unwrap_or_default(),
                        result_text: event.result.clone(),
                        cost_usd: cost,
                        duration_ms: duration,
                        num_turns: turns,
                        session_id: session_id.clone(),
                    };

                    match completion_detector.evaluate(&signal) {
                        CompletionVerdict::Success {
                            result_text,
                            cost_usd,
                            duration_ms,
                            num_turns,
                            session_id: sid,
                            quality_passed,
                        } => {
                            if let Some((ref tid, _)) = current_task {
                                let _ = event_tx.send(QueenEvent::TaskCompleted {
                                    queen_id: id.clone(),
                                    task_id: tid.clone(),
                                    result_text,
                                    cost_usd,
                                    duration_ms,
                                    num_turns,
                                    session_id: sid,
                                    quality_passed,
                                }).await;
                                let _ = status_tx.send(QueenStatus::Idle);
                                let _ = event_tx.send(QueenEvent::StatusChanged {
                                    queen_id: id.clone(),
                                    status: QueenStatus::Idle,
                                }).await;
                            }
                            current_task = None;
                            turn_count = 0;
                            accumulated_cost = 0.0;
                        }
                        CompletionVerdict::Failed { error, cost_usd, num_turns } => {
                            if let Some((ref tid, _)) = current_task {
                                let _ = event_tx.send(QueenEvent::TaskFailed {
                                    queen_id: id.clone(),
                                    task_id: tid.clone(),
                                    error,
                                    cost_usd,
                                    num_turns,
                                }).await;
                                let _ = status_tx.send(QueenStatus::Idle);
                            }
                            current_task = None;
                            turn_count = 0;
                            accumulated_cost = 0.0;
                        }
                        CompletionVerdict::TimedOut { reason } => {
                            if let Some((ref tid, _)) = current_task {
                                let _ = event_tx.send(QueenEvent::TaskFailed {
                                    queen_id: id.clone(),
                                    task_id: tid.clone(),
                                    error: reason,
                                    cost_usd: accumulated_cost,
                                    num_turns: turn_count,
                                }).await;
                            }
                            current_task = None;
                        }
                    }
                }
            }

            // Process died
            status = child.wait() => {
                let exit_code = status.ok().and_then(|s| s.code());
                let _ = event_tx.send(QueenEvent::ProcessDied {
                    queen_id: id.clone(),
                    exit_code,
                    session_id: session_id.clone(),
                }).await;
                let _ = status_tx.send(QueenStatus::Dead);
                break;
            }

            // Shutdown signal
            _ = shutdown_rx.recv() => {
                drop(stdin_tx);
                let _ = child.kill().await;
                break;
            }
        }
    }
}

// ============================================================================
// Helper Tasks
// ============================================================================

/// Writes NDJSON lines to child stdin.
async fn stdin_writer(mut stdin: ChildStdin, mut rx: mpsc::Receiver<String>) {
    while let Some(line) = rx.recv().await {
        if stdin.write_all(line.as_bytes()).await.is_err() {
            break;
        }
        if stdin.write_all(b"\n").await.is_err() {
            break;
        }
        if stdin.flush().await.is_err() {
            break;
        }
    }
}

/// Reads NDJSON lines from child stdout and parses into ClaudeEvent.
async fn stdout_reader(stdout: ChildStdout, tx: mpsc::Sender<ClaudeEvent>) {
    let mut reader = BufReader::new(stdout).lines();
    while let Ok(Some(line)) = reader.next_line().await {
        if let Ok(event) = serde_json::from_str::<ClaudeEvent>(&line) {
            if tx.send(event).await.is_err() {
                break;
            }
        }
    }
}

// ============================================================================
// Prompt Formatting
// ============================================================================

/// Format a task assignment prompt with context.
fn format_task_prompt(task: &Task, context: &TaskContext) -> String {
    let mut prompt = String::new();

    // Manager role preamble
    prompt.push_str("## Your Role: Queen Manager\n\n");
    prompt.push_str("You are a MANAGER. Do NOT implement anything yourself.\n");
    prompt.push_str("Decompose the task below into subtasks and spawn worker agents using the Task tool.\n");
    prompt.push_str("Available agent types: rust-implementer, implementer, research-agent, rust-expert, Explore.\n");
    prompt.push_str("Launch independent agents in PARALLEL. Only serialize when there are dependencies.\n\n");

    // Task details
    prompt.push_str(&format!("## Task: {}\n\n{}", task.id.0, task.description));

    if !context.knowledge.is_empty() {
        prompt.push_str("\n\n## Context Knowledge\n");
        for (key, value) in &context.knowledge {
            prompt.push_str(&format!("- {}: {}\n", key, value));
        }
    }

    if !context.shared_state.is_empty() {
        prompt.push_str("\n\n## Shared State\n");
        for (key, value) in &context.shared_state {
            prompt.push_str(&format!("- {}: {}\n", key, value));
        }
    }

    prompt
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::types::TaskStatus;
    use chrono::Utc;
    use std::collections::HashMap;

    #[test]
    fn test_config_creation() {
        let config = StreamQueenConfig::new(
            QueenId("Q0".to_string()),
            PathBuf::from("/tmp/test"),
        );
        assert_eq!(config.id.0, "Q0");
        assert_eq!(config.model, "sonnet");
        assert!(config.max_turns.is_none());
    }

    #[test]
    fn test_format_task_prompt_basic() {
        let task = Task {
            id: TaskId("T1".to_string()),
            description: "Implement feature X".to_string(),
            status: TaskStatus::Ready,
            assigned_to: None,
            priority: 100,
            blocked_by: vec![],
            created_at: Utc::now(),
        };
        let context = TaskContext {
            knowledge: HashMap::new(),
            recent_messages: vec![],
            shared_state: HashMap::new(),
        };

        let prompt = format_task_prompt(&task, &context);
        assert!(prompt.contains("MANAGER"));
        assert!(prompt.contains("Task tool"));
        assert!(prompt.contains("Task: T1"));
        assert!(prompt.contains("Implement feature X"));
    }

    #[test]
    fn test_format_task_prompt_with_context() {
        let task = Task {
            id: TaskId("T2".to_string()),
            description: "Fix bug Y".to_string(),
            status: TaskStatus::Ready,
            assigned_to: None,
            priority: 200,
            blocked_by: vec![],
            created_at: Utc::now(),
        };

        let mut knowledge = HashMap::new();
        knowledge.insert(
            "repo_path".to_string(),
            serde_json::Value::String("/path/to/repo".to_string()),
        );

        let mut shared_state = HashMap::new();
        shared_state.insert("branch".to_string(), "feature-y".to_string());

        let context = TaskContext {
            knowledge,
            recent_messages: vec![],
            shared_state,
        };

        let prompt = format_task_prompt(&task, &context);
        assert!(prompt.contains("MANAGER"));
        assert!(prompt.contains("Task tool"));
        assert!(prompt.contains("Task: T2"));
        assert!(prompt.contains("Context Knowledge"));
        assert!(prompt.contains("repo_path"));
        assert!(prompt.contains("Shared State"));
        assert!(prompt.contains("branch"));
    }
}
