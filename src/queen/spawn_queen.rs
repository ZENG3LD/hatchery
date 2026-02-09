//! SpawnQueen: Per-task process spawning Queen implementation.
//!
//! Each task gets a fresh Claude Code subprocess with context via `--resume <session_id>`.
//! After task completion, the process exits and the Queen goes back to Idle state.

use crate::core::types::{QueenId, QueenStatus, SwarmMessage, Task, TaskContext};
use crate::queen::completion::{CompletionConfig, CompletionDetector, CompletionSignal, CompletionVerdict};
use crate::queen::handle::{QueenCommand, QueenEvent, QueenHandle};
use crate::queen::pipe_process::{PipeProcess, PipeProcessOptions};
use crate::queen::spawn_mode::{ClaudeEvent, SpawnMode};
use anyhow::Result;
use std::path::PathBuf;
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
    /// Swarm ID for IPC identification.
    pub swarm_id: Option<String>,
    /// IPC port for CLI communication.
    pub ipc_port: Option<u16>,
    /// Setting sources for Claude Code (e.g., "user" to skip project CLAUDE.md).
    pub setting_sources: Option<String>,
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
            swarm_id: None,
            ipc_port: None,
            setting_sources: None,
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
    });
    let _ = event_tx.send(QueenEvent::StatusChanged {
        queen_id: id.clone(),
        status: QueenStatus::Working {
            task_id: task.id.clone(),
            progress: 0.0,
        },
    }).await;

    // Build options from config
    let options = PipeProcessOptions {
        append_system_prompt: config.system_prompt.clone(),
        resume_session_id: session_id.clone(),
        model: Some(config.model.clone()),
        max_turns: config.max_turns,
        max_budget_usd: config.max_budget_usd,
        allowed_tools: config.allowed_tools.clone(),
        setting_sources: config.setting_sources.clone(),
        ..Default::default()
    };

    // Build environment variables
    let mut envs = Vec::new();
    if let Some(ref swarm_id) = config.swarm_id {
        envs.push(("HATCHERY_SWARM_ID".to_string(), swarm_id.clone()));
    }
    envs.push(("HATCHERY_QUEEN_ID".to_string(), config.id.0.clone()));
    envs.push(("HATCHERY_WORKING_DIR".to_string(), config.working_dir.to_string_lossy().to_string()));
    if let Some(port) = config.ipc_port {
        envs.push(("HATCHERY_PORT".to_string(), port.to_string()));
    }

    eprintln!("[SpawnQueen {}] Spawning process for task {} (prompt len: {} chars)", id.0, task.id.0, prompt.len());

    // Spawn via PipeProcess
    let mut proc = match PipeProcess::new_with_options(&config.working_dir, &prompt, options, envs) {
        Ok(p) => {
            eprintln!("[SpawnQueen {}] Process spawned OK, is_running={}", id.0, "yes");
            p
        }
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

    // Process output using try_recv in a polling loop
    let mut turn_count: u32 = 0;
    let mut accumulated_cost: f64 = 0.0;

    loop {
        tokio::select! {
            _ = tokio::time::sleep(std::time::Duration::from_millis(50)) => {
                // Drain all available lines
                while let Some(line) = proc.try_recv() {
                    let line = line.trim_end();
                    eprintln!("[SpawnQueen {}] STDOUT: {}...", id.0, &line[..line.len().min(120)]);
                    if line.is_empty() { continue; }

                    // Try to parse as ClaudeEvent
                    match serde_json::from_str::<ClaudeEvent>(line) {
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

                                // Return to Idle
                                let _ = status_tx.send(QueenStatus::Idle);
                                let _ = event_tx.send(QueenEvent::StatusChanged {
                                    queen_id: id.clone(),
                                    status: QueenStatus::Idle,
                                }).await;
                                return;
                            }
                        }
                        Err(e) => {
                            eprintln!("[SpawnQueen {}] Failed to parse event: {} | Line: {}", id.0, e, line);
                        }
                    }
                }

                // Check if process is still running
                if !proc.is_running() {
                    // Drain remaining output
                    while let Some(line) = proc.try_recv() {
                        let line = line.trim_end();
                        if line.is_empty() { continue; }

                        // Parse remaining events
                        match serde_json::from_str::<ClaudeEvent>(line) {
                            Ok(event) => {
                                if event.is_result() {
                                    let subtype = event.subtype.clone().unwrap_or_else(|| "unknown".to_string());
                                    let cost_usd = event.cost().unwrap_or(accumulated_cost);
                                    let duration_ms = event.duration_ms.unwrap_or(0);
                                    let num_turns = event.num_turns.unwrap_or(turn_count);
                                    let result_text = event.result.clone().unwrap_or_default();
                                    let sess_id = event.session_id.clone();

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

                                    // Return to Idle
                                    let _ = status_tx.send(QueenStatus::Idle);
                                    let _ = event_tx.send(QueenEvent::StatusChanged {
                                        queen_id: id.clone(),
                                        status: QueenStatus::Idle,
                                    }).await;
                                    return;
                                }
                            }
                            Err(_) => {}
                        }
                    }

                    // Process died without sending result event
                    eprintln!("[SpawnQueen {}] Process died without result event", id.0);
                    let _ = event_tx.send(QueenEvent::ProcessDied {
                        queen_id: id.clone(),
                        exit_code: None,
                        session_id: session_id.clone(),
                    }).await;

                    // Return to Idle
                    let _ = status_tx.send(QueenStatus::Idle);
                    let _ = event_tx.send(QueenEvent::StatusChanged {
                        queen_id: id.clone(),
                        status: QueenStatus::Idle,
                    }).await;
                    return;
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
                        let _ = proc.kill();
                        let _ = status_tx.send(QueenStatus::Idle);
                        return;
                    }
                    _ => {
                        // Ignore Assign while busy
                    }
                }
            }

            _ = shutdown_rx.recv() => {
                eprintln!("[SpawnQueen {}] Shutdown broadcast during task — killing process", id.0);
                let _ = proc.kill();
                let _ = status_tx.send(QueenStatus::Idle);
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

    // Skill hint (if provided)
    if let Some(ref hint) = context.skill_hint {
        prompt.push_str(&format!(
            "## Recommended Execution Pattern\nUse /{} pattern for this task. Read the skill docs and follow its phases.\n\n",
            hint
        ));
    }

    // Shared knowledge from other Queens (via SharedMemory)
    if !context.knowledge_entries.is_empty() {
        prompt.push_str("## Shared Knowledge (from other Queens)\n");
        for entry in &context.knowledge_entries {
            prompt.push_str(&format!("- {}\n", entry));
        }
        prompt.push_str("\n");
    }

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

        // Static blocks (MANAGER, Task tool) are now in system prompt, not task prompt
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
