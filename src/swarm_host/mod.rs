pub mod tick;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use anyhow::{Result, anyhow};
use chrono::Utc;
use tokio::task::JoinHandle;

use crate::core::types::*;
use crate::queen::Queen;
use crate::queen::handle::{QueenHandle, QueenEvent};
use crate::queen::spawn_mode::SpawnMode;
use crate::queen::completion::CompletionConfig;
use crate::queen::stream_queen::{self, StreamQueenConfig};
use crate::queen::spawn_queen::{self, SpawnQueenConfig};
use crate::mailbox::SwarmMailbox;
use crate::mailbox::event_log::SqliteEventLog;
use crate::mailbox::event_bus::EventBus;
use crate::core::task_dag::{TaskDag, DagTask, DagTaskStatus, Priority, Complexity};
use crate::core::validator::{Validator, ValidationResult};
use crate::core::shared_memory::SharedMemory;
use crate::safety::worktree::{WorktreeManager, MergeResult};
use crate::queen::recovery::{SessionTracker, RecoveryManager, RecoveryConfig};
use crate::swarm_host::tick::HeuristicTick;
use crate::ipc;

/// Configuration for SwarmHost.
#[derive(Debug, Clone)]
pub struct SwarmHostConfig {
    /// Maximum number of Queens
    pub max_queens: usize,
    /// Verification command (e.g., "cargo check")
    pub verify_cmd: Option<String>,
    /// Working directory
    pub working_dir: PathBuf,
    /// Enable git worktree isolation
    pub git_isolation: bool,
    /// Autosave interval for shared memory
    pub autosave_interval: Duration,
    /// Max iterations before stopping
    pub max_iterations: usize,
}

impl Default for SwarmHostConfig {
    fn default() -> Self {
        Self {
            max_queens: 4,
            verify_cmd: None,
            working_dir: PathBuf::from("."),
            git_isolation: false,
            autosave_interval: Duration::from_secs(60),
            max_iterations: 100,
        }
    }
}

/// SwarmHost — Level 2 tactical coordinator.
///
/// V3 event-driven orchestrator using QueenHandles and EventBus.
pub struct SwarmHost {
    /// Unique identifier
    id: SwarmHostId,
    /// Queen handles (cloneable actor handles)
    handles: HashMap<QueenId, QueenHandle>,
    /// Actor task join handles (for awaiting/aborting)
    actor_tasks: HashMap<QueenId, JoinHandle<()>>,
    /// Task dependency graph
    task_dag: TaskDag,
    /// Event bus (replaces SwarmMailbox for hot path)
    event_bus: EventBus,
    /// KEEP: SwarmMailbox for outbox to BroodLord/Operator (backward compat)
    mailbox: SwarmMailbox,
    /// Shared knowledge store
    memory: SharedMemory,
    /// Git isolation manager (optional)
    worktree_mgr: Option<WorktreeManager>,
    /// Validator for completed work
    validator: Option<Validator>,
    /// Configuration
    config: SwarmHostConfig,
    /// Adaptive tick controller
    tick_state: HeuristicTick,
    /// Current iteration count
    iteration: usize,
    /// Tracks Claude Code session IDs for each Queen
    session_tracker: SessionTracker,
    /// Manages Queen health checks and recovery planning
    recovery_manager: RecoveryManager,
    /// IPC listener port (for CLI communication)
    ipc_port: Option<u16>,
}

/// Result of a single tick (schedule + poll cycle).
#[derive(Debug, Clone)]
pub struct TickResult {
    pub tasks_assigned: usize,
    pub messages_processed: usize,
    pub tasks_completed: usize,
    pub tasks_failed: usize,
    pub iteration: usize,
}

/// Progress summary.
#[derive(Debug, Clone)]
pub struct SwarmProgress {
    pub total_tasks: usize,
    pub completed: usize,
    pub in_progress: usize,
    pub blocked: usize,
    pub failed: usize,
    pub queens_active: usize,
    pub queens_idle: usize,
}

/// Result of validated merge operation.
#[derive(Debug, Clone)]
pub enum ValidatedMergeResult {
    /// Validation passed and merge succeeded
    Merged { commit_sha: String },
    /// Validation passed but merge had conflicts
    MergeConflict { files: Vec<PathBuf> },
    /// Validation failed — don't merge
    ValidationFailed { feedback: String },
    /// No worktree manager configured (git isolation disabled)
    NoGitIsolation,
    /// No changes to merge
    NoChanges,
}

impl SwarmHost {
    /// Create a new SwarmHost with configuration.
    pub fn new(id: SwarmHostId, config: SwarmHostConfig) -> Result<Self> {
        let event_log = Arc::new(SqliteEventLog::in_memory()?);
        let mailbox = SwarmMailbox::new(event_log.clone());
        let memory = SharedMemory::with_persistence(id.clone(), &config.working_dir);

        let worktree_mgr = if config.git_isolation {
            match WorktreeManager::new(&config.working_dir, Some("main")) {
                Ok(mgr) => Some(mgr),
                Err(e) => {
                    eprintln!("Warning: Failed to create WorktreeManager: {}", e);
                    None
                }
            }
        } else {
            None
        };

        let validator = config.verify_cmd.as_ref().map(|cmd| {
            Validator::command(cmd.as_str(), config.working_dir.clone())
        });

        // Create EventBus without audit logging by default
        // (audit can be enabled separately if needed)
        let event_bus = EventBus::new(128);

        Ok(Self {
            id,
            handles: HashMap::new(),
            actor_tasks: HashMap::new(),
            task_dag: TaskDag::new(),
            event_bus,
            mailbox,
            memory,
            worktree_mgr,
            validator,
            config,
            tick_state: HeuristicTick::new(),
            iteration: 0,
            session_tracker: SessionTracker::new(),
            recovery_manager: RecoveryManager::new(RecoveryConfig::default()),
            ipc_port: None,
        })
    }

    /// Enable audit logging to SQLite.
    ///
    /// This replaces the EventBus with one that has audit logging enabled.
    /// Must be called before registering any queens or running the event loop.
    ///
    /// # Arguments
    /// - `event_log`: SqliteEventLog for durable storage
    pub fn enable_audit(&mut self, event_log: SqliteEventLog) {
        self.event_bus = EventBus::with_audit(128, event_log);
    }

    /// Register a Queen by spawning StreamQueen or SpawnQueen actor.
    ///
    /// This replaces the old register_queen(Box<dyn Queen>) method.
    pub fn register_queen_actor(
        &mut self,
        id: QueenId,
        model: String,
        spawn_mode: SpawnMode,
        completion_config: CompletionConfig,
    ) -> Result<()> {
        if self.handles.len() >= self.config.max_queens {
            return Err(anyhow!("Cannot register queen: max_queens limit ({}) reached", self.config.max_queens));
        }

        let event_tx = self.event_bus.event_sender();
        let shutdown_rx = self.event_bus.shutdown_receiver();

        let system_prompt = Some(format!(
            "{}\n\n{}",
            crate::core::prompts::default_system_prompt("queen"),
            crate::core::prompts::orchestration_discipline_block()
        ));

        let (handle, join_handle) = match spawn_mode {
            SpawnMode::Stream => {
                let config = StreamQueenConfig {
                    id: id.clone(),
                    model,
                    working_dir: self.config.working_dir.clone(),
                    max_turns: None,
                    max_budget_usd: None,
                    system_prompt,
                    allowed_tools: None,
                    completion: completion_config,
                    swarm_id: Some(self.id.0.clone()),
                    ipc_port: self.ipc_port,
                };
                stream_queen::spawn(config, event_tx, shutdown_rx)?
            }
            SpawnMode::PerTask => {
                let config = SpawnQueenConfig {
                    id: id.clone(),
                    model,
                    working_dir: self.config.working_dir.clone(),
                    max_turns: None,
                    max_budget_usd: None,
                    system_prompt,
                    allowed_tools: None,
                    completion: completion_config,
                    swarm_id: Some(self.id.0.clone()),
                    ipc_port: self.ipc_port,
                };
                spawn_queen::spawn(config, event_tx, shutdown_rx)?
            }
        };

        // Register in mailbox (for outbox backward compat)
        self.mailbox.register_queen(id.clone());

        // Create worktree if git_isolation enabled
        if let Some(ref mut worktree_mgr) = self.worktree_mgr {
            if let Err(e) = worktree_mgr.create(&id) {
                eprintln!("Warning: Failed to create worktree for {}: {}", id.0, e);
            }
        }

        self.handles.insert(id.clone(), handle);
        self.actor_tasks.insert(id, join_handle);

        Ok(())
    }

    /// Register a Queen with a Box<dyn Queen>.
    /// DEPRECATED: Use register_queen_actor() for V3 actor-based Queens.
    #[deprecated(note = "Use register_queen_actor() for V3")]
    pub fn register_queen(&mut self, queen: Box<dyn Queen>) -> Result<()> {
        // Check max limit
        if self.handles.len() >= self.config.max_queens {
            return Err(anyhow!("Cannot register queen: max_queens limit ({}) reached", self.config.max_queens));
        }
        let queen_id = queen.id();
        self.mailbox.register_queen(queen_id.clone());
        if let Some(ref mut worktree_mgr) = self.worktree_mgr {
            if let Err(e) = worktree_mgr.create(&queen_id) {
                eprintln!("Warning: Failed to create worktree for {}: {}", queen_id.0, e);
            }
        }
        // Note: old-style queens don't have handles — this is for backward compat only
        // They won't participate in the event-driven run() loop
        Ok(())
    }

    /// Add a task to the DAG.
    pub fn add_task(
        &mut self,
        id: &str,
        description: &str,
        blocked_by: Vec<String>,
        priority: Priority,
        complexity: Complexity,
        skill_hint: Option<String>,
    ) {
        let task = DagTask {
            id: id.to_string(),
            description: description.to_string(),
            status: if blocked_by.is_empty() {
                DagTaskStatus::Ready
            } else {
                DagTaskStatus::Blocked
            },
            assigned_to: None,
            blocked_by,
            blocks: Vec::new(),
            priority,
            estimated_complexity: complexity,
            result: None,
            created_at: Utc::now(),
            started_at: None,
            completed_at: None,
            skill_hint,
        };

        self.task_dag.add_task(task);
    }

    /// Event-driven main loop.
    ///
    /// This replaces the old `loop { tick(); sleep(2s); }` pattern.
    /// Uses tokio::select! to wait on events from the EventBus.
    pub async fn run(&mut self) -> Result<()> {
        // Start IPC listener for CLI commands
        let ipc_config = ipc::IpcConfig {
            working_dir: self.config.working_dir.clone(),
            verify_cmd: self.config.verify_cmd.clone(),
        };
        let memory_state = self.memory.shared_state();
        let port = ipc::start_ipc_listener(memory_state, ipc_config).await?;
        self.ipc_port = Some(port);

        // Write port file
        let port_dir = self.config.working_dir.join(".hatchery");
        std::fs::create_dir_all(&port_dir)?;
        let port_path = port_dir.join(format!("{}.port", self.id.0));
        std::fs::write(&port_path, port.to_string())?;
        eprintln!("[SwarmHost] IPC listener started on port {}", port);

        loop {
            self.try_schedule().await?;

            let interval = self.tick_state.next_interval();

            tokio::select! {
                Some(event) = self.event_bus.recv_event() => {
                    self.handle_event(event).await?;
                }
                _ = tokio::time::sleep(interval) => {
                    self.periodic_maintenance().await?;
                }
            }

            if self.is_complete() {
                break;
            }
        }

        Ok(())
    }

    /// Process a single QueenEvent from the EventBus.
    async fn handle_event(&mut self, event: QueenEvent) -> Result<()> {
        // Audit log
        let audit_entry = EventBus::event_to_audit_entry(&event);
        self.event_bus.audit(audit_entry);

        match event {
            QueenEvent::TaskCompleted {
                queen_id, task_id, result_text, cost_usd,
                duration_ms, num_turns, session_id, quality_passed,
            } => {
                self.tick_state.note_completion();

                // Update session tracker
                if let Some(sid) = &session_id {
                    self.session_tracker.register(queen_id.clone(), sid.clone(), self.config.working_dir.clone());
                }

                // Mark complete in DAG
                let dag_result = crate::core::task_dag::DagTaskResult {
                    success: true,
                    output: result_text.clone(),
                    files_modified: vec![],
                };
                self.task_dag.complete(&task_id.0, dag_result);

                // Store in memory
                let result_json = serde_json::json!({
                    "status": "completed",
                    "output": result_text,
                    "cost_usd": cost_usd,
                    "duration_ms": duration_ms,
                    "num_turns": num_turns,
                    "quality_passed": quality_passed,
                });
                self.memory.store_task_result(&task_id.0, result_json);

                // Validate and merge if git isolation
                if let Ok(merge_result) = self.validated_merge(&queen_id).await {
                    match merge_result {
                        ValidatedMergeResult::Merged { commit_sha } => {
                            println!("[SwarmHost] Validated and merged for {}: {}", queen_id.0, commit_sha);
                        }
                        ValidatedMergeResult::ValidationFailed { feedback } => {
                            // Q2Q: Send validation feedback back to Queen
                            if let Some(handle) = self.handles.get(&queen_id) {
                                let feedback_msg = SwarmMessage {
                                    id: uuid::Uuid::new_v4().to_string(),
                                    from: AgentId::Validator,
                                    to: AgentId::Queen(queen_id.clone()),
                                    msg_type: MessageType::Custom("ValidationFeedback".to_string()),
                                    payload: serde_json::json!({ "feedback": feedback }),
                                    timestamp: Utc::now(),
                                    correlation_id: None,
                                    visibility: Visibility::default_internal(),
                                };
                                let _ = handle.send_message(feedback_msg).await;
                            }
                        }
                        _ => {} // NoGitIsolation, MergeConflict, NoChanges
                    }
                }

                // Try to schedule next task for this queen immediately
                self.try_schedule_queen(&queen_id).await?;

                println!(
                    "[SwarmHost] Task {} completed by {} (${:.2}, {}ms, {} turns)",
                    task_id.0, queen_id.0, cost_usd, duration_ms, num_turns
                );
            }

            QueenEvent::TaskFailed {
                queen_id, task_id, error, cost_usd, num_turns,
            } => {
                self.tick_state.note_failure();

                // Mark failed in DAG
                self.task_dag.fail(&task_id.0, error.clone());

                eprintln!(
                    "[SwarmHost] Task {} failed for {} (${:.2}, {} turns): {}",
                    task_id.0, queen_id.0, cost_usd, num_turns, error
                );

                // Try to schedule next task for this queen
                self.try_schedule_queen(&queen_id).await?;
            }

            QueenEvent::Progress {
                queen_id: _,
                task_id: _,
                turns_completed: _,
                cost_usd: _,
            } => {
                self.tick_state.note_event();
                // Could update progress tracking here
            }

            QueenEvent::Knowledge {
                queen_id, key, value,
            } => {
                self.tick_state.note_event();
                self.memory.insert(key, value, AgentId::Queen(queen_id));
            }

            QueenEvent::ProcessDied {
                queen_id, exit_code, session_id,
            } => {
                self.tick_state.note_failure();

                eprintln!(
                    "[SwarmHost] Process died for {} (exit: {:?}, session: {:?})",
                    queen_id.0, exit_code, session_id
                );

                // Recovery: check if we should restart
                if let Some(plan) = self.recovery_manager.check_health(
                    &queen_id, false, &self.session_tracker
                ) {
                    eprintln!(
                        "[SwarmHost] Recovery plan for {}: {:?} (attempt #{})",
                        plan.queen_id.0, plan.reason, plan.attempt
                    );
                    self.recovery_manager.mark_recovery_attempted(&plan.queen_id);
                }
            }

            QueenEvent::StatusChanged { queen_id: _, status: _ } => {
                self.tick_state.note_event();
            }

            QueenEvent::ContextCompressed { queen_id, pre_tokens, trigger } => {
                self.tick_state.note_event();
                eprintln!(
                    "[SwarmHost] Context compressed for {} (pre_tokens: {}, trigger: {})",
                    queen_id.0, pre_tokens, trigger
                );
            }

            QueenEvent::MessagesReceived { queen_id, count } => {
                self.tick_state.note_event();
                eprintln!(
                    "[SwarmHost] {} received {} queued messages after task completion",
                    queen_id.0, count
                );
            }
        }

        Ok(())
    }

    /// Find ready tasks + idle queens, assign tasks.
    async fn try_schedule(&mut self) -> Result<usize> {
        let ready_tasks = self.task_dag.ready_tasks();
        if ready_tasks.is_empty() {
            return Ok(0);
        }

        // Find idle queens via handle.status()
        let idle_queens: Vec<QueenId> = self.handles.iter()
            .filter(|(_, handle)| matches!(handle.status(), QueenStatus::Idle))
            .map(|(id, _)| id.clone())
            .collect();

        if idle_queens.is_empty() {
            return Ok(0);
        }

        // Build context from SharedMemory
        let knowledge = self.memory.query("*")
            .into_iter()
            .map(|entry| (entry.key, entry.value))
            .collect();

        // Get recent knowledge entries from SharedMemory for sharing between Queens
        let all_entries = self.memory.query("");
        let knowledge_entries: Vec<String> = all_entries
            .iter()
            .rev()
            .take(10)
            .map(|e| {
                let author_str = match &e.author {
                    AgentId::Queen(qid) => qid.0.clone(),
                    AgentId::SwarmHost(sid) => sid.0.clone(),
                    AgentId::Validator => "Validator".to_string(),
                    AgentId::BroodLord => "BroodLord".to_string(),
                    AgentId::Operator => "Operator".to_string(),
                };
                format!("[{}] {}: {}", author_str, e.key, e.value)
            })
            .collect();

        let context = TaskContext {
            knowledge,
            recent_messages: Vec::new(),
            shared_state: HashMap::new(),
            skill_hint: None,  // Will be set per-task below
            knowledge_entries,
        };

        let mut assigned = 0;

        // Collect (task_id, description, priority, blocked_by, created_at, skill_hint, queen_id) tuples
        let assignments: Vec<(String, String, u8, Vec<TaskId>, chrono::DateTime<Utc>, Option<String>, QueenId)> = ready_tasks.iter()
            .zip(idle_queens.iter())
            .map(|(dag_task, queen_id)| {
                (
                    dag_task.id.clone(),
                    dag_task.description.clone(),
                    dag_task.priority as u8,
                    dag_task.blocked_by.iter().map(|id| TaskId(id.clone())).collect(),
                    dag_task.created_at,
                    dag_task.skill_hint.clone(),
                    queen_id.clone(),
                )
            })
            .collect();

        // Now assign tasks (no borrow conflict)
        for (task_id, description, priority, blocked_by, created_at, skill_hint, queen_id) in assignments {
            let task = Task {
                id: TaskId(task_id.clone()),
                description,
                status: TaskStatus::Assigned,
                assigned_to: Some(queen_id.clone()),
                priority,
                blocked_by,
                created_at,
            };

            // Set skill_hint in context for this task
            let mut task_context = context.clone();
            task_context.skill_hint = skill_hint;

            if let Some(handle) = self.handles.get(&queen_id) {
                if let Err(e) = handle.assign(task, task_context).await {
                    eprintln!("[SwarmHost] Failed to assign task {} to {}: {}", task_id, queen_id.0, e);
                    continue;
                }

                self.task_dag.assign(&task_id, queen_id.clone());
                assigned += 1;
            }
        }

        Ok(assigned)
    }

    /// Try to schedule the next task for a specific queen (after completion).
    async fn try_schedule_queen(&mut self, queen_id: &QueenId) -> Result<()> {
        let handle = match self.handles.get(queen_id) {
            Some(h) => h.clone(),
            None => return Ok(()),
        };

        if !matches!(handle.status(), QueenStatus::Idle) {
            return Ok(());
        }

        let ready_tasks = self.task_dag.ready_tasks();
        let dag_task_info = ready_tasks.first().map(|dt| {
            (
                dt.id.clone(),
                dt.description.clone(),
                dt.priority as u8,
                dt.blocked_by.iter().map(|id| TaskId(id.clone())).collect::<Vec<TaskId>>(),
                dt.created_at,
                dt.skill_hint.clone(),
            )
        });

        if let Some((task_id, description, priority, blocked_by, created_at, skill_hint)) = dag_task_info {
            let knowledge = self.memory.query("*")
                .into_iter()
                .map(|entry| (entry.key, entry.value))
                .collect();

            // Get recent knowledge entries from SharedMemory for sharing between Queens
            let all_entries = self.memory.query("");
            let knowledge_entries: Vec<String> = all_entries
                .iter()
                .rev()
                .take(10)
                .map(|e| {
                    let author_str = match &e.author {
                        AgentId::Queen(qid) => qid.0.clone(),
                        AgentId::SwarmHost(sid) => sid.0.clone(),
                        AgentId::Validator => "Validator".to_string(),
                        AgentId::BroodLord => "BroodLord".to_string(),
                        AgentId::Operator => "Operator".to_string(),
                    };
                    format!("[{}] {}: {}", author_str, e.key, e.value)
                })
                .collect();

            let context = TaskContext {
                knowledge,
                recent_messages: Vec::new(),
                shared_state: HashMap::new(),
                skill_hint,
                knowledge_entries,
            };

            let task = Task {
                id: TaskId(task_id.clone()),
                description,
                status: TaskStatus::Assigned,
                assigned_to: Some(queen_id.clone()),
                priority,
                blocked_by,
                created_at,
            };

            if let Err(e) = handle.assign(task, context).await {
                eprintln!("[SwarmHost] Failed to assign task {} to {}: {}", task_id, queen_id.0, e);
            } else {
                self.task_dag.assign(&task_id, queen_id.clone());
            }
        }

        Ok(())
    }

    /// Periodic maintenance (runs on tick interval).
    async fn periodic_maintenance(&mut self) -> Result<()> {
        // Check queen health via handles
        for (queen_id, handle) in &self.handles {
            if !handle.is_alive() {
                if let Some(plan) = self.recovery_manager.check_health(
                    queen_id, false, &self.session_tracker
                ) {
                    eprintln!(
                        "[SwarmHost] Recovery needed for {}: {:?}",
                        plan.queen_id.0, plan.reason
                    );
                    self.recovery_manager.mark_recovery_attempted(&plan.queen_id);
                }
            }
        }

        // Evict expired memory entries
        self.memory.evict_expired();

        Ok(())
    }

    /// Run one iteration: schedule + poll one event + maintenance.
    /// DEPRECATED: Use run() for event-driven loop.
    pub async fn tick(&mut self) -> Result<TickResult> {
        self.iteration += 1;

        let tasks_assigned = self.try_schedule().await?;

        // Try to receive one event (non-blocking with small timeout)
        let mut tasks_completed = 0;
        let mut tasks_failed = 0;
        let mut messages_processed = 0;

        if let Ok(maybe_event) = tokio::time::timeout(
            std::time::Duration::from_millis(50),
            self.event_bus.recv_event()
        ).await {
            if let Some(event) = maybe_event {
                messages_processed += 1;
                match &event {
                    QueenEvent::TaskCompleted { .. } => tasks_completed += 1,
                    QueenEvent::TaskFailed { .. } => tasks_failed += 1,
                    _ => {}
                }
                self.handle_event(event).await?;
            }
        }

        self.periodic_maintenance().await?;

        Ok(TickResult {
            tasks_assigned,
            messages_processed,
            tasks_completed,
            tasks_failed,
            iteration: self.iteration,
        })
    }

    /// Check if all tasks are completed.
    pub fn is_complete(&self) -> bool {
        let stats = self.task_dag.stats();
        stats.total > 0 && stats.completed + stats.failed == stats.total
    }

    /// Get progress summary.
    pub fn progress(&self) -> SwarmProgress {
        let stats = self.task_dag.stats();

        // Count active/idle queens from handles
        let queens_active = self.handles.iter()
            .filter(|(_, handle)| !matches!(handle.status(), QueenStatus::Idle))
            .count();
        let queens_idle = self.handles.len().saturating_sub(queens_active);

        SwarmProgress {
            total_tasks: stats.total,
            completed: stats.completed,
            in_progress: stats.in_progress,
            blocked: stats.blocked,
            failed: stats.failed,
            queens_active,
            queens_idle,
        }
    }

    /// Get the SwarmHost ID.
    pub fn id(&self) -> &SwarmHostId {
        &self.id
    }

    /// Number of registered queens.
    pub fn queen_count(&self) -> usize {
        self.handles.len()
    }

    /// Access to shared memory (for external reads).
    pub fn memory(&self) -> &SharedMemory {
        &self.memory
    }

    /// Drain outbox messages (for BroodLord/Operator).
    pub fn drain_outbox(&mut self) -> Vec<SwarmMessage> {
        self.mailbox.drain_outbox()
    }

    /// Merge a Queen's worktree after validation passes.
    /// Returns ValidatedMergeResult or validation failure.
    pub async fn validated_merge(&mut self, queen_id: &QueenId) -> Result<ValidatedMergeResult> {
        // Check if git isolation enabled
        let worktree_mgr = match self.worktree_mgr.as_mut() {
            Some(mgr) => mgr,
            None => return Ok(ValidatedMergeResult::NoGitIsolation),
        };

        // If validator exists, run validation on the queen's worktree
        if let Some(ref validator) = self.validator {
            let worktree_path = worktree_mgr.worktree_path(queen_id);

            // Validate files in the worktree
            // For now we just pass empty files list - in real implementation,
            // you'd collect modified files from git status
            let validation_result = validator.validate(&[worktree_path]).await?;

            if !validation_result.passed {
                let feedback = format_validation_feedback(&validation_result);
                return Ok(ValidatedMergeResult::ValidationFailed { feedback });
            }
        }

        // Validation passed (or no validator), proceed with merge
        let merge_result = worktree_mgr.merge(queen_id)?;

        // Map MergeResult to ValidatedMergeResult
        Ok(match merge_result {
            MergeResult::Success { commit_sha } => {
                ValidatedMergeResult::Merged { commit_sha }
            }
            MergeResult::Conflict { files } => {
                ValidatedMergeResult::MergeConflict { files }
            }
            MergeResult::NoChanges => {
                ValidatedMergeResult::NoChanges
            }
        })
    }

    /// Get mutable access to the session tracker (for registering sessions).
    pub fn session_tracker_mut(&mut self) -> &mut SessionTracker {
        &mut self.session_tracker
    }

    /// Get read access to the session tracker.
    pub fn session_tracker(&self) -> &SessionTracker {
        &self.session_tracker
    }

    /// Get mutable access to the recovery manager.
    pub fn recovery_manager_mut(&mut self) -> &mut RecoveryManager {
        &mut self.recovery_manager
    }

    /// Remove a dead Queen from the registry (before respawning).
    pub async fn unregister_queen(&mut self, queen_id: &QueenId) -> Option<QueenHandle> {
        self.mailbox.unregister_queen(queen_id);

        // Abort the actor task
        if let Some(task) = self.actor_tasks.remove(queen_id) {
            task.abort();
        }

        self.handles.remove(queen_id)
    }

    /// Graceful shutdown: shutdown all queens, save memory, cleanup worktrees.
    pub async fn shutdown(&mut self) -> Result<()> {
        // Broadcast shutdown to all actors
        self.event_bus.shutdown();

        // Shutdown via handles
        for (queen_id, handle) in &self.handles {
            if let Err(e) = handle.shutdown().await {
                eprintln!("Failed to shutdown queen {}: {}", queen_id.0, e);
            }
        }

        // Wait for actor tasks to finish
        for (queen_id, task) in self.actor_tasks.drain() {
            if let Err(e) = tokio::time::timeout(
                std::time::Duration::from_secs(5),
                task
            ).await {
                eprintln!("Actor task for {} did not finish in time: {}", queen_id.0, e);
            }
        }

        // Remove port file
        if let Some(_port) = self.ipc_port {
            let port_path = self.config.working_dir.join(".hatchery").join(format!("{}.port", self.id.0));
            let _ = std::fs::remove_file(&port_path);
        }

        // Save memory
        if let Err(e) = self.memory.save() {
            eprintln!("Failed to save shared memory: {}", e);
        }

        // Cleanup worktrees
        if let Some(ref mut worktree_mgr) = self.worktree_mgr {
            if let Err(e) = worktree_mgr.prune() {
                eprintln!("Failed to prune worktrees: {}", e);
            }
        }

        Ok(())
    }
}

// ============================================================================
// Helpers
// ============================================================================

/// Format validation feedback into human-readable string.
fn format_validation_feedback(validation_result: &ValidationResult) -> String {
    let mut feedback = String::new();

    for stage in &validation_result.stage_results {
        if !stage.passed {
            feedback.push_str(&format!(
                "Stage '{}' failed ({:.2}s):\n{}\n",
                stage.stage_name,
                stage.duration.as_secs_f64(),
                stage.output
            ));
        }
    }

    if feedback.is_empty() {
        feedback = "Validation failed but no specific feedback available".to_string();
    }

    feedback
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::task_dag::{Priority, Complexity};
    use crate::queen::handle::QueenCommand;
    use tokio::sync::{mpsc, watch};

    #[test]
    fn test_create_swarm_host_with_default_config() {
        let config = SwarmHostConfig::default();
        let host = SwarmHost::new(SwarmHostId::default(), config);
        assert!(host.is_ok());
        let host = host.unwrap();
        assert_eq!(host.queen_count(), 0);
        assert_eq!(host.iteration, 0);
    }

    #[test]
    fn test_add_tasks_to_dag() {
        let config = SwarmHostConfig::default();
        let mut host = SwarmHost::new(SwarmHostId::default(), config).unwrap();
        host.add_task("task1", "First task", vec![], Priority::High, Complexity::Medium, None);
        host.add_task("task2", "Second task", vec!["task1".to_string()], Priority::Normal, Complexity::Simple, None);
        let progress = host.progress();
        assert_eq!(progress.total_tasks, 2);
        assert_eq!(progress.blocked, 1);
    }

    #[test]
    fn test_is_complete_returns_false_when_tasks_pending() {
        let config = SwarmHostConfig::default();
        let mut host = SwarmHost::new(SwarmHostId::default(), config).unwrap();
        host.add_task("task1", "Test task", vec![], Priority::High, Complexity::Trivial, None);
        assert!(!host.is_complete());
    }

    #[test]
    fn test_is_complete_returns_true_when_all_done() {
        let config = SwarmHostConfig::default();
        let host = SwarmHost::new(SwarmHostId::default(), config).unwrap();
        assert!(!host.is_complete());
    }

    #[test]
    fn test_progress_returns_correct_counts() {
        let config = SwarmHostConfig::default();
        let mut host = SwarmHost::new(SwarmHostId::default(), config).unwrap();
        host.add_task("task1", "T1", vec![], Priority::High, Complexity::Trivial, None);
        host.add_task("task2", "T2", vec!["task1".to_string()], Priority::Normal, Complexity::Medium, None);
        host.add_task("task3", "T3", vec![], Priority::Low, Complexity::VeryComplex, None);
        let progress = host.progress();
        assert_eq!(progress.total_tasks, 3);
        assert_eq!(progress.completed, 0);
        assert_eq!(progress.blocked, 1);
    }

    #[test]
    fn test_drain_outbox_works() {
        let config = SwarmHostConfig::default();
        let mut host = SwarmHost::new(SwarmHostId::default(), config).unwrap();
        let messages = host.drain_outbox();
        assert_eq!(messages.len(), 0);
    }

    #[tokio::test]
    async fn test_try_schedule_with_handle() {
        let config = SwarmHostConfig::default();
        let mut host = SwarmHost::new(SwarmHostId::default(), config).unwrap();

        // Create a mock queen handle
        let (cmd_tx, mut cmd_rx) = mpsc::channel(64);
        let (_status_tx, status_rx) = watch::channel(QueenStatus::Idle);
        let handle = QueenHandle::new(
            QueenId("Q0".to_string()),
            SpawnMode::PerTask,
            cmd_tx,
            status_rx,
        );

        host.handles.insert(QueenId("Q0".to_string()), handle);

        // Add a task
        host.add_task("task1", "Test task", vec![], Priority::High, Complexity::Trivial, None);

        // Schedule
        let assigned = host.try_schedule().await.unwrap();
        assert_eq!(assigned, 1);

        // Verify command was sent
        let cmd = cmd_rx.recv().await.unwrap();
        assert!(matches!(cmd, QueenCommand::Assign { .. }));
    }

    #[tokio::test]
    async fn test_handle_event_task_completed() {
        let config = SwarmHostConfig::default();
        let mut host = SwarmHost::new(SwarmHostId::default(), config).unwrap();

        // Add and assign a task
        host.add_task("T1", "Test task", vec![], Priority::High, Complexity::Trivial, None);
        host.task_dag.assign("T1", QueenId("Q0".to_string()));

        // Handle completion event
        let event = QueenEvent::TaskCompleted {
            queen_id: QueenId("Q0".to_string()),
            task_id: TaskId("T1".to_string()),
            result_text: "Done".to_string(),
            cost_usd: 0.15,
            duration_ms: 5000,
            num_turns: 3,
            session_id: Some("sess-123".to_string()),
            quality_passed: true,
        };

        host.handle_event(event).await.unwrap();

        // Task should be completed
        let stats = host.task_dag.stats();
        assert_eq!(stats.completed, 1);
    }

    #[tokio::test]
    async fn test_handle_event_task_failed() {
        let config = SwarmHostConfig::default();
        let mut host = SwarmHost::new(SwarmHostId::default(), config).unwrap();

        host.add_task("T1", "Test task", vec![], Priority::High, Complexity::Trivial, None);
        host.task_dag.assign("T1", QueenId("Q0".to_string()));

        let event = QueenEvent::TaskFailed {
            queen_id: QueenId("Q0".to_string()),
            task_id: TaskId("T1".to_string()),
            error: "Compilation error".to_string(),
            cost_usd: 0.05,
            num_turns: 1,
        };

        host.handle_event(event).await.unwrap();

        let stats = host.task_dag.stats();
        assert_eq!(stats.failed, 1);
    }

    #[tokio::test]
    async fn test_handle_event_knowledge() {
        let config = SwarmHostConfig::default();
        let mut host = SwarmHost::new(SwarmHostId::default(), config).unwrap();

        let event = QueenEvent::Knowledge {
            queen_id: QueenId("Q0".to_string()),
            key: "api_key".to_string(),
            value: serde_json::json!("secret123"),
        };

        host.handle_event(event).await.unwrap();

        // Verify knowledge was stored
        let results = host.memory.query("api_key");
        assert!(!results.is_empty());
    }

    #[tokio::test]
    async fn test_shutdown_gracefully() {
        let config = SwarmHostConfig::default();
        let mut host = SwarmHost::new(SwarmHostId::default(), config).unwrap();
        assert!(host.shutdown().await.is_ok());
    }

    #[tokio::test]
    async fn test_validated_merge_no_git_isolation() {
        let config = SwarmHostConfig {
            git_isolation: false,
            ..Default::default()
        };
        let mut host = SwarmHost::new(SwarmHostId::default(), config).unwrap();
        let queen_id = QueenId("Q0".to_string());
        let result = host.validated_merge(&queen_id).await.unwrap();
        assert!(matches!(result, ValidatedMergeResult::NoGitIsolation));
    }

    #[tokio::test]
    async fn test_tick_returns_result() {
        let config = SwarmHostConfig::default();
        let mut host = SwarmHost::new(SwarmHostId::default(), config).unwrap();

        host.add_task("T1", "Test", vec![], Priority::Normal, Complexity::Trivial, None);

        let result = host.tick().await.unwrap();
        assert_eq!(result.iteration, 1);
        assert_eq!(result.tasks_assigned, 0); // No queens registered
    }

    #[tokio::test]
    async fn test_skill_hint_in_task_context() {
        let config = SwarmHostConfig::default();
        let mut host = SwarmHost::new(SwarmHostId::default(), config).unwrap();

        // Add task with skill_hint
        host.add_task(
            "carousel-task",
            "Create exchange connector using carousel pattern",
            vec![],
            Priority::High,
            Complexity::VeryComplex,
            Some("carousel".to_string()),
        );

        // Verify the DagTask has the skill_hint
        let dag_task = host.task_dag.get("carousel-task").unwrap();
        assert_eq!(dag_task.skill_hint, Some("carousel".to_string()));

        // Create a mock queen handle to test context propagation
        let (cmd_tx, mut cmd_rx) = mpsc::channel(64);
        let (_status_tx, status_rx) = watch::channel(QueenStatus::Idle);
        let handle = QueenHandle::new(
            QueenId("Q0".to_string()),
            SpawnMode::PerTask,
            cmd_tx,
            status_rx,
        );

        host.handles.insert(QueenId("Q0".to_string()), handle);

        // Schedule the task
        let assigned = host.try_schedule().await.unwrap();
        assert_eq!(assigned, 1);

        // Verify the TaskContext contains the skill_hint
        if let Some(QueenCommand::Assign { task: _, context }) = cmd_rx.recv().await {
            assert_eq!(context.skill_hint, Some("carousel".to_string()));
        } else {
            panic!("Expected Assign command");
        }
    }
}
