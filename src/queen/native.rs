//! NativeQueen — wraps Claude Code CLI via PipeProcess
//! Phase 1.3 implementation

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use async_trait::async_trait;
use anyhow::{Result, Context as AnyhowContext};
use parking_lot::Mutex;
use zengeld_hub_core::{CliTool, PipeProcess, PipeProcessOptions};
use crate::core::types::*;
use crate::core::prompts;
use super::Queen;

/// Configuration for NativeQueen
#[derive(Debug, Clone)]
pub struct NativeQueenConfig {
    /// Model to use: "sonnet", "opus", "haiku"
    pub model: String,
    /// Use native Claude Teams infrastructure
    pub use_teams: bool,
    /// How many sub-agents Queen can spawn
    pub max_workers: usize,
    /// Task timeout
    pub timeout: Duration,
    /// Custom prompt template (optional)
    pub prompt_template: Option<String>,
}

impl Default for NativeQueenConfig {
    fn default() -> Self {
        Self {
            model: "sonnet".to_string(),
            use_teams: false,
            max_workers: 4,
            timeout: Duration::from_secs(600),
            prompt_template: None,
        }
    }
}

impl NativeQueenConfig {
    /// Returns the default prompt template for the Queen
    pub fn default_prompt(&self) -> String {
        format!(
            r#"You are a Queen in the Hatchery swarm system.

Your role:
1. Receive a task from Nydus
2. Break it into {} or fewer sub-tasks
3. Assign sub-tasks to workers using the Task tool
4. Monitor progress and handle failures
5. Report status to Nydus via @hatchery: protocol

Communication protocol:
- Report status: @hatchery:status:<json>
- Report completion: @hatchery:complete:<json>
- Report failures: @hatchery:error:<json>
- Ask questions: @hatchery:escalate:<json>
- Share knowledge: @hatchery:knowledge:<json>

You have access to {} workers.

{}"#,
            self.max_workers,
            self.max_workers,
            crate::core::prompts::orchestration_discipline_block()
        )
    }
}

/// Internal mutable state for NativeQueen
struct NativeQueenState {
    process: Option<PipeProcess>,
    outbox: VecDeque<SwarmMessage>,
    current_task: Option<TaskId>,
    last_activity: Instant,
    cached_status: QueenStatus,
    completed_result: Option<TaskResult>,
    session_id: Option<String>,
}

/// NativeQueen wraps Claude Code CLI with PipeProcess
/// DEPRECATED: Use StreamQueen or SpawnQueen for V3 actor-based implementation.
#[deprecated(note = "Use StreamQueen or SpawnQueen for V3")]
pub struct NativeQueen {
    id: QueenId,
    working_dir: PathBuf,
    config: NativeQueenConfig,
    alive: Arc<AtomicBool>,
    state: Arc<Mutex<NativeQueenState>>,
}

/// Parsed messages from the @hatchery: protocol
#[derive(Debug)]
pub enum ParsedMessage {
    StatusReport(QueenStatus),
    Completion(TaskResult),
    Error(String),
    Escalation { issue: String, severity: Severity },
    Knowledge { key: String, value: serde_json::Value },
    Unknown(String),
}

/// Parser for @hatchery: protocol messages
pub struct HatcheryProtocolParser;

impl HatcheryProtocolParser {
    pub fn parse(&self, line: &str) -> Result<ParsedMessage> {
        if let Some(json_str) = line.strip_prefix("status:") {
            let status: QueenStatus = serde_json::from_str(json_str)
                .context("Failed to parse status JSON")?;
            Ok(ParsedMessage::StatusReport(status))
        } else if let Some(json_str) = line.strip_prefix("complete:") {
            let result: TaskResult = serde_json::from_str(json_str)
                .context("Failed to parse completion JSON")?;
            Ok(ParsedMessage::Completion(result))
        } else if let Some(json_str) = line.strip_prefix("error:") {
            let error: String = serde_json::from_str(json_str)
                .unwrap_or_else(|_| json_str.to_string());
            Ok(ParsedMessage::Error(error))
        } else if let Some(json_str) = line.strip_prefix("escalate:") {
            #[derive(serde::Deserialize)]
            struct EscalationData {
                issue: String,
                severity: Severity,
            }
            let data: EscalationData = serde_json::from_str(json_str)
                .context("Failed to parse escalation JSON")?;
            Ok(ParsedMessage::Escalation {
                issue: data.issue,
                severity: data.severity,
            })
        } else if let Some(json_str) = line.strip_prefix("knowledge:") {
            #[derive(serde::Deserialize)]
            struct KnowledgeData {
                key: String,
                value: serde_json::Value,
            }
            let data: KnowledgeData = serde_json::from_str(json_str)
                .context("Failed to parse knowledge JSON")?;
            Ok(ParsedMessage::Knowledge {
                key: data.key,
                value: data.value,
            })
        } else {
            Ok(ParsedMessage::Unknown(line.to_string()))
        }
    }
}

#[allow(deprecated)]
impl NativeQueen {
    /// Build PipeProcessOptions with discipline rules in --append-system-prompt.
    /// This ensures rules are in the system prompt — NEVER compressed, NEVER ignored.
    fn build_options(config: &NativeQueenConfig, resume_session_id: Option<String>) -> PipeProcessOptions {
        let discipline = prompts::orchestration_discipline_block().to_string();
        PipeProcessOptions {
            append_system_prompt: Some(discipline),
            resume_session_id,
            model: Some(config.model.clone()),
            input_format: None,
            output_format: None,
            max_turns: None,
            max_budget_usd: None,
            json_schema: None,
            allowed_tools: None,
        }
    }

    /// Inject CLAUDE.md as a backup layer (Layer 2: reinforcement).
    /// CLAUDE.md is reloaded after compaction but CAN be ignored by Claude.
    fn inject_claude_md(working_dir: &PathBuf) {
        let claude_dir = working_dir.join(".claude");
        if !claude_dir.exists() {
            let _ = std::fs::create_dir_all(&claude_dir);
        }
        let claude_md_path = claude_dir.join("CLAUDE.md");
        // Only write if not already present (don't overwrite user's CLAUDE.md)
        if !claude_md_path.exists() {
            let rules = format!(
                "# Hatchery Worker Rules\n\n{}\n\n## Worker Protocol\n- Report progress via @hatchery:status after each sub-task\n- Share discoveries via @hatchery:knowledge\n- Escalate blockers immediately via @hatchery:escalate\n- Mark PRD checkboxes as you complete items\n",
                prompts::orchestration_discipline_block()
            );
            let _ = std::fs::write(&claude_md_path, rules);
        }
    }

    /// Spawn a new NativeQueen with the given configuration.
    ///
    /// Anti-degradation: discipline rules are injected at TWO levels:
    /// 1. `--append-system-prompt` — highest priority, never compressed, never ignored
    /// 2. `.claude/CLAUDE.md` — backup reinforcement, reloaded after compaction
    pub fn spawn(
        id: QueenId,
        working_dir: PathBuf,
        config: NativeQueenConfig,
    ) -> Result<Self> {
        Self::spawn_internal(id, working_dir, config, None)
    }

    /// Spawn a NativeQueen that resumes an existing Claude Code session.
    /// Used by RecoveryManager to restore crashed Queens with full context.
    pub fn spawn_with_resume(
        id: QueenId,
        working_dir: PathBuf,
        config: NativeQueenConfig,
        session_id: String,
    ) -> Result<Self> {
        Self::spawn_internal(id, working_dir, config, Some(session_id))
    }

    fn spawn_internal(
        id: QueenId,
        working_dir: PathBuf,
        config: NativeQueenConfig,
        resume_session_id: Option<String>,
    ) -> Result<Self> {
        let prompt = config.prompt_template.as_ref()
            .map(|s| s.clone())
            .unwrap_or_else(|| config.default_prompt());

        // Layer 1: --append-system-prompt (guaranteed, never compressed)
        let options = Self::build_options(&config, resume_session_id);
        let process = PipeProcess::new_with_options(
            CliTool::ClaudeCode, &working_dir, &prompt, options,
        ).context("Failed to spawn PipeProcess")?;

        // Layer 2: CLAUDE.md (reinforcement, reloaded after compaction)
        Self::inject_claude_md(&working_dir);

        let state = NativeQueenState {
            process: Some(process),
            outbox: VecDeque::new(),
            current_task: None,
            last_activity: Instant::now(),
            cached_status: QueenStatus::Idle,
            completed_result: None,
            session_id: None,
        };

        Ok(Self {
            id: id.clone(),
            working_dir,
            config,
            alive: Arc::new(AtomicBool::new(true)),
            state: Arc::new(Mutex::new(state)),
        })
    }

    /// Get the Claude Code session ID (captured from NDJSON system event).
    pub fn session_id(&self) -> Option<String> {
        let state = self.state.lock();
        state.session_id.clone()
    }

    /// Poll messages from the process and update state
    fn poll_messages(&self) -> Result<()> {
        let mut state = self.state.lock();

        // First, collect all available lines and check alive status
        let mut lines = Vec::new();
        let is_running = if let Some(process) = state.process.as_mut() {
            let running = process.is_running();
            while let Some(line) = process.try_recv() {
                lines.push(line);
            }
            running
        } else {
            false
        };

        // Update alive status
        self.alive.store(is_running, Ordering::Relaxed);

        if !is_running && state.process.is_some() {
            // Process died
            return Ok(());
        }

        // Update activity timestamp if we got any lines
        let has_activity = !lines.is_empty();

        // Now process the lines (no longer holding a borrow of process)
        let parser = HatcheryProtocolParser;
        for line in &lines {
            // Try to capture session_id from NDJSON system event
            if state.session_id.is_none() {
                if let Ok(json_val) = serde_json::from_str::<serde_json::Value>(line) {
                    if json_val.get("type").and_then(|v| v.as_str()) == Some("system") {
                        if let Some(sid) = json_val.get("session_id").and_then(|v| v.as_str()) {
                            state.session_id = Some(sid.to_string());
                        }
                    }
                }
            }

            // Look for @hatchery: protocol messages
            if let Some(hatchery_msg) = line.strip_prefix("@hatchery:") {
                match parser.parse(hatchery_msg) {
                    Ok(ParsedMessage::StatusReport(status)) => {
                        state.cached_status = status.clone();
                        let msg = SwarmMessage::status_report(
                            AgentId::Queen(self.id.clone()),
                            AgentId::Nydus(NydusId::default()),
                            status,
                        );
                        state.outbox.push_back(msg);
                    }
                    Ok(ParsedMessage::Completion(result)) => {
                        let task_id = state.current_task.clone().unwrap_or_default();
                        state.cached_status = QueenStatus::Completed {
                            task_id: task_id.clone(),
                        };
                        state.completed_result = Some(result.clone());
                        let msg = SwarmMessage::task_result(
                            AgentId::Queen(self.id.clone()),
                            AgentId::Nydus(NydusId::default()),
                            task_id,
                            result,
                        );
                        state.outbox.push_back(msg);
                    }
                    Ok(ParsedMessage::Error(error)) => {
                        let task_id = state.current_task.clone().unwrap_or_default();
                        state.cached_status = QueenStatus::Failed {
                            task_id,
                            error: error.clone(),
                        };
                        let msg = SwarmMessage::escalation(
                            AgentId::Queen(self.id.clone()),
                            AgentId::Nydus(NydusId::default()),
                            error,
                            Severity::High,
                        );
                        state.outbox.push_back(msg);
                    }
                    Ok(ParsedMessage::Escalation { issue, severity }) => {
                        let msg = SwarmMessage::escalation(
                            AgentId::Queen(self.id.clone()),
                            AgentId::Nydus(NydusId::default()),
                            issue,
                            severity,
                        );
                        state.outbox.push_back(msg);
                    }
                    Ok(ParsedMessage::Knowledge { key, value }) => {
                        let msg = SwarmMessage::knowledge(
                            AgentId::Queen(self.id.clone()),
                            AgentId::Nydus(NydusId::default()),
                            key,
                            value,
                        );
                        state.outbox.push_back(msg);
                    }
                    Ok(ParsedMessage::Unknown(_)) => {
                        // Log and ignore unknown messages
                    }
                    Err(e) => {
                        eprintln!("[NativeQueen] Failed to parse @hatchery message: {}", e);
                    }
                }
            }
        }

        if has_activity {
            state.last_activity = Instant::now();
        }

        Ok(())
    }
}

#[async_trait]
#[allow(deprecated)]
impl Queen for NativeQueen {
    fn id(&self) -> QueenId {
        self.id.clone()
    }

    fn backend(&self) -> QueenBackend {
        if self.config.use_teams {
            QueenBackend::ClaudeNative
        } else {
            QueenBackend::ClaudeRaw
        }
    }

    async fn assign(&mut self, task: Task, context: TaskContext) -> Result<()> {
        let mut state = self.state.lock();

        state.current_task = Some(task.id.clone());
        state.cached_status = QueenStatus::Working {
            task_id: task.id.clone(),
            progress: 0.0,
            sub_tasks: vec![],
        };

        let assignment = serde_json::json!({
            "type": "task_assignment",
            "task": task,
            "context": context,
        });

        let json_str = serde_json::to_string(&assignment)?;

        if let Some(process) = state.process.as_mut() {
            process.write(&json_str)
                .context("Failed to write task assignment to process")?;
            process.write("\n")
                .context("Failed to write newline")?;
        }

        Ok(())
    }

    async fn status(&self) -> QueenStatus {
        let state = self.state.lock();
        state.cached_status.clone()
    }

    async fn result(&self) -> Option<TaskResult> {
        let state = self.state.lock();
        state.completed_result.clone()
    }

    async fn send_message(&mut self, msg: SwarmMessage) -> Result<()> {
        let mut state = self.state.lock();
        let json = serde_json::to_string(&msg)?;

        if let Some(process) = state.process.as_mut() {
            process.write(&json)
                .context("Failed to write message to process")?;
            process.write("\n")
                .context("Failed to write newline")?;
        }

        Ok(())
    }

    async fn drain_outbox(&mut self) -> Vec<SwarmMessage> {
        // First poll for new messages
        let _ = self.poll_messages();

        // Then drain the outbox
        let mut state = self.state.lock();
        state.outbox.drain(..).collect()
    }

    async fn is_alive(&self) -> bool {
        self.alive.load(Ordering::Relaxed)
    }

    async fn shutdown(&mut self) -> Result<()> {
        // Send graceful shutdown signal
        {
            let mut state = self.state.lock();
            if let Some(process) = state.process.as_mut() {
                let _ = process.write("@hatchery:shutdown\n");
            }
        }

        // Wait a bit for graceful shutdown
        tokio::time::sleep(Duration::from_secs(2)).await;

        // Force kill if still running
        {
            let mut state = self.state.lock();
            if let Some(process) = state.process.as_mut() {
                let _ = process.kill();
            }
        }

        self.alive.store(false, Ordering::Relaxed);
        Ok(())
    }
}
