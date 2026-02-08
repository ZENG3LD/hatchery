//! SpawnQueen: Per-task process spawning Queen implementation.
//!
//! Each task gets a fresh Claude Code subprocess with context via `--resume <session_id>`.
//! After task completion, the process exits and the Queen goes back to Idle state.

use crate::core::types::{QueenId, QueenStatus, SwarmMessage, Task, TaskContext};
use crate::queen::completion::{CompletionConfig, CompletionDetector, CompletionSignal, CompletionVerdict};
use crate::queen::handle::{QueenCommand, QueenEvent, QueenHandle};
use crate::queen::spawn_mode::{ClaudeEvent, SpawnMode};
use anyhow::Result;
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command as TokioCommand};
use tokio::sync::{broadcast, mpsc, watch};

/// Configuration for SpawnQueen.
#[derive(Debug, Clone)]
pub struct SpawnQueenConfig {
    pub id: QueenId,
    pub model: String,
    pub working_dir: PathBuf,
    pub max_turns: Option<u32>,
    pub max_budget_usd: Option<f64>,
    pub system_prompt: Option<String>,
    pub completion: CompletionConfig,
    pub allowed_tools: Option<String>,
}

impl Default for SpawnQueenConfig {
    fn default() -> Self {
        Self {
            id: QueenId("spawn-queen-0".to_string()),
            model: "sonnet".to_string(),
            working_dir: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            max_turns: None,
            max_budget_usd: None,
            system_prompt: None,
            completion: CompletionConfig::default(),
            allowed_tools: None,
        }
    }
}

/// Spawn a SpawnQueen actor.
///
/// Returns a cloneable handle and a join handle for the actor task.
pub fn spawn(
    config: SpawnQueenConfig,
    event_tx: mpsc::Sender<QueenEvent>,
    shutdown_rx: broadcast::Receiver<()>,
) -> Result<(QueenHandle, tokio::task::JoinHandle<()>)> {
    let (cmd_tx, cmd_rx) = mpsc::channel::<QueenCommand>(64);
    let (status_tx, status_rx) = watch::channel(QueenStatus::Idle);

    let handle = QueenHandle::new(
        config.id.clone(),
        SpawnMode::PerTask,
        cmd_tx,
        status_rx,
    );

    let join_handle = tokio::spawn(run_actor(
        config,
        cmd_rx,
        event_tx,
        status_tx,
        shutdown_rx,
    ));

    Ok((handle, join_handle))
}

/// Main actor loop: wait for commands, spawn processes per task.
async fn run_actor(
    config: SpawnQueenConfig,
    mut cmd_rx: mpsc::Receiver<QueenCommand>,
    event_tx: mpsc::Sender<QueenEvent>,
    status_tx: watch::Sender<QueenStatus>,
    mut shutdown_rx: broadcast::Receiver<()>,
) {
    let id = config.id.clone();
    let completion_detector = CompletionDetector::new(config.completion.clone());

    let mut session_id: Option<String> = None;
    let mut queued_messages: Vec<SwarmMessage> = Vec::new();

    eprintln!("[SpawnQueen {}] Started in PerTask mode", id.0);

    loop {
        tokio::select! {
            Some(cmd) = cmd_rx.recv() => {
                match cmd {
                    QueenCommand::Assign { task, context } => {
                        eprintln!("[SpawnQueen {}] Received task: {}", id.0, task.id.0);
                        run_task(
                            &config,
                            &task,
                            &context,
                            &id,
                            &event_tx,
                            &status_tx,
                            &mut session_id,
                            &mut queued_messages,
                            &completion_detector,
                            &mut cmd_rx,
                            &mut shutdown_rx,
                        ).await;

                        // Forum-style: report queued messages after task completion
                        if !queued_messages.is_empty() {
                            let _ = event_tx.send(QueenEvent::MessagesReceived {
                                queen_id: id.clone(),
                                count: queued_messages.len(),
                            }).await;
                        }
                    }
                    QueenCommand::Message(msg) => {
                        eprintln!("[SpawnQueen {}] Queued message from {:?}", id.0, msg.from);
                        queued_messages.push(msg);
                    }
                    QueenCommand::Shutdown => {
                        eprintln!("[SpawnQueen {}] Shutdown requested", id.0);
                        break;
                    }
                }
            }
            _ = shutdown_rx.recv() => {
                eprintln!("[SpawnQueen {}] Shutdown broadcast received", id.0);
                break;
            }
        }
    }

    eprintln!("[SpawnQueen {}] Actor loop exited", id.0);
}

/// Execute a single task by spawning a new Claude Code process.
async fn run_task(
    config: &SpawnQueenConfig,
    task: &Task,
    context: &TaskContext,
    id: &QueenId,
    event_tx: &mpsc::Sender<QueenEvent>,
    status_tx: &watch::Sender<QueenStatus>,
    session_id: &mut Option<String>,
    queued_messages: &mut Vec<SwarmMessage>,
    completion_detector: &CompletionDetector,
    cmd_rx: &mut mpsc::Receiver<QueenCommand>,
    shutdown_rx: &mut broadcast::Receiver<()>,
) {
    // Build the task prompt
    let prompt = format_task_prompt(task, context, queued_messages);
    queued_messages.clear();

    // Update status to Working
    let _ = status_tx.send(QueenStatus::Working {
        task_id: task.id.clone(),
        progress: 0.0,
        sub_tasks: vec![],
    });
    let _ = event_tx.send(QueenEvent::StatusChanged {
        queen_id: id.clone(),
        status: QueenStatus::Working {
            task_id: task.id.clone(),
            progress: 0.0,
            sub_tasks: vec![],
        },
    }).await;

    // Build the command
    let mut cmd = TokioCommand::new("claude");
    cmd.arg("-p");
    cmd.args(["--output-format", "stream-json"]);
    cmd.args(["--verbose", "--dangerously-skip-permissions"]);

    // Resume session if available
    if let Some(sid) = session_id {
        cmd.args(["--resume", sid]);
    }

    // Model
    if !config.model.is_empty() {
        cmd.args(["--model", &config.model]);
    }

    // Max turns
    if let Some(turns) = config.max_turns {
        cmd.args(["--max-turns", &turns.to_string()]);
    }

    // Max budget
    if let Some(budget) = config.max_budget_usd {
        cmd.args(["--max-budget-usd", &budget.to_string()]);
    }

    // System prompt
    if let Some(ref sys_prompt) = config.system_prompt {
        cmd.args(["--append-system-prompt", sys_prompt]);
    }

    // Allowed tools
    if let Some(ref tools) = config.allowed_tools {
        cmd.args(["--allowedTools", tools]);
    }

    // Prompt as positional argument
    cmd.arg(&prompt);

    // Working directory
    cmd.current_dir(&config.working_dir);

    // Capture stdout and stderr
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    eprintln!("[SpawnQueen {}] Spawning process for task {}", id.0, task.id.0);

    // Spawn the process
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[SpawnQueen {}] Failed to spawn process: {}", id.0, e);
            let _ = event_tx.send(QueenEvent::TaskFailed {
                queen_id: id.clone(),
                task_id: task.id.clone(),
                error: format!("Failed to spawn process: {}", e),
                cost_usd: 0.0,
                num_turns: 0,
            }).await;
            let _ = status_tx.send(QueenStatus::Idle);
            return;
        }
    };

    // Spawn stderr logger
    if let Some(stderr) = child.stderr.take() {
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                eprintln!("[Claude stderr] {}", line);
            }
        });
    }

    // Read stdout and process events
    if let Some(stdout) = child.stdout.take() {
        process_stdout(
            stdout,
            &mut child,
            task,
            id,
            event_tx,
            session_id,
            completion_detector,
            queued_messages,
            cmd_rx,
            shutdown_rx,
        ).await;
    }

    // Wait for the process to exit
    let exit_status = match child.wait().await {
        Ok(status) => status,
        Err(e) => {
            eprintln!("[SpawnQueen {}] Error waiting for process: {}", id.0, e);
            let _ = event_tx.send(QueenEvent::ProcessDied {
                queen_id: id.clone(),
                exit_code: None,
                session_id: session_id.clone(),
            }).await;
            let _ = status_tx.send(QueenStatus::Idle);
            return;
        }
    };

    eprintln!("[SpawnQueen {}] Process exited with code {:?}", id.0, exit_status.code());

    // If no result event was received during stdout processing, evaluate ProcessExit
    // (This is a fallback; normally we get a result event)
    let _ = event_tx.send(QueenEvent::ProcessDied {
        queen_id: id.clone(),
        exit_code: exit_status.code(),
        session_id: session_id.clone(),
    }).await;

    // Return to Idle
    let _ = status_tx.send(QueenStatus::Idle);
    let _ = event_tx.send(QueenEvent::StatusChanged {
        queen_id: id.clone(),
        status: QueenStatus::Idle,
    }).await;
}

/// Process stdout from the child process, reading NDJSON events.
async fn process_stdout(
    stdout: impl tokio::io::AsyncRead + Unpin,
    child: &mut Child,
    task: &Task,
    id: &QueenId,
    event_tx: &mpsc::Sender<QueenEvent>,
    session_id: &mut Option<String>,
    completion_detector: &CompletionDetector,
    queued_messages: &mut Vec<SwarmMessage>,
    cmd_rx: &mut mpsc::Receiver<QueenCommand>,
    shutdown_rx: &mut broadcast::Receiver<()>,
) {
    let mut reader = BufReader::new(stdout).lines();
    let mut turn_count: u32 = 0;
    let mut accumulated_cost: f64 = 0.0;

    loop {
        tokio::select! {
            line_result = reader.next_line() => {
                match line_result {
                    Ok(Some(line)) => {
                        // Try to parse as ClaudeEvent
                        match serde_json::from_str::<ClaudeEvent>(&line) {
                            Ok(event) => {
                                // Capture session_id from system init
                                if event.is_system_init() {
                                    if let Some(sid) = &event.session_id {
                                        *session_id = Some(sid.clone());
                                        eprintln!("[SpawnQueen {}] Captured session ID: {}", id.0, sid);
                                    }
                                }

                                // Count turns and track cost
                                if event.is_assistant() {
                                    turn_count += 1;
                                    if let Some(cost) = event.cost() {
                                        accumulated_cost = cost;
                                    }
                                    // Emit progress
                                    let _ = event_tx.send(QueenEvent::Progress {
                                        queen_id: id.clone(),
                                        task_id: task.id.clone(),
                                        turns_completed: turn_count,
                                        cost_usd: accumulated_cost,
                                    }).await;
                                }

                                // Handle result event
                                if event.is_result() {
                                    let subtype = event.subtype.clone().unwrap_or_else(|| "unknown".to_string());
                                    let cost_usd = event.cost().unwrap_or(accumulated_cost);
                                    let duration_ms = event.duration_ms.unwrap_or(0);
                                    let num_turns = event.num_turns.unwrap_or(turn_count);
                                    let result_text = event.result.clone().unwrap_or_default();
                                    let sess_id = event.session_id.clone();

                                    // Capture session ID if present
                                    if let Some(sid) = &sess_id {
                                        *session_id = Some(sid.clone());
                                    }

                                    let signal = CompletionSignal::ResultEvent {
                                        subtype: subtype.clone(),
                                        result_text: Some(result_text.clone()),
                                        cost_usd,
                                        duration_ms,
                                        num_turns,
                                        session_id: sess_id.clone(),
                                    };

                                    let verdict = completion_detector.evaluate(&signal);
                                    emit_verdict(verdict, id, task, event_tx).await;
                                    return; // Task complete, stop reading
                                }
                            }
                            Err(e) => {
                                eprintln!("[SpawnQueen {}] Failed to parse event: {} | Line: {}", id.0, e, line);
                            }
                        }
                    }
                    Ok(None) => {
                        // EOF reached
                        eprintln!("[SpawnQueen {}] Stdout EOF", id.0);
                        break;
                    }
                    Err(e) => {
                        eprintln!("[SpawnQueen {}] Error reading stdout: {}", id.0, e);
                        break;
                    }
                }
            }

            // Handle commands during task execution
            Some(cmd) = cmd_rx.recv() => {
                match cmd {
                    QueenCommand::Message(msg) => {
                        eprintln!("[SpawnQueen {}] Queued message during task execution", id.0);
                        queued_messages.push(msg);
                        // Can't send to running process in PerTask mode
                    }
                    QueenCommand::Shutdown => {
                        eprintln!("[SpawnQueen {}] Shutdown during task — killing process", id.0);
                        let _ = child.kill().await;
                        return;
                    }
                    _ => {
                        // Ignore Assign while busy
                    }
                }
            }

            _ = shutdown_rx.recv() => {
                eprintln!("[SpawnQueen {}] Shutdown broadcast during task — killing process", id.0);
                let _ = child.kill().await;
                return;
            }
        }
    }
}

/// Emit the completion verdict as a QueenEvent.
async fn emit_verdict(
    verdict: CompletionVerdict,
    id: &QueenId,
    task: &Task,
    event_tx: &mpsc::Sender<QueenEvent>,
) {
    match verdict {
        CompletionVerdict::Success {
            result_text,
            cost_usd,
            duration_ms,
            num_turns,
            session_id,
            quality_passed,
        } => {
            eprintln!(
                "[SpawnQueen {}] Task {} completed successfully (cost: ${:.4}, turns: {}, quality: {})",
                id.0, task.id.0, cost_usd, num_turns, quality_passed
            );
            let _ = event_tx.send(QueenEvent::TaskCompleted {
                queen_id: id.clone(),
                task_id: task.id.clone(),
                result_text,
                cost_usd,
                duration_ms,
                num_turns,
                session_id,
                quality_passed,
            }).await;
        }
        CompletionVerdict::Failed { error, cost_usd, num_turns } => {
            eprintln!(
                "[SpawnQueen {}] Task {} failed: {} (cost: ${:.4}, turns: {})",
                id.0, task.id.0, error, cost_usd, num_turns
            );
            let _ = event_tx.send(QueenEvent::TaskFailed {
                queen_id: id.clone(),
                task_id: task.id.clone(),
                error,
                cost_usd,
                num_turns,
            }).await;
        }
        CompletionVerdict::TimedOut { reason } => {
            eprintln!(
                "[SpawnQueen {}] Task {} timed out: {}",
                id.0, task.id.0, reason
            );
            let _ = event_tx.send(QueenEvent::TaskFailed {
                queen_id: id.clone(),
                task_id: task.id.clone(),
                error: format!("Timeout: {}", reason),
                cost_usd: 0.0,
                num_turns: 0,
            }).await;
        }
    }
}

/// Format the task prompt from task, context, and queued messages.
fn format_task_prompt(task: &Task, context: &TaskContext, queued_messages: &[SwarmMessage]) -> String {
    let mut prompt = String::new();

    // Manager role preamble
    prompt.push_str("## Your Role: Queen Manager\n\n");
    prompt.push_str("You are a MANAGER. Do NOT implement anything yourself.\n");
    prompt.push_str("Decompose the task below into subtasks and spawn worker agents using the Task tool.\n");
    prompt.push_str("Available agent types: rust-implementer, implementer, research-agent, rust-expert, Explore.\n");
    prompt.push_str("Launch independent agents in PARALLEL. Only serialize when there are dependencies.\n\n");

    // Orchestration discipline (survives context compression)
    prompt.push_str(&format!("{}\n\n", crate::core::prompts::orchestration_discipline_block()));

    // Skill hint (if provided)
    if let Some(ref hint) = context.skill_hint {
        prompt.push_str(&format!(
            "\n## Recommended Execution Pattern\nUse /{} pattern for this task. Read the skill docs and follow its phases.\n",
            hint
        ));
    }

    // Shared knowledge from other Queens (via file)
    if !context.knowledge_entries.is_empty() {
        prompt.push_str("\n## Shared Knowledge (from other Queens)\n");
        for entry in &context.knowledge_entries {
            prompt.push_str(&format!("- {}\n", entry));
        }
    }

    // Task details
    prompt.push_str(&format!("\n## Task: {}\n\n{}", task.id.0, task.description));

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

    if !queued_messages.is_empty() {
        prompt.push_str("\n\n## Messages from Other Agents\n");
        for msg in queued_messages {
            prompt.push_str(&format!("- From {:?}: {}\n", msg.from, msg.payload));
        }
    }

    prompt
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::types::{TaskStatus, TaskId};
    use chrono::Utc;
    use std::collections::HashMap;

    #[test]
    fn test_spawn_queen_config_default() {
        let config = SpawnQueenConfig::default();
        assert_eq!(config.model, "sonnet");
        assert!(config.max_turns.is_none());
        assert!(config.max_budget_usd.is_none());
    }

    #[test]
    fn test_format_task_prompt() {
        let task = Task {
            id: TaskId("T1".to_string()),
            description: "Test task description".to_string(),
            status: TaskStatus::Ready,
            assigned_to: None,
            priority: 100,
            blocked_by: vec![],
            created_at: Utc::now(),
        };

        let mut context = TaskContext {
            knowledge: HashMap::new(),
            recent_messages: vec![],
            shared_state: HashMap::new(),
            skill_hint: None,
            knowledge_entries: vec![],
        };
        context.knowledge.insert("key1".to_string(), serde_json::json!("value1"));
        context.shared_state.insert("state1".to_string(), "state_value".to_string());

        let prompt = format_task_prompt(&task, &context, &[]);

        assert!(prompt.contains("MANAGER"));
        assert!(prompt.contains("Task tool"));
        assert!(prompt.contains("## Task: T1"));
        assert!(prompt.contains("Test task description"));
        assert!(prompt.contains("## Context Knowledge"));
        assert!(prompt.contains("key1: \"value1\""));
        assert!(prompt.contains("## Shared State"));
        assert!(prompt.contains("state1: state_value"));
    }

    #[tokio::test]
    async fn test_spawn_creates_handle() {
        let config = SpawnQueenConfig::default();
        let (event_tx, _event_rx) = mpsc::channel(10);
        let (_shutdown_tx, shutdown_rx) = broadcast::channel(1);

        let result = spawn(config, event_tx, shutdown_rx);
        assert!(result.is_ok());

        let (handle, join_handle) = result.unwrap();
        assert_eq!(handle.id().0, "spawn-queen-0");
        assert_eq!(handle.spawn_mode(), SpawnMode::PerTask);
        assert!(handle.is_alive());

        // Clean shutdown
        let _ = handle.shutdown().await;
        let _ = join_handle.await;
    }

    #[test]
    fn test_queued_messages_included_in_next_task() {
        use crate::core::types::{AgentId, MessageType, Visibility};

        let task = Task {
            id: TaskId("T1".to_string()),
            description: "Test task".to_string(),
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
            skill_hint: None,
            knowledge_entries: vec![],
        };

        let msg1 = SwarmMessage {
            id: "msg-1".to_string(),
            from: AgentId::Queen(QueenId("Q0".to_string())),
            to: AgentId::Queen(QueenId("Q1".to_string())),
            msg_type: MessageType::TaskResult,
            payload: serde_json::json!({"result": "done"}),
            timestamp: Utc::now(),
            correlation_id: None,
            visibility: Visibility::default_internal(),
        };

        let msg2 = SwarmMessage {
            id: "msg-2".to_string(),
            from: AgentId::Queen(QueenId("Q2".to_string())),
            to: AgentId::Queen(QueenId("Q1".to_string())),
            msg_type: MessageType::StatusRequest,
            payload: serde_json::json!({"status": "checking"}),
            timestamp: Utc::now(),
            correlation_id: None,
            visibility: Visibility::default_internal(),
        };

        let queued_messages = vec![msg1.clone(), msg2.clone()];

        let prompt = format_task_prompt(&task, &context, &queued_messages);

        // Verify the prompt includes the messages section
        assert!(prompt.contains("## Messages from Other Agents"));
        assert!(prompt.contains(&format!("From {:?}:", msg1.from)));
        assert!(prompt.contains(&format!("From {:?}:", msg2.from)));
        assert!(prompt.contains("\"result\":\"done\""));
        assert!(prompt.contains("\"status\":\"checking\""));
    }
}
