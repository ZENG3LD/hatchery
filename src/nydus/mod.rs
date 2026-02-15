pub mod tick;
pub mod mailbox;
pub mod ipc;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use anyhow::{Result, anyhow};
use chrono::Utc;
use tokio::task::JoinHandle;
use tokio::sync::Notify;

use crate::core::types::*;
use crate::queen::handle::{QueenHandle, QueenEvent};
use crate::queen::completion::CompletionConfig;
use crate::queen::stream_queen::{self, StreamQueenConfig};
use self::mailbox::SwarmMailbox;
use self::mailbox::event_log::SqliteEventLog;
use self::mailbox::event_bus::EventBus;
use crate::core::task_dag::{TaskDag, DagTask, DagTaskStatus, Priority, Complexity, DagStats};
use crate::core::validator::{Validator, ValidationResult};
use crate::core::shared_memory::SharedMemory;
use crate::safety::worktree::{WorktreeManager, MergeResult, SyncResult};
use crate::queen::recovery::{SessionTracker, RecoveryManager, RecoveryConfig};
use crate::nydus::tick::HeuristicTick;
use crate::swarm_pool::{SwarmPool, SwarmPoolConfig, SwarmPoolAction};

/// Configuration for Nydus.
#[derive(Debug, Clone)]
pub struct NydusConfig {
    /// Minimum number of Queens in elastic pool
    pub min_queens: usize,
    /// Maximum number of Queens in elastic pool
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
    /// Setting sources for Queens (e.g., "user" to skip project CLAUDE.md).
    pub setting_sources: Option<String>,
    /// Keep-alive mode: don't exit after DAG completion, wait for operator commands
    pub keep_alive: bool,
    /// Path to PRD file (for updating checkboxes)
    pub prd_path: Option<PathBuf>,
    /// Enable Zerg Rush mode (multiple Queens on bottleneck tasks)
    pub zerg_rush_enabled: bool,
    /// Minimum bottleneck score (task must block this many tasks) to trigger zerg rush
    pub zerg_rush_min_bottleneck: usize,
    /// Maximum number of Queens to assign to a single task in zerg rush
    pub zerg_rush_max_queens: usize,
}

impl Default for NydusConfig {
    fn default() -> Self {
        Self {
            min_queens: 3,
            max_queens: 8,
            verify_cmd: None,
            working_dir: PathBuf::from("."),
            git_isolation: false,
            autosave_interval: Duration::from_secs(60),
            max_iterations: 100,
            setting_sources: None,
            keep_alive: false,
            prd_path: None,
            zerg_rush_enabled: true,
            zerg_rush_min_bottleneck: 2,
            zerg_rush_max_queens: 5,
        }
    }
}

/// Nydus — transport & scheduling node.
///
/// V3 event-driven orchestrator using QueenHandles and EventBus.
pub struct Nydus {
    /// Unique identifier
    id: NydusId,
    /// Queen handles (cloneable actor handles)
    handles: HashMap<QueenId, QueenHandle>,
    /// Actor task join handles (for awaiting/aborting)
    actor_tasks: HashMap<QueenId, JoinHandle<()>>,
    /// Task dependency graph
    task_dag: TaskDag,
    /// SwarmPool for spawn heuristics (zerg rush, elastic pool, retry policy)
    swarm_pool: SwarmPool,
    /// Event bus (replaces SwarmMailbox for hot path)
    event_bus: EventBus,
    /// KEEP: SwarmMailbox for outbox to Operator (backward compat)
    /// Wrapped in Arc<Mutex> for sharing with IPC handler
    mailbox: Arc<parking_lot::Mutex<SwarmMailbox>>,
    /// Shared knowledge store
    memory: SharedMemory,
    /// Git isolation manager (optional)
    worktree_mgr: Option<WorktreeManager>,
    /// Validator for completed work
    validator: Option<Validator>,
    /// Configuration
    config: NydusConfig,
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
    /// Queen status snapshots for IPC queries (shared with IPC handler)
    queen_snapshots: Arc<parking_lot::RwLock<Vec<ipc::protocol::QueenStatusSnapshot>>>,
    /// DAG stats snapshot for IPC queries (shared with IPC handler)
    dag_stats: Arc<parking_lot::RwLock<DagStats>>,
    /// Cost tracking snapshot for IPC queries (shared with IPC handler)
    cost_tracking: Arc<parking_lot::RwLock<CostTracking>>,
    /// Shutdown signal sender (for IPC-triggered shutdown)
    shutdown_tx: tokio::sync::watch::Sender<bool>,
    /// Shutdown signal receiver (checked in main loop)
    shutdown_rx: tokio::sync::watch::Receiver<bool>,
    /// Start time for uptime calculation
    started_at: std::time::Instant,
    /// Notify handle for waking Nydus when a Queen completes a task
    wakeup_notify: Arc<Notify>,
    /// Overlord handle for merge validation
    overlord: Option<crate::overlord::OverlordHandle>,
    /// Overlord event receiver (QueenEvent because Overlord is a StreamQueen)
    overlord_event_rx: Option<tokio::sync::mpsc::Receiver<QueenEvent>>,
    /// Total cost across all Queens
    total_queen_cost_usd: f64,
    /// Total cost for Overlord reviews
    total_overlord_cost_usd: f64,
    /// Number of Overlord reviews completed
    overlord_reviews_completed: u32,
    /// Tasks currently under Overlord review (task_id -> winner queen_id)
    in_review: HashMap<String, QueenId>,
    /// Next Queen ID counter for dynamic spawning
    next_queen_id: usize,
    /// Model name for dynamically spawned Queens
    default_model: String,
    /// Completion config for dynamically spawned Queens
    default_completion_config: CompletionConfig,
    /// Tracks rate limit failures for graceful shutdown detection
    rate_limit_window: Vec<std::time::Instant>,
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
    pub total_queen_cost_usd: f64,
    pub total_overlord_cost_usd: f64,
    pub overlord_reviews_completed: u32,
}

/// Cost tracking for Queens and Overlord.
#[derive(Debug, Clone)]
pub struct CostTracking {
    pub total_queen_cost_usd: f64,
    pub total_overlord_cost_usd: f64,
    pub overlord_reviews_completed: u32,
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

impl Nydus {
    /// Build a summary of other tasks being worked on by other Queens.
    /// This helps Queens understand what work is being done in parallel.
    fn build_other_tasks_summary(&self, current_task_id: &str) -> String {
        let mut summary = String::new();

        // Get all tasks from DAG
        let all_tasks = self.task_dag.all_tasks();

        // Filter and format tasks that are assigned or in progress (excluding current task)
        let other_tasks: Vec<String> = all_tasks
            .iter()
            .filter(|t| {
                t.id != current_task_id &&
                (matches!(t.status, crate::core::task_dag::DagTaskStatus::Assigned(_)) ||
                 matches!(t.status, crate::core::task_dag::DagTaskStatus::InProgress) ||
                 matches!(t.status, crate::core::task_dag::DagTaskStatus::ZergRush { .. }))
            })
            .map(|t| {
                let status_str = match &t.status {
                    crate::core::task_dag::DagTaskStatus::Assigned(qid) => format!("ASSIGNED to {}", qid.0),
                    crate::core::task_dag::DagTaskStatus::InProgress => "IN PROGRESS".to_string(),
                    crate::core::task_dag::DagTaskStatus::ZergRush { queens, .. } => {
                        let queen_list = queens.iter()
                            .map(|q| q.0.as_str())
                            .collect::<Vec<_>>()
                            .join(", ");
                        format!("ZERG RUSH by {}", queen_list)
                    }
                    _ => "UNKNOWN".to_string(),
                };
                format!("- {}: {} ({})", t.id, t.description.lines().next().unwrap_or(""), status_str)
            })
            .collect();

        // Add completed tasks (for context)
        let completed_tasks: Vec<String> = all_tasks
            .iter()
            .filter(|t| matches!(t.status, crate::core::task_dag::DagTaskStatus::Completed))
            .map(|t| format!("- {}: COMPLETED", t.id))
            .collect();

        if !other_tasks.is_empty() {
            summary.push_str("Active tasks being handled by other Queens:\n");
            for task in &other_tasks {
                summary.push_str(task);
                summary.push('\n');
            }
        }

        if !completed_tasks.is_empty() {
            summary.push_str("\nCompleted tasks:\n");
            for task in &completed_tasks {
                summary.push_str(task);
                summary.push('\n');
            }
        }

        if summary.is_empty() {
            summary.push_str("No other active tasks at this time.\n");
        }

        summary
    }

    /// Create a new Nydus with configuration.
    pub fn new(id: NydusId, config: NydusConfig) -> Result<Self> {
        let event_log = Arc::new(SqliteEventLog::in_memory()?);
        let mailbox = Arc::new(parking_lot::Mutex::new(SwarmMailbox::new(event_log.clone())));
        let memory = SharedMemory::with_persistence(id.clone(), &config.working_dir);

        let worktree_mgr = if config.git_isolation {
            match WorktreeManager::new(&config.working_dir, None) {
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

        // Create shutdown channel for IPC-triggered shutdown
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

        // Initialize DAG stats with zero values
        let initial_dag_stats = DagStats {
            total: 0,
            blocked: 0,
            ready: 0,
            in_progress: 0,
            validating: 0,
            completed: 0,
            failed: 0,
        };

        // Initialize cost tracking with zero values
        let initial_cost_tracking = CostTracking {
            total_queen_cost_usd: 0.0,
            total_overlord_cost_usd: 0.0,
            overlord_reviews_completed: 0,
        };

        // Create wakeup notify for instant Queen completion handling
        let wakeup_notify = Arc::new(Notify::new());

        // Build SwarmPoolConfig from NydusConfig
        let swarm_pool_config = SwarmPoolConfig {
            min_queens: config.min_queens,
            max_queens: config.max_queens,
            zerg_rush_threshold: config.zerg_rush_min_bottleneck,
            zerg_rush_queens: config.zerg_rush_max_queens,
            max_retries_before_escalate: 1, // First decline → retry, second decline → escalate
        };
        let swarm_pool = SwarmPool::new(swarm_pool_config);

        Ok(Self {
            id,
            handles: HashMap::new(),
            actor_tasks: HashMap::new(),
            task_dag: TaskDag::new(),
            swarm_pool,
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
            queen_snapshots: Arc::new(parking_lot::RwLock::new(Vec::new())),
            dag_stats: Arc::new(parking_lot::RwLock::new(initial_dag_stats)),
            cost_tracking: Arc::new(parking_lot::RwLock::new(initial_cost_tracking)),
            shutdown_tx,
            shutdown_rx,
            started_at: std::time::Instant::now(),
            wakeup_notify,
            overlord: None,
            overlord_event_rx: None,
            total_queen_cost_usd: 0.0,
            total_overlord_cost_usd: 0.0,
            overlord_reviews_completed: 0,
            in_review: HashMap::new(),
            next_queen_id: 3,  // Start at Q3, since we register Q0-Q2 initially
            default_model: "sonnet".to_string(),
            default_completion_config: CompletionConfig::default(),
            rate_limit_window: Vec::new(),
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

    /// Register an Overlord for merge validation.
    ///
    /// This spawns an Overlord actor that reviews Queen-completed tasks before merging.
    ///
    /// # Errors
    /// Returns an error if the Overlord fails to spawn.
    pub fn register_overlord(&mut self, model: &str) -> Result<()> {
        use crate::overlord::{spawn_overlord, OverlordConfig};

        let config = OverlordConfig {
            id: OverlordId("overlord-0".to_string()),
            model: model.to_string(),
            working_dir: self.config.working_dir.clone(),
            wakeup_notify: Some(self.wakeup_notify.clone()),
            swarm_id: Some(self.id.0.clone()),
            ipc_port: self.ipc_port,
            setting_sources: self.config.setting_sources.clone(),
        };

        let shutdown_rx = self.event_bus.shutdown_receiver();
        let (handle, event_rx, _join_handle) = spawn_overlord(config, shutdown_rx)?;

        self.overlord = Some(handle);
        self.overlord_event_rx = Some(event_rx);

        eprintln!("[Nydus] Registered Overlord with model: {}", model);
        Ok(())
    }

    /// Update queen status snapshots by reading from handles.
    fn update_queen_snapshots(&self) {
        use crate::nydus::ipc::protocol::QueenStatusSnapshot;

        let snapshots: Vec<QueenStatusSnapshot> = self.handles
            .iter()
            .map(|(queen_id, handle)| {
                let status = handle.status();
                let (status_str, task_id, progress) = match &status {
                    QueenStatus::Idle => ("idle".to_string(), None, None),
                    QueenStatus::Working { task_id, progress } => {
                        ("working".to_string(), Some(task_id.0.clone()), Some(*progress))
                    }
                    QueenStatus::Blocked { task_id, .. } => {
                        ("blocked".to_string(), Some(task_id.0.clone()), None)
                    }
                    QueenStatus::Failed { task_id, .. } => {
                        ("failed".to_string(), Some(task_id.0.clone()), None)
                    }
                    QueenStatus::Completed { task_id } => {
                        ("completed".to_string(), Some(task_id.0.clone()), None)
                    }
                    QueenStatus::Dead => ("dead".to_string(), None, None),
                };

                QueenStatusSnapshot {
                    id: queen_id.0.clone(),
                    status: status_str,
                    task_id,
                    progress,
                    spawn_mode: "stream".to_string(),
                    is_alive: handle.is_alive(),
                }
            })
            .collect();

        let mut snapshots_lock = self.queen_snapshots.write();
        *snapshots_lock = snapshots;
    }

    /// Update DAG stats snapshot by reading from TaskDag.
    fn update_dag_stats(&self) {
        let stats = self.task_dag.stats();
        let mut dag_stats_lock = self.dag_stats.write();
        *dag_stats_lock = stats;
    }

    /// Update cost tracking snapshot.
    fn update_cost_tracking(&self) {
        let mut cost_lock = self.cost_tracking.write();
        cost_lock.total_queen_cost_usd = self.total_queen_cost_usd;
        cost_lock.total_overlord_cost_usd = self.total_overlord_cost_usd;
        cost_lock.overlord_reviews_completed = self.overlord_reviews_completed;
    }

    /// Register a Queen by spawning StreamQueen or SpawnQueen actor.
    ///
    /// This replaces the old register_queen(Box<dyn Queen>) method.
    pub fn register_queen_actor(
        &mut self,
        id: QueenId,
        model: String,
        completion_config: CompletionConfig,
    ) -> Result<()> {
        if self.handles.len() >= self.config.max_queens {
            return Err(anyhow!("Cannot register queen: max_queens limit ({}) reached", self.config.max_queens));
        }

        let event_tx = self.event_bus.event_sender();
        let shutdown_rx = self.event_bus.shutdown_receiver();

        let system_prompt = Some(format!(
            "{}\n\n{}\n\n{}\n\n{}\n\n{}",
            crate::core::prompts::default_system_prompt("queen"),
            crate::core::prompts::queen_preamble(),
            crate::core::prompts::orchestration_discipline_block(),
            crate::core::prompts::hatchery_cli_tools_block(),
            crate::core::prompts::git_safety_block()
        ));

        // Determine working directory for this Queen
        // If git_isolation is enabled, create worktree BEFORE spawning Queen
        let queen_working_dir = if self.config.git_isolation {
            if let Some(ref mut worktree_mgr) = self.worktree_mgr {
                // Create worktree and get its path
                match worktree_mgr.create(&id) {
                    Ok(worktree_path) => worktree_path,
                    Err(e) => {
                        // CRITICAL: Worktree creation failed — Queen MUST NOT work without git isolation
                        eprintln!("\n========================================");
                        eprintln!("FATAL: Failed to create worktree for {}", id.0);
                        eprintln!("Reason: {}", e);
                        eprintln!("Git isolation is REQUIRED but worktree creation failed");
                        eprintln!("Refusing to register Queen without proper git sandbox");
                        eprintln!("========================================\n");

                        // Send error message to mailbox for Operator visibility
                        let error_msg = SwarmMessage {
                            id: uuid::Uuid::new_v4().to_string(),
                            from: AgentId::Nydus(self.id.clone()),
                            to: AgentId::Operator,
                            msg_type: MessageType::Custom("QueenRegistrationFailed".to_string()),
                            payload: serde_json::json!({
                                "queen_id": id.0,
                                "reason": "worktree_creation_failed",
                                "error": e.to_string(),
                                "git_isolation_required": true,
                            }),
                            timestamp: Utc::now(),
                            correlation_id: None,
                            visibility: Visibility {
                                agent_visible: false,
                                coordinator_visible: true,
                                user_visible: true,
                            },
                        };
                        self.mailbox.lock().send(error_msg);

                        // Return error — DO NOT register this Queen
                        return Err(anyhow!("Queen {} cannot be registered: worktree creation failed: {}", id.0, e));
                    }
                }
            } else {
                // Git isolation is enabled but WorktreeManager is None (initialization failed)
                eprintln!("\n========================================");
                eprintln!("FATAL: Git isolation is enabled but WorktreeManager is not available");
                eprintln!("Cannot register Queen {} without git isolation", id.0);
                eprintln!("========================================\n");

                // Send error message to mailbox
                let error_msg = SwarmMessage {
                    id: uuid::Uuid::new_v4().to_string(),
                    from: AgentId::Nydus(self.id.clone()),
                    to: AgentId::Operator,
                    msg_type: MessageType::Custom("QueenRegistrationFailed".to_string()),
                    payload: serde_json::json!({
                        "queen_id": id.0,
                        "reason": "worktree_manager_unavailable",
                        "error": "Git isolation enabled but WorktreeManager failed to initialize",
                        "git_isolation_required": true,
                    }),
                    timestamp: Utc::now(),
                    correlation_id: None,
                    visibility: Visibility {
                        agent_visible: false,
                        coordinator_visible: true,
                        user_visible: true,
                    },
                };
                self.mailbox.lock().send(error_msg);

                // Return error — DO NOT register this Queen
                return Err(anyhow!("Queen {} cannot be registered: WorktreeManager is not available", id.0));
            }
        } else {
            // Git isolation disabled, use main repo
            self.config.working_dir.clone()
        };

        // Always use StreamQueen (long-lived subprocess with stream-json mode)
        let config = StreamQueenConfig {
            id: id.clone(),
            model,
            working_dir: queen_working_dir.clone(),
            max_turns: None,
            max_budget_usd: None,
            system_prompt,
            allowed_tools: None,
            completion: completion_config,
            swarm_id: Some(self.id.0.clone()),
            ipc_port: self.ipc_port,
            setting_sources: self.config.setting_sources.clone(),
            wakeup_notify: Some(self.wakeup_notify.clone()),
        };
        let (handle, join_handle) = stream_queen::spawn(config, event_tx, shutdown_rx)?;

        // Register in mailbox (for outbox backward compat)
        self.mailbox.lock().register_queen(id.clone());

        self.handles.insert(id.clone(), handle);
        self.actor_tasks.insert(id, join_handle);

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
        verify_cmd: Option<String>,
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
            retry_count: 0,
            rejection_feedback: Vec::new(),
            verify_cmd,
        };

        self.task_dag.add_task(task);
        self.update_dag_stats();
    }

    /// Event-driven main loop.
    ///
    /// This replaces the old `loop { tick(); sleep(2s); }` pattern.
    /// Uses tokio::select! to wait on events from the EventBus.
    pub async fn run(&mut self) -> Result<()> {
        // Create inject channel for runtime task injection
        let (inject_tx, mut inject_rx) = tokio::sync::mpsc::channel::<ipc::protocol::InjectRequest>(32);

        // Create message delivery channel for operator→queen messages
        let (message_delivery_tx, mut message_delivery_rx) = tokio::sync::mpsc::channel::<ipc::protocol::MessageDeliveryNotification>(32);

        // Start IPC listener for CLI commands
        let ipc_config = ipc::IpcConfig {
            working_dir: self.config.working_dir.clone(),
            verify_cmd: self.config.verify_cmd.clone(),
        };
        let memory_state = self.memory.shared_state();
        let port = ipc::start_ipc_listener(
            self.queen_snapshots.clone(),
            self.dag_stats.clone(),
            self.cost_tracking.clone(),
            memory_state,
            self.mailbox.clone(),
            inject_tx,
            message_delivery_tx,
            ipc_config,
            self.shutdown_tx.clone(),
            self.started_at,
            self.config.keep_alive,
        ).await?;
        self.ipc_port = Some(port);

        // Write port file
        let port_dir = self.config.working_dir.join(".hatchery");
        std::fs::create_dir_all(&port_dir)?;
        let port_path = port_dir.join(format!("{}.port", self.id.0));
        std::fs::write(&port_path, port.to_string())?;
        eprintln!("[Nydus] IPC listener started on port {}", port);

        loop {
            self.try_schedule().await?;

            let interval = self.tick_state.next_interval();

            tokio::select! {
                Some(event) = self.event_bus.recv_event() => {
                    self.handle_event(event).await?;
                }
                Some(inject_req) = inject_rx.recv() => {
                    self.handle_inject(inject_req).await;
                }
                Some(msg_notification) = message_delivery_rx.recv() => {
                    self.handle_message_delivery(msg_notification).await;
                }
                Some(event) = async {
                    if let Some(ref mut rx) = self.overlord_event_rx {
                        rx.recv().await
                    } else {
                        std::future::pending().await
                    }
                } => {
                    self.handle_overlord_queen_event(event).await?;
                }
                _ = self.wakeup_notify.notified() => {
                    // Queen completed a task, immediately try to schedule ready tasks
                    self.try_schedule().await?;
                }
                _ = tokio::time::sleep(interval) => {
                    self.periodic_maintenance().await?;
                }
                _ = self.shutdown_rx.changed() => {
                    if *self.shutdown_rx.borrow() {
                        eprintln!("[Nydus] Shutdown signal received");
                        break;
                    }
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

                // Check if this is a zerg rush task
                if self.task_dag.is_zerg_task(&task_id.0) {
                    // Check if already in review (late completion)
                    if self.in_review.contains_key(&task_id.0) {
                        eprintln!(
                            "[Nydus] ZERG RUSH: Ignoring late completion from {} for task {} (winner already chosen)",
                            queen_id.0, task_id.0
                        );
                        return Ok(());
                    }

                    // This Queen is the winner! Mark it and get losers
                    eprintln!(
                        "[Nydus] ZERG RUSH: Queen {} won task {} (${:.2}, {}ms, {} turns)",
                        queen_id.0, task_id.0, cost_usd, duration_ms, num_turns
                    );

                    let losers = self.task_dag.zerg_winner(&task_id.0, queen_id.clone());

                    // Insert into in_review
                    self.in_review.insert(task_id.0.clone(), queen_id.clone());

                    // ABORT loser Queens (kill subprocesses and remove from pool)
                    for loser in &losers {
                        eprintln!("[Nydus] ZERG RUSH: Aborting loser Queen {} for task {}", loser.0, task_id.0);
                        self.abort_queen(loser);
                    }

                    // Maintain minimum pool size: spawn new Queens if below min_queens
                    let current_queens = self.total_queens();
                    if current_queens < self.config.min_queens {
                        let to_spawn = self.config.min_queens.saturating_sub(current_queens);
                        eprintln!(
                            "[Nydus] ELASTIC POOL: Below min_queens ({}), spawning {} Queens",
                            self.config.min_queens,
                            to_spawn
                        );

                        for _ in 0..to_spawn {
                            match self.spawn_queen().await {
                                Ok(new_queen_id) => {
                                    eprintln!("[Nydus] ELASTIC POOL: Spawned replacement Queen {}", new_queen_id.0);
                                }
                                Err(e) => {
                                    eprintln!("[Nydus] ELASTIC POOL: Failed to spawn replacement Queen: {}", e);
                                    break;
                                }
                            }
                        }
                    }

                    // Continue with normal completion flow for winner (Overlord review, etc.)
                }

                // Mark as validating (awaiting Overlord review), NOT completed yet
                self.task_dag.set_validating(&task_id.0);

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

                // Accumulate Queen cost
                self.total_queen_cost_usd += cost_usd;
                self.update_cost_tracking();

                // Auto-push TaskCompleted result to outbox for Operator
                let completion_msg = SwarmMessage {
                    id: uuid::Uuid::new_v4().to_string(),
                    from: AgentId::Queen(queen_id.clone()),
                    to: AgentId::Operator,
                    msg_type: MessageType::TaskResult,
                    payload: serde_json::json!({
                        "task_id": task_id.0,
                        "queen_id": queen_id.0,
                        "status": "completed",
                        "result": result_text,
                        "cost_usd": cost_usd,
                        "duration_ms": duration_ms,
                        "num_turns": num_turns,
                        "quality_passed": quality_passed,
                    }),
                    timestamp: Utc::now(),
                    correlation_id: None,
                    visibility: Visibility::default_internal(),
                };
                self.mailbox.lock().send(completion_msg);

                // Trigger Overlord review if available and git isolation is enabled
                if self.config.git_isolation && self.overlord.is_some() {
                    let mut review_sent = false;

                    // Get worktree info for review
                    if let Some(ref worktree_mgr) = self.worktree_mgr {
                        // DEBUG: Log all known worktrees to diagnose missing paths
                        let known_queens = worktree_mgr.known_queen_ids();
                        eprintln!(
                            "[Nydus] DEBUG: Looking up worktree for Queen {}. Known worktrees: {:?}",
                            queen_id.0, known_queens
                        );
                        if let Some(worktree_path) = worktree_mgr.get_worktree_path(&queen_id) {
                            eprintln!(
                                "[Nydus] Sending task {} from {} to Overlord for review",
                                task_id.0, queen_id.0
                            );

                            // Look up task description and verify_cmd from DAG
                            let (task_description, verify_cmd) = if let Some(dag_task) = self.task_dag.get(&task_id.0) {
                                (dag_task.description.clone(), dag_task.verify_cmd.clone())
                            } else {
                                (format!("Task {}", task_id.0), None)
                            };

                            // HYBRID REVIEW PIPELINE: Run deterministic checks first
                            eprintln!("[Nydus] Running hybrid review pipeline for task {} from {}", task_id.0, queen_id.0);

                            let duration_secs = duration_ms as f64 / 1000.0;
                            match crate::overlord::verdict::run_hybrid_review(
                                &worktree_path,
                                "main",
                                verify_cmd.as_deref(),
                                &task_description,
                                duration_secs,
                                cost_usd,
                                num_turns as usize,
                            ).await {
                                Ok(hybrid_result) => {
                                    match hybrid_result.verdict {
                                        crate::overlord::verdict::OverlordVerdict::Approve => {
                                            // Auto-approve without LLM
                                            eprintln!("[Nydus] Hybrid review APPROVED task {} (deterministic checks passed)", task_id.0);
                                            self.handle_overlord_approve(&queen_id, &task_id.0, "Auto-approved by hybrid review (all checks passed)").await?;
                                            review_sent = true;
                                        }
                                        crate::overlord::verdict::OverlordVerdict::Reject { reason } => {
                                            // Auto-reject without LLM
                                            eprintln!("[Nydus] Hybrid review REJECTED task {}: {}", task_id.0, reason);
                                            self.handle_overlord_reject(&queen_id, &task_id.0, &reason).await?;
                                            review_sent = true;
                                        }
                                    }
                                }
                                Err(e) => {
                                    eprintln!("[Nydus] Hybrid review pipeline failed: {}. Rejecting task.", e);
                                    let reason = format!("Hybrid review pipeline error: {}", e);
                                    self.handle_overlord_reject(&queen_id, &task_id.0, &reason).await?;
                                    review_sent = true;
                                }
                            }
                    } else {
                        eprintln!("[Nydus] ERROR: No worktree path found for Queen {} — cannot send task {} to Overlord!", queen_id.0, task_id.0);
                    }
                } else {
                    eprintln!("[Nydus] ERROR: WorktreeManager is None — cannot send task {} to Overlord!", task_id.0);
                }

                    // CRITICAL: If review was NOT sent, task is stuck in Validating forever.
                    // Fail it loudly so operator sees it and dependencies don't silently stall.
                    if !review_sent {
                        eprintln!("[Nydus] CRITICAL: Hybrid review NOT completed for task {}! Failing task to prevent silent stall.", task_id.0);
                        self.task_dag.fail(&task_id.0, "FAILED: Hybrid review pipeline did not complete (no Accept/Reject verdict)".to_string());

                        // Send escalation to operator
                        let escalation_msg = SwarmMessage {
                            id: uuid::Uuid::new_v4().to_string(),
                            from: AgentId::Nydus(crate::core::types::NydusId("SH0".to_string())),
                            to: AgentId::Operator,
                            msg_type: MessageType::Escalation,
                            payload: serde_json::json!({
                                "type": "hybrid_review_incomplete",
                                "task_id": task_id.0,
                                "queen_id": queen_id.0,
                                "reason": "Hybrid review pipeline did not complete (no Accept/Reject verdict). Task failed."
                            }),
                            timestamp: Utc::now(),
                            correlation_id: None,
                            visibility: Visibility::default_internal(),
                        };
                        self.mailbox.lock().send(escalation_msg);
                        self.try_schedule().await?;
                    }
                } else if self.config.git_isolation {
                    // No Overlord, fall back to old validation + merge flow
                    if let Ok(merge_result) = self.validated_merge(&queen_id).await {
                        match merge_result {
                            ValidatedMergeResult::Merged { commit_sha } => {
                                println!("[Nydus] Validated and merged for {}: {}", queen_id.0, commit_sha);
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
                }

                // Check queen inbox for pending operator messages BEFORE scheduling next DAG task
                self.deliver_pending_messages(&queen_id).await?;

                // Recovery: Check if this Queen has any other tasks stuck in Assigned state
                // This can happen if Queen was assigned multiple tasks but only started one
                let recovered = self.task_dag.recover_stuck_tasks(&[queen_id.clone()]);
                if recovered > 0 {
                    eprintln!("[Nydus] Recovered {} stuck tasks assigned to {} after completion", recovered, queen_id.0);
                }

                // Update PRD file checkbox
                if let Some(ref prd_path) = self.config.prd_path {
                    if let Err(e) = crate::prd::mark_task_done(prd_path, &task_id.0) {
                        eprintln!("[Nydus] Warning: Failed to update PRD checkbox for {}: {}", task_id.0, e);
                    }
                }

                // Work stealing: Try to schedule next task to THIS queen first (warm worktree)
                // Then schedule to other idle queens
                self.try_schedule_with_preference(Some(&queen_id)).await?;

                println!(
                    "[Nydus] Task {} completed by {} (${:.2}, {}ms, {} turns)",
                    task_id.0, queen_id.0, cost_usd, duration_ms, num_turns
                );
            }

            QueenEvent::TaskFailed {
                queen_id, task_id, error, cost_usd, num_turns,
            } => {
                self.tick_state.note_failure();

                // Rate limit cascade detection
                if error.contains("rate_limit") {
                    let now = std::time::Instant::now();
                    self.rate_limit_window.push(now);
                    // Keep only events from last 60 seconds
                    self.rate_limit_window.retain(|t| now.duration_since(*t) < std::time::Duration::from_secs(60));

                    let count = self.rate_limit_window.len();
                    eprintln!("[Nydus] RATE LIMIT: Queen {} hit rate limit ({} hits in last 60s)", queen_id.0, count);

                    // If 2+ rate limit failures within 60s, it's a global rate limit — shut down
                    if count >= 2 {
                        eprintln!("[Nydus] RATE LIMIT CASCADE: {} Queens hit rate limit within 60s. Global rate limit detected.", count);
                        eprintln!("[Nydus] Initiating graceful shutdown — retrying is pointless.");

                        // Mark task as failed but with clear rate limit reason
                        self.task_dag.fail(&task_id.0, format!("RATE_LIMIT: Global API rate limit hit. Swarm shutting down. Task can be resumed later."));

                        // Send escalation
                        let msg = SwarmMessage {
                            id: uuid::Uuid::new_v4().to_string(),
                            from: AgentId::Nydus(self.id.clone()),
                            to: AgentId::Operator,
                            msg_type: MessageType::Escalation,
                            payload: serde_json::json!({
                                "type": "rate_limit_shutdown",
                                "rate_limit_hits": count,
                                "message": "Global API rate limit detected. All Queens affected. Swarm shutting down gracefully. Resume when rate limit resets."
                            }),
                            timestamp: chrono::Utc::now(),
                            correlation_id: None,
                            visibility: Visibility::default_internal(),
                        };
                        self.mailbox.lock().send(msg);

                        // Trigger shutdown via the watch channel
                        let _ = self.shutdown_tx.send(true);
                        return Ok(());
                    }
                }

                // Mark failed in DAG
                self.task_dag.fail(&task_id.0, error.clone());

                eprintln!(
                    "[Nydus] Task {} failed for {} (${:.2}, {} turns): {}",
                    task_id.0, queen_id.0, cost_usd, num_turns, error
                );

                // Auto-push TaskFailed result to outbox for Operator
                let failure_msg = SwarmMessage {
                    id: uuid::Uuid::new_v4().to_string(),
                    from: AgentId::Queen(queen_id.clone()),
                    to: AgentId::Operator,
                    msg_type: MessageType::TaskResult,
                    payload: serde_json::json!({
                        "task_id": task_id.0,
                        "queen_id": queen_id.0,
                        "status": "failed",
                        "error": error,
                        "cost_usd": cost_usd,
                        "num_turns": num_turns,
                    }),
                    timestamp: Utc::now(),
                    correlation_id: None,
                    visibility: Visibility::default_internal(),
                };
                self.mailbox.lock().send(failure_msg);

                // Check queen inbox for pending operator messages BEFORE scheduling next DAG task
                self.deliver_pending_messages(&queen_id).await?;

                // Try to schedule next tasks for all idle queens
                self.try_schedule().await?;
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
                    "[Nydus] Process died for {} (exit: {:?}, session: {:?})",
                    queen_id.0, exit_code, session_id
                );

                // Recovery: check if we should restart
                if let Some(plan) = self.recovery_manager.check_health(
                    &queen_id, false, &self.session_tracker
                ) {
                    eprintln!(
                        "[Nydus] Recovery plan for {}: {:?} (attempt #{})",
                        plan.queen_id.0, plan.reason, plan.attempt
                    );
                    self.recovery_manager.mark_recovery_attempted(&plan.queen_id);
                }
            }

            QueenEvent::StatusChanged { queen_id: _, status } => {
                self.tick_state.note_event();
                // When a Queen becomes Idle, try to schedule ready tasks to all idle queens
                // This fixes the race condition where TaskCompleted arrives before StatusChanged
                if matches!(status, QueenStatus::Idle) {
                    self.try_schedule().await?;
                }
            }

            QueenEvent::ContextCompressed { queen_id, pre_tokens, trigger } => {
                self.tick_state.note_event();
                eprintln!(
                    "[Nydus] Context compressed for {} (pre_tokens: {}, trigger: {})",
                    queen_id.0, pre_tokens, trigger
                );
            }

            QueenEvent::MessagesReceived { queen_id, count } => {
                self.tick_state.note_event();
                eprintln!(
                    "[Nydus] {} received {} queued messages after task completion",
                    queen_id.0, count
                );
            }
        }
        // Update queen status snapshots and DAG stats after every event
        self.update_queen_snapshots();
        self.update_dag_stats();

        Ok(())
    }

    /// Handle events from the Overlord (merge validator).
    /// The Overlord is a StreamQueen, so events are QueenEvents.
    async fn handle_overlord_queen_event(&mut self, event: QueenEvent) -> Result<()> {
        match event {
            QueenEvent::TaskCompleted { task_id, result_text, cost_usd, .. } => {
                // Parse task_id to extract queen_id and original task_id
                // task_id format: "review-Q0-prd-1"
                let (queen_id, original_task_id) = parse_review_task_id(&task_id.0);

                // Accumulate Overlord cost
                self.total_overlord_cost_usd += cost_usd;
                self.overlord_reviews_completed += 1;
                self.update_cost_tracking();

                // Parse verdict from result_text
                if result_text.contains("VERDICT: APPROVE") {
                    let summary = extract_tag(&result_text, "summary")
                        .unwrap_or_else(|| "Approved".to_string());
                    eprintln!(
                        "[Nydus] Overlord APPROVED merge for {} task {} (${:.2}): {}",
                        queen_id.0, original_task_id, cost_usd, summary
                    );
                    self.handle_overlord_approve(&queen_id, &original_task_id, &summary).await?;
                } else if result_text.contains("VERDICT: REJECT") {
                    let reason = extract_tag(&result_text, "reason")
                        .unwrap_or_else(|| "Rejected without details".to_string());
                    eprintln!(
                        "[Nydus] Overlord REJECTED merge for {} task {} (${:.2}): {}",
                        queen_id.0, original_task_id, cost_usd, reason
                    );
                    self.handle_overlord_reject(&queen_id, &original_task_id, &reason).await?;
                } else {
                    // No clear verdict — REJECT (do not auto-approve)
                    eprintln!("[Nydus] Overlord did not provide clear verdict (${:.2}), rejecting", cost_usd);
                    self.handle_overlord_reject(&queen_id, &original_task_id, "No clear verdict from Overlord review").await?;
                }
            }
            QueenEvent::TaskFailed { task_id, error, .. } => {
                // Review failed — REJECT or escalate (do NOT auto-approve failed reviews)
                let (queen_id, original_task_id) = parse_review_task_id(&task_id.0);
                eprintln!("[Nydus] Overlord review process failed, rejecting task: {}", error);
                self.handle_overlord_reject(&queen_id, &original_task_id, &format!("Review process failed: {}", error)).await?;
            }
            QueenEvent::ProcessDied { exit_code, .. } => {
                eprintln!("[Nydus] Overlord process died: exit_code={:?}", exit_code);

                // Send process death notification
                let death_msg = SwarmMessage {
                    id: uuid::Uuid::new_v4().to_string(),
                    from: AgentId::Nydus(self.id.clone()),
                    to: AgentId::Operator,
                    msg_type: MessageType::Escalation,
                    payload: serde_json::json!({
                        "agent": "overlord",
                        "status": "process_died",
                        "exit_code": exit_code,
                    }),
                    timestamp: Utc::now(),
                    correlation_id: None,
                    visibility: Visibility::default_internal(),
                };
                self.mailbox.lock().send(death_msg);
            }
            _ => {
                // Ignore other events (Progress, Knowledge, StatusChanged, etc.)
            }
        }

        Ok(())
    }

    /// Handle Overlord approval — merge the Queen's worktree.
    async fn handle_overlord_approve(
        &mut self,
        queen_id: &QueenId,
        task_id: &str,
        summary: &str,
    ) -> Result<()> {
        // Remove from in_review
        self.in_review.remove(task_id);

        // Proceed with merge
        if let Some(ref mut worktree_mgr) = self.worktree_mgr {
            match worktree_mgr.merge(queen_id) {
                Ok(MergeResult::Success { commit_sha }) => {
                    eprintln!("[Nydus] Successfully merged {} to main: {}", queen_id.0, commit_sha);

                    // Post-merge verification: run cargo check on the main repository
                    eprintln!("[Nydus] Running post-merge verification for task {}", task_id);
                    let check_output = tokio::process::Command::new("cargo")
                        .arg("check")
                        .arg("--workspace")
                        .current_dir(&self.config.working_dir)
                        .output()
                        .await;

                    match check_output {
                        Ok(output) if !output.status.success() => {
                            // Verification failed — revert merge and requeue task
                            let stderr = String::from_utf8_lossy(&output.stderr);
                            eprintln!(
                                "[Nydus] Post-merge verification failed for task {}, reverting merge",
                                task_id
                            );

                            // Revert the merge
                            if let Err(e) = worktree_mgr.revert_last_merge() {
                                eprintln!("[Nydus] Failed to revert merge for {}: {}", task_id, e);
                            } else {
                                eprintln!("[Nydus] Reverted merge for task {}", task_id);
                            }

                            // Requeue the task with feedback
                            let feedback = format!("Post-merge cargo check failed:\n{}", stderr);
                            let requeued = self.task_dag.requeue_with_feedback(task_id, feedback);
                            if requeued {
                                eprintln!("[Nydus] Task {} requeued with compilation error feedback", task_id);
                                // Try to schedule the requeued task
                                self.try_schedule().await?;
                            } else {
                                eprintln!("[Nydus] Failed to requeue task {}", task_id);
                            }

                            // Return early without syncing worktrees
                            return Ok(());
                        }
                        Ok(_) => {
                            // Verification passed
                            eprintln!("[Nydus] Post-merge verification passed for task {}", task_id);

                            // Now mark task as complete in DAG (after successful merge and verification)
                            let dag_result = crate::core::task_dag::DagTaskResult {
                                success: true,
                                output: format!("Completed and merged to main ({})", commit_sha),
                                files_modified: vec![],
                            };
                            self.task_dag.complete(task_id, dag_result);

                            // Reset decline counts in SwarmPool
                            self.swarm_pool.on_task_completed(task_id);

                            // Update PRD checkbox
                            if let Some(ref prd_path) = self.config.prd_path {
                                if let Err(e) = crate::prd::mark_task_done(prd_path, task_id) {
                                    eprintln!("[Nydus] Warning: Failed to update PRD checkbox for {}: {}", task_id, e);
                                } else {
                                    eprintln!("[Nydus] PRD checkbox marked done for task {}", task_id);
                                }
                            }
                        }
                        Err(e) => {
                            // Failed to run cargo check (command not found, etc.)
                            eprintln!("[Nydus] Warning: Failed to run post-merge verification: {}", e);
                            // Continue with merge (don't block on verification failures)

                            // Still mark complete — don't block on verification tool failures
                            let dag_result = crate::core::task_dag::DagTaskResult {
                                success: true,
                                output: format!("Completed and merged to main (verification skipped: {})", e),
                                files_modified: vec![],
                            };
                            self.task_dag.complete(task_id, dag_result);

                            // Reset decline counts in SwarmPool
                            self.swarm_pool.on_task_completed(task_id);

                            // Update PRD checkbox
                            if let Some(ref prd_path) = self.config.prd_path {
                                if let Err(e) = crate::prd::mark_task_done(prd_path, task_id) {
                                    eprintln!("[Nydus] Warning: Failed to update PRD checkbox for {}: {}", task_id, e);
                                } else {
                                    eprintln!("[Nydus] PRD checkbox marked done for task {}", task_id);
                                }
                            }
                        }
                    }

                    // Send approval message to operator
                    let approval_msg = SwarmMessage {
                        id: uuid::Uuid::new_v4().to_string(),
                        from: AgentId::Overlord(OverlordId("overlord-0".to_string())),
                        to: AgentId::Operator,
                        msg_type: MessageType::Custom("MergeApproved".to_string()),
                        payload: serde_json::json!({
                            "queen_id": queen_id.0,
                            "task_id": task_id,
                            "commit_sha": commit_sha,
                            "summary": summary,
                        }),
                        timestamp: Utc::now(),
                        correlation_id: None,
                        visibility: Visibility::default_internal(),
                    };
                    self.mailbox.lock().send(approval_msg);

                    // Sync all other Queens' worktrees with the latest main
                    let results = worktree_mgr.sync_all_with_base(Some(queen_id));
                    for (qid, result) in &results {
                        match result {
                            SyncResult::Synced => eprintln!("[Nydus] Synced {} worktree with main", qid.0),
                            SyncResult::Skipped(reason) => eprintln!("[Nydus] Skipped sync for {}: {}", qid.0, reason),
                            SyncResult::ConflictAborted => eprintln!("[Nydus] Sync conflict in {}, aborted", qid.0),
                            SyncResult::Error(e) => eprintln!("[Nydus] Sync error for {}: {}", qid.0, e),
                        }
                    }

                    // Recreate merged Queen's worktree from fresh main
                    // The old branch was merged, so we need a new clean branch from main
                    if let Err(e) = worktree_mgr.cleanup(queen_id) {
                        eprintln!("[Nydus] Warning: Failed to cleanup old worktree for {}: {}", queen_id.0, e);
                    }
                    match worktree_mgr.create(queen_id) {
                        Ok(new_path) => {
                            eprintln!("[Nydus] Recreated worktree for {} from fresh main: {}", queen_id.0, new_path.display());
                        }
                        Err(e) => {
                            eprintln!("[Nydus] ERROR: Failed to recreate worktree for {}: {}", queen_id.0, e);
                        }
                    }

                    // Schedule newly-unblocked tasks
                    self.try_schedule().await?;
                }
                Ok(MergeResult::Conflict { files }) => {
                    let file_list: Vec<String> = files.iter().map(|p| p.to_string_lossy().to_string()).collect();
                    eprintln!(
                        "[Nydus] Merge conflict for {} despite approval: {:?}",
                        queen_id.0, file_list
                    );

                    // Requeue task with conflict feedback so Queen can retry with context
                    let feedback = format!(
                        "Merge conflict detected in {} files after Overlord approval.\n\
                         Conflicted files: {:?}\n\
                         The Queen's worktree was reset (merge --abort). \
                         Please resolve conflicts with the latest main branch before re-submitting.",
                        files.len(), file_list
                    );
                    let requeued = self.task_dag.requeue_with_feedback(task_id, feedback);
                    if requeued {
                        eprintln!("[Nydus] Task {} requeued after merge conflict", task_id);
                        self.try_schedule().await?;
                    } else {
                        eprintln!("[Nydus] Failed to requeue task {} after merge conflict", task_id);
                    }

                    // Also send conflict notification to operator
                    let conflict_msg = SwarmMessage {
                        id: uuid::Uuid::new_v4().to_string(),
                        from: AgentId::Nydus(self.id.clone()),
                        to: AgentId::Operator,
                        msg_type: MessageType::Escalation,
                        payload: serde_json::json!({
                            "queen_id": queen_id.0,
                            "task_id": task_id,
                            "reason": "merge_conflict_after_approval",
                            "files": file_list,
                        }),
                        timestamp: Utc::now(),
                        correlation_id: None,
                        visibility: Visibility::default_internal(),
                    };
                    self.mailbox.lock().send(conflict_msg);
                }
                Ok(MergeResult::NoChanges) => {
                    eprintln!("[Nydus] No changes to merge for {} (task {}), marking complete", queen_id.0, task_id);
                    // Still mark as complete — the work was done, just nothing to merge
                    let dag_result = crate::core::task_dag::DagTaskResult {
                        success: true,
                        output: "Completed with no merge changes (approved by Overlord)".to_string(),
                        files_modified: vec![],
                    };
                    self.task_dag.complete(task_id, dag_result);

                    // Update PRD checkbox
                    if let Some(ref prd_path) = self.config.prd_path {
                        if let Err(e) = crate::prd::mark_task_done(prd_path, task_id) {
                            eprintln!("[Nydus] Warning: Failed to update PRD checkbox for {}: {}", task_id, e);
                        } else {
                            eprintln!("[Nydus] PRD checkbox marked done for task {}", task_id);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("[Nydus] Merge failed for {}: {}", queen_id.0, e);
                    let feedback = format!("Merge operation failed: {}", e);
                    let requeued = self.task_dag.requeue_with_feedback(task_id, feedback);
                    if requeued {
                        eprintln!("[Nydus] Task {} requeued after merge failure", task_id);
                        self.try_schedule().await?;
                    }
                }
            }
        }

        // Run post-merge verification scan to detect tasks completed by the same commit
        // This must be AFTER the worktree_mgr scope ends to avoid mutable borrow conflicts
        if let Err(e) = self.post_merge_verify_scan().await {
            eprintln!("[Nydus] POST-MERGE SCAN: Error during scan: {}", e);
        }

        Ok(())
    }

    /// Handle Overlord rejection — requeue with feedback or escalate.
    async fn handle_overlord_reject(
        &mut self,
        queen_id: &QueenId,
        task_id: &str,
        reason: &str,
    ) -> Result<()> {
        // Remove from in_review
        self.in_review.remove(task_id);

        // Ask SwarmPool what to do with this decline
        let action = self.swarm_pool.on_decline(task_id, reason);

        match action {
            SwarmPoolAction::RetryTask { task_id } => {
                // Requeue with feedback
                let requeued = self.task_dag.requeue_with_feedback(&task_id, reason.to_string());
                if requeued {
                    let retry_count = self.task_dag.get(&task_id)
                        .map(|t| t.retry_count)
                        .unwrap_or(0);
                    eprintln!(
                        "[Nydus] Task {} rejected (attempt {}), requeued with feedback: {}",
                        task_id, retry_count, reason
                    );
                    self.try_schedule().await?;
                } else {
                    eprintln!("[Nydus] Failed to requeue task {}, escalating", task_id);
                    // Fallback escalation
                    let rejection_msg = SwarmMessage {
                        id: uuid::Uuid::new_v4().to_string(),
                        from: AgentId::Overlord(OverlordId("overlord-0".to_string())),
                        to: AgentId::Operator,
                        msg_type: MessageType::Escalation,
                        payload: serde_json::json!({
                            "queen_id": queen_id.0,
                            "task_id": task_id,
                            "status": "rejected_requeue_failed",
                            "reason": reason,
                        }),
                        timestamp: Utc::now(),
                        correlation_id: None,
                        visibility: Visibility::default_internal(),
                    };
                    self.mailbox.lock().send(rejection_msg);
                }
            }
            SwarmPoolAction::EscalateToOvermind { task_id, decline_count, reasons } => {
                eprintln!(
                    "[Nydus] Task {} escalated after {} declines",
                    task_id, decline_count
                );
                // Send escalation message
                let rejection_msg = SwarmMessage {
                    id: uuid::Uuid::new_v4().to_string(),
                    from: AgentId::Overlord(OverlordId("overlord-0".to_string())),
                    to: AgentId::Operator,
                    msg_type: MessageType::Escalation,
                    payload: serde_json::json!({
                        "queen_id": queen_id.0,
                        "task_id": task_id,
                        "status": "escalated_to_overmind",
                        "decline_count": decline_count,
                        "reasons": reasons,
                    }),
                    timestamp: Utc::now(),
                    correlation_id: None,
                    visibility: Visibility::default_internal(),
                };
                self.mailbox.lock().send(rejection_msg);

                // For now, fall back to requeue (Overmind wiring in Phase 5)
                if let Some(last_reason) = reasons.last() {
                    self.task_dag.requeue_with_feedback(&task_id, last_reason.clone());
                    self.try_schedule().await?;
                }
            }
            _ => {
                // Other actions not expected in this context
            }
        }

        Ok(())
    }

    /// Post-merge verification scan: check if any other tasks are now satisfiable.
    ///
    /// This handles the case where a Queen implements multiple tasks in one commit
    /// but only the assigned task gets marked as completed. After a successful merge,
    /// we scan all pending/blocked tasks and run their verify commands to see if
    /// any were accidentally completed by the same commit.
    async fn post_merge_verify_scan(&mut self) -> Result<()> {
        // Get all tasks that are NOT completed or failed
        let pending_tasks: Vec<(String, Option<String>)> = self.task_dag
            .all_tasks()
            .iter()
            .filter(|t| !matches!(t.status, DagTaskStatus::Completed | DagTaskStatus::Failed { .. }))
            .map(|t| (t.id.clone(), t.verify_cmd.clone()))
            .collect();

        if pending_tasks.is_empty() {
            return Ok(());
        }

        eprintln!(
            "[Nydus] POST-MERGE SCAN: Checking {} pending tasks for accidental completion...",
            pending_tasks.len()
        );

        let mut newly_completed = 0;

        for (task_id, verify_cmd) in pending_tasks {
            if let Some(ref cmd) = verify_cmd {
                // Run verify command in the main working directory
                let output = tokio::process::Command::new("bash")
                    .arg("-c")
                    .arg(cmd)
                    .current_dir(&self.config.working_dir)
                    .output()
                    .await;

                match output {
                    Ok(result) if result.status.success() => {
                        eprintln!(
                            "[Nydus] POST-MERGE SCAN: Task {} verify command passed! Marking as completed.",
                            task_id
                        );

                        // Mark task as completed in DAG
                        let dag_result = crate::core::task_dag::DagTaskResult {
                            success: true,
                            output: format!("Completed via post-merge verification scan (verify: {})", cmd),
                            files_modified: vec![],
                        };
                        self.task_dag.complete(&task_id, dag_result);

                        // Update PRD checkbox
                        if let Some(ref prd_path) = self.config.prd_path {
                            if let Err(e) = crate::prd::mark_task_done(prd_path, &task_id) {
                                eprintln!(
                                    "[Nydus] POST-MERGE SCAN: Warning: Failed to update PRD checkbox for {}: {}",
                                    task_id, e
                                );
                            }
                        }

                        newly_completed += 1;
                    }
                    Ok(_) => {
                        // Verify command failed — task not done yet, that's fine
                    }
                    Err(e) => {
                        eprintln!(
                            "[Nydus] POST-MERGE SCAN: Error running verify for {}: {}",
                            task_id, e
                        );
                    }
                }
            }
        }

        if newly_completed > 0 {
            eprintln!(
                "[Nydus] POST-MERGE SCAN: Found {} tasks that were completed by the same commit",
                newly_completed
            );

            // Re-schedule after scan (new tasks may have become unblocked)
            self.try_schedule().await?;
        } else {
            eprintln!("[Nydus] POST-MERGE SCAN: No additional completed tasks found");
        }

        Ok(())
    }

    /// Handle a runtime task injection request from IPC.
    async fn handle_inject(&mut self, inject_req: ipc::protocol::InjectRequest) {
        use crate::core::task_dag::{DagTask, DagTaskStatus, Priority, Complexity};

        let task_id = inject_req.task_id.clone();
        let queen_id_opt = inject_req.queen_id.clone();

        // Try to assign directly to the specified queen if it's idle
        if let Some(ref queen_id) = queen_id_opt {
            if let Some(handle) = self.handles.get(queen_id) {
                if matches!(handle.status(), QueenStatus::Idle) {
                    // Build context from SharedMemory
                    let knowledge = self.memory.query("*")
                        .into_iter()
                        .map(|entry| (entry.key, entry.value))
                        .collect();

                    let all_entries = self.memory.query("");
                    let knowledge_entries: Vec<String> = all_entries
                        .iter()
                        .rev()
                        .take(10)
                        .map(|e| {
                            let author_str = match &e.author {
                                AgentId::Queen(qid) => qid.0.clone(),
                                AgentId::Nydus(sid) => sid.0.clone(),
                                AgentId::Validator => "Validator".to_string(),
                                AgentId::Operator => "Operator".to_string(),
                                AgentId::Overlord(iid) => iid.0.clone(),
                                AgentId::Overmind(oid) => oid.0.clone(),
                            };
                            format!("[{}] {}: {}", author_str, e.key, e.value)
                        })
                        .collect();

                    let other_tasks_summary = self.build_other_tasks_summary(&task_id);

                    let context = TaskContext {
                        knowledge,
                        recent_messages: Vec::new(),
                        shared_state: HashMap::new(),
                        skill_hint: None,
                        knowledge_entries,
                        other_tasks_summary: Some(other_tasks_summary),
                        rejection_feedback: None,
                    };

                    let task = Task {
                        id: TaskId(task_id.clone()),
                        description: inject_req.prompt.clone(),
                        status: TaskStatus::Assigned,
                        assigned_to: Some(queen_id.clone()),
                        priority: inject_req.priority,
                        blocked_by: vec![],
                        created_at: Utc::now(),
                    };

                    match handle.assign(task, context).await {
                        Ok(_) => {
                            eprintln!("[Nydus] Injected task {} assigned to {}", task_id, queen_id.0);
                            let response = ipc::protocol::InjectResponse {
                                task_id: task_id.clone(),
                                assigned_to: Some(queen_id.0.clone()),
                                status: "assigned".to_string(),
                            };
                            let _ = inject_req.response_tx.send(response);
                            return;
                        }
                        Err(e) => {
                            eprintln!("[Nydus] Failed to assign injected task {} to {}: {}", task_id, queen_id.0, e);
                        }
                    }
                }
            }
        }

        // If direct assignment failed or no queen specified, add to DAG
        // Convert u8 priority to Priority enum
        let priority = match inject_req.priority {
            0..=63 => Priority::Low,
            64..=127 => Priority::Normal,
            128..=191 => Priority::High,
            192..=255 => Priority::Critical,
        };

        let dag_task = DagTask {
            id: task_id.clone(),
            description: inject_req.prompt.clone(),
            status: DagTaskStatus::Ready,
            assigned_to: None,
            blocked_by: vec![],
            blocks: vec![],
            priority,
            estimated_complexity: Complexity::Medium,
            result: None,
            created_at: Utc::now(),
            started_at: None,
            completed_at: None,
            skill_hint: None,
            retry_count: 0,
            rejection_feedback: Vec::new(),
            verify_cmd: None,
        };

        self.task_dag.add_task(dag_task);
        self.update_dag_stats();
        eprintln!("[Nydus] Injected task {} added to DAG, will be scheduled to next idle queen", task_id);

        // Try to schedule immediately
        let _ = self.try_schedule().await;

        let response = ipc::protocol::InjectResponse {
            task_id: task_id.clone(),
            assigned_to: None,
            status: "queued".to_string(),
        };
        let _ = inject_req.response_tx.send(response);
    }

    /// Helper: Get total number of Queens currently in the pool.
    fn total_queens(&self) -> usize {
        self.handles.len()
    }

    /// Dynamically spawn a new Queen and add it to the pool.
    /// Returns the new Queen's ID.
    async fn spawn_queen(&mut self) -> Result<QueenId> {
        let queen_id = QueenId(format!("Q{}", self.next_queen_id));
        self.next_queen_id += 1;

        eprintln!("[Nydus] ELASTIC POOL: Spawning dynamic Queen {}", queen_id.0);

        self.register_queen_actor(
            queen_id.clone(),
            self.default_model.clone(),
            self.default_completion_config.clone(),
        )?;

        Ok(queen_id)
    }

    /// Abort a Queen subprocess and remove it from the pool.
    /// This is used to kill loser Queens in Zerg Rush scenarios.
    fn abort_queen(&mut self, queen_id: &QueenId) {
        eprintln!("[Nydus] ELASTIC POOL: Aborting Queen {}", queen_id.0);

        // Abort the actor task (kills subprocess)
        if let Some(join_handle) = self.actor_tasks.remove(queen_id) {
            join_handle.abort();
        }

        // Remove handle
        self.handles.remove(queen_id);

        // Remove from mailbox
        self.mailbox.lock().unregister_queen(queen_id);

        // NOTE: We intentionally do NOT cleanup worktrees for aborted Queens.
        // Keeping their worktrees + branches allows post-mortem analysis of
        // solution quality. The `git worktree prune` fallback in cleanup()
        // is a GLOBAL operation that can corrupt state for other Queens.
        // Worktrees are cleaned up at session end or manually.
        eprintln!("[Nydus] ELASTIC POOL: Aborted Queen {} — worktree preserved for forensics", queen_id.0);

        eprintln!("[Nydus] ELASTIC POOL: Queen {} removed from pool (current size: {})", queen_id.0, self.total_queens());
    }

    /// Execute actions requested by SwarmPool heuristics.
    /// Only handles SpawnQueens and KillQueen actions.
    /// RetryTask, EscalateToOvermind, and ZergRush must be handled by the caller.
    async fn execute_swarm_pool_actions_sync(&mut self, actions: &[SwarmPoolAction]) -> Result<()> {
        for action in actions {
            match action {
                SwarmPoolAction::SpawnQueens(n) => {
                    eprintln!("[Nydus] SWARM POOL: Spawning {} Queens", n);
                    for _ in 0..*n {
                        if let Err(e) = self.spawn_queen().await {
                            eprintln!("[Nydus] SWARM POOL: Failed to spawn Queen: {}", e);
                            break;
                        }
                    }
                }
                SwarmPoolAction::KillQueen(id) => {
                    eprintln!("[Nydus] SWARM POOL: Kill Queen {} requested (not yet implemented)", id.0);
                    // TODO: Implement graceful Queen shutdown
                }
                _ => {
                    // Skip non-sync actions (ZergRush, RetryTask, EscalateToOvermind handled by caller)
                }
            }
        }
        Ok(())
    }

    /// Find ready tasks + idle queens, assign tasks.
    ///
    /// If `preferred_queen` is provided and is idle, it will be scheduled first (work stealing).
    /// This keeps the completing Queen busy and reduces context switch overhead.
    async fn try_schedule_with_preference(&mut self, preferred_queen: Option<&QueenId>) -> Result<usize> {
        let ready_tasks = self.task_dag.ready_tasks();
        if ready_tasks.is_empty() {
            return Ok(0);
        }

        // Find idle queens via handle.status()
        let mut idle_queens: Vec<QueenId> = self.handles.iter()
            .filter(|(_, handle)| matches!(handle.status(), QueenStatus::Idle))
            .map(|(id, _)| id.clone())
            .collect();

        if idle_queens.is_empty() {
            return Ok(0);
        }

        // Work stealing optimization: put preferred queen first if it's idle
        if let Some(preferred) = preferred_queen {
            if let Some(pos) = idle_queens.iter().position(|q| q == preferred) {
                idle_queens.swap(0, pos);
            }
        }

        // Build context from SharedMemory
        let knowledge: HashMap<String, serde_json::Value> = self.memory.query("*")
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
                    AgentId::Nydus(sid) => sid.0.clone(),
                    AgentId::Validator => "Validator".to_string(),
                    AgentId::Operator => "Operator".to_string(),
                    AgentId::Overlord(iid) => iid.0.clone(),
                    AgentId::Overmind(oid) => oid.0.clone(),
                };
                format!("[{}] {}: {}", author_str, e.key, e.value)
            })
            .collect();

        let mut assigned = 0;

        // PHASE 1: Ask SwarmPool for spawn decisions (zerg rush + elastic pool)
        let ready_count = ready_tasks.len();
        let active_count = self.handles.values().filter(|h| !matches!(h.status(), QueenStatus::Idle)).count();
        let idle_count = idle_queens.len();

        // Get bottleneck tasks for SwarmPool
        let bottlenecks = self.task_dag.bottleneck_tasks(self.swarm_pool.config().zerg_rush_threshold);
        let bottleneck_pairs: Vec<(String, usize)> = bottlenecks.iter()
            .map(|task| (task.id.clone(), task.blocks.len()))
            .collect();

        let actions = self.swarm_pool.on_dag_change(ready_count, active_count, idle_count, bottleneck_pairs);

        // Execute sync actions first (SpawnQueens only)
        self.execute_swarm_pool_actions_sync(&actions).await?;

        // Re-fetch idle queens after spawning
        idle_queens = self.handles.iter()
            .filter(|(_, handle)| matches!(handle.status(), QueenStatus::Idle))
            .map(|(id, _)| id.clone())
            .collect();

        // Handle ZergRush actions inline (need access to idle queens and assignment logic)
        for action in actions {
            if let SwarmPoolAction::ZergRush { task_id, num_queens } = action {
                // Get idle queens for this zerg rush
                let mut queens_for_zerg: Vec<QueenId> = idle_queens.iter()
                    .take(num_queens)
                    .cloned()
                    .collect();

                // Spawn additional queens if needed
                let need_more = num_queens.saturating_sub(queens_for_zerg.len());
                let can_spawn = self.config.max_queens.saturating_sub(self.total_queens());
                let to_spawn = need_more.min(can_spawn);

                for _ in 0..to_spawn {
                    match self.spawn_queen().await {
                        Ok(new_queen) => {
                            queens_for_zerg.push(new_queen.clone());
                            idle_queens.push(new_queen);
                        }
                        Err(e) => {
                            eprintln!("[Nydus] ZERG RUSH: Failed to spawn additional Queen: {}", e);
                            break;
                        }
                    }
                }

                if queens_for_zerg.is_empty() {
                    continue;
                }

                eprintln!(
                    "[Nydus] ZERG RUSH: Assigning task {} to {} Queens: {:?}",
                    task_id,
                    queens_for_zerg.len(),
                    queens_for_zerg.iter().map(|q| &q.0).collect::<Vec<_>>()
                );

                // Mark as zerg rush in DAG
                if !self.task_dag.assign_zerg(&task_id, queens_for_zerg.clone()) {
                    eprintln!("[Nydus] Failed to assign zerg rush for task {}", task_id);
                    return Ok(assigned);
                }

                // Get task description and metadata
                let dag_task = match self.task_dag.get(&task_id) {
                    Some(t) => t,
                    None => {
                        eprintln!("[Nydus] Task {} not found in DAG", task_id);
                        return Ok(assigned);
                    }
                };

                let mut description = dag_task.description.clone();
                let priority = dag_task.priority as u8;
                let blocked_by: Vec<TaskId> = dag_task.blocked_by.iter().map(|id| TaskId(id.clone())).collect();
                let created_at = dag_task.created_at;
                let skill_hint = dag_task.skill_hint.clone();

                // Get rejection feedback if present
                let rejection_feedback = if dag_task.rejection_feedback.is_empty() {
                    None
                } else {
                    Some(dag_task.rejection_feedback.clone())
                };

                // Prepend feedback to description if present
                if let Some(ref feedback) = rejection_feedback {
                    let retry_count = dag_task.retry_count;
                    description = format!(
                        "{}\n\n## PREVIOUS REJECTION (attempt #{}):\n{}\n\nFix the issues above and resubmit.",
                        description,
                        retry_count,
                        feedback.join("\n---\n")
                    );
                }

                // Assign to all Queens in parallel
                for queen_id in &queens_for_zerg {
                    let task = Task {
                        id: TaskId(task_id.clone()),
                        description: description.clone(),
                        status: TaskStatus::Assigned,
                        assigned_to: Some(queen_id.clone()),
                        priority,
                        blocked_by: blocked_by.clone(),
                        created_at,
                    };

                    let other_tasks_summary = self.build_other_tasks_summary(&task_id);

                    let task_context = TaskContext {
                        knowledge: knowledge.clone(),
                        recent_messages: Vec::new(),
                        shared_state: HashMap::new(),
                        skill_hint: skill_hint.clone(),
                        knowledge_entries: knowledge_entries.clone(),
                        other_tasks_summary: Some(other_tasks_summary),
                        rejection_feedback: rejection_feedback.clone(),
                    };

                    if let Some(handle) = self.handles.get(queen_id) {
                        if let Err(e) = handle.assign(task, task_context).await {
                            eprintln!("[Nydus] Failed to assign zerg task {} to {}: {}", task_id, queen_id.0, e);
                        } else {
                            assigned += 1;
                        }
                    }
                }

                // Remove assigned queens from idle pool
                idle_queens.retain(|q| !queens_for_zerg.contains(q));
            }
        }

        // Refresh ready tasks (exclude zerged tasks) and clone task data immediately
        let ready_task_data: Vec<(String, String, u8, Vec<String>, chrono::DateTime<Utc>, Option<String>)> = self.task_dag.ready_tasks()
            .into_iter()
            .filter(|t| !self.task_dag.is_zerg_task(&t.id))
            .map(|t| (
                t.id.clone(),
                t.description.clone(),
                t.priority as u8,
                t.blocked_by.clone(),
                t.created_at,
                t.skill_hint.clone(),
            ))
            .collect();

        if ready_task_data.is_empty() || idle_queens.is_empty() {
            return Ok(assigned);
        }

        // PHASE 2: Standard 1:1 assignment for remaining tasks
        // Spawn additional queens if needed
        if ready_task_data.len() > idle_queens.len() {
            let need_more = ready_task_data.len().saturating_sub(idle_queens.len());
            let can_spawn = self.config.max_queens.saturating_sub(self.total_queens());
            let to_spawn = need_more.min(can_spawn);

            for _ in 0..to_spawn {
                match self.spawn_queen().await {
                    Ok(new_queen) => {
                        idle_queens.push(new_queen);
                    }
                    Err(e) => {
                        eprintln!("[Nydus] ELASTIC POOL: Failed to spawn additional Queen for normal assignment: {}", e);
                        break;
                    }
                }
            }
        }

        // Collect (task_id, description, priority, blocked_by, created_at, skill_hint, queen_id) tuples
        let assignments: Vec<(String, String, u8, Vec<TaskId>, chrono::DateTime<Utc>, Option<String>, QueenId)> = ready_task_data.iter()
            .zip(idle_queens.iter())
            .map(|((task_id, description, priority, blocked_by, created_at, skill_hint), queen_id)| {
                (
                    task_id.clone(),
                    description.clone(),
                    *priority,
                    blocked_by.iter().map(|id| TaskId(id.clone())).collect(),
                    *created_at,
                    skill_hint.clone(),
                    queen_id.clone(),
                )
            })
            .collect();

        // Now assign tasks (no borrow conflict)
        for (task_id, mut description, priority, blocked_by, created_at, skill_hint, queen_id) in assignments {
            // CRITICAL: Mark as assigned in DAG FIRST to prevent double-assignment race
            // If another try_schedule() runs before handle.assign() completes,
            // ready_tasks() will not return this task anymore
            self.task_dag.assign(&task_id, queen_id.clone());

            // Get rejection feedback if present and prepend to description
            let rejection_feedback = self.task_dag.get(&task_id)
                .and_then(|t| {
                    if t.rejection_feedback.is_empty() {
                        None
                    } else {
                        Some(t.rejection_feedback.clone())
                    }
                });

            // Prepend feedback to description if present
            if let Some(ref feedback) = rejection_feedback {
                let retry_count = self.task_dag.get(&task_id).map(|t| t.retry_count).unwrap_or(0);
                description = format!(
                    "{}\n\n## PREVIOUS REJECTION (attempt #{}):\n{}\n\nFix the issues above and resubmit.",
                    description,
                    retry_count,
                    feedback.join("\n---\n")
                );
            }

            let task = Task {
                id: TaskId(task_id.clone()),
                description,
                status: TaskStatus::Assigned,
                assigned_to: Some(queen_id.clone()),
                priority,
                blocked_by,
                created_at,
            };

            // Build context for this specific task
            let other_tasks_summary = self.build_other_tasks_summary(&task_id);

            let task_context = TaskContext {
                knowledge: knowledge.clone(),
                recent_messages: Vec::new(),
                shared_state: HashMap::new(),
                skill_hint,
                knowledge_entries: knowledge_entries.clone(),
                other_tasks_summary: Some(other_tasks_summary),
                rejection_feedback,
            };

            if let Some(handle) = self.handles.get(&queen_id) {
                if let Err(e) = handle.assign(task, task_context).await {
                    eprintln!("[Nydus] Failed to assign task {} to {}: {}", task_id, queen_id.0, e);
                    // Rollback DAG assignment on failure
                    self.task_dag.unassign(&task_id);
                    continue;
                }

                assigned += 1;
            } else {
                // Queen handle missing — rollback DAG assignment
                eprintln!("[Nydus] No handle for queen {}, cannot assign task {}", queen_id.0, task_id);
                self.task_dag.unassign(&task_id);
            }
        }

        Ok(assigned)
    }

    /// Find ready tasks + idle queens, assign tasks (no preference).
    ///
    /// This is a convenience wrapper for try_schedule_with_preference(None).
    async fn try_schedule(&mut self) -> Result<usize> {
        self.try_schedule_with_preference(None).await
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
                        "[Nydus] Recovery needed for {}: {:?}",
                        plan.queen_id.0, plan.reason
                    );
                    self.recovery_manager.mark_recovery_attempted(&plan.queen_id);
                }
            }
        }

        // Evict expired memory entries
        self.memory.evict_expired();

        // Elastic pool maintenance: ensure min/max Queens
        let active_count = self.handles.values().filter(|h| !matches!(h.status(), QueenStatus::Idle)).count();
        let idle_count = self.handles.values().filter(|h| matches!(h.status(), QueenStatus::Idle)).count();
        let actions = self.swarm_pool.maintenance(active_count, idle_count);
        self.execute_swarm_pool_actions_sync(&actions).await?;

        // Deadlock detection: all Queens idle but tasks remain
        let all_idle = self.handles.values().all(|h| matches!(h.status(), QueenStatus::Idle));
        let stats = self.task_dag.stats();
        let has_remaining = stats.total > (stats.completed + stats.failed);
        if all_idle && has_remaining {
            eprintln!(
                "[Nydus] DEADLOCK DETECTED: All {} Queens idle, {} tasks remaining ({} blocked, {} ready, {} in_progress). Attempting recovery...",
                self.handles.len(),
                stats.total - stats.completed - stats.failed,
                stats.blocked,
                stats.ready,
                stats.in_progress
            );

            // Collect idle queen IDs for recovery
            let idle_queen_ids: Vec<QueenId> = self.handles.iter()
                .filter(|(_, h)| matches!(h.status(), QueenStatus::Idle))
                .map(|(id, _)| id.clone())
                .collect();

            // Recover stuck tasks (Assigned/InProgress → Ready)
            let recovered = self.task_dag.recover_stuck_tasks(&idle_queen_ids);
            if recovered > 0 {
                eprintln!("[Nydus] Recovered {} stuck tasks, rescheduling", recovered);
            }

            // Also refresh blocked → ready
            self.task_dag.refresh_readiness();

            // Try scheduling again
            self.try_schedule().await?;
        }

        // Update queen status snapshots and DAG stats
        self.update_queen_snapshots();
        self.update_dag_stats();
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

    /// Handle message delivery notification from IPC.
    /// Called when operator sends a message to a queen.
    async fn handle_message_delivery(&mut self, notification: ipc::protocol::MessageDeliveryNotification) {
        eprintln!("[Nydus] Message delivery notification for {}: {}", notification.queen_id.0, notification.message_id);

        // Check if queen is idle, if so deliver message immediately
        if let Some(handle) = self.handles.get(&notification.queen_id) {
            if matches!(handle.status(), QueenStatus::Idle) {
                eprintln!("[Nydus] Queen {} is idle, attempting message delivery", notification.queen_id.0);
                let _ = self.deliver_pending_messages(&notification.queen_id).await;
            } else {
                eprintln!("[Nydus] Queen {} is busy, message will be delivered after task completion", notification.queen_id.0);
            }
        }
    }

    /// Deliver pending messages from queen's inbox as synthetic tasks.
    /// Returns Ok if messages were delivered or inbox was empty, Err if delivery failed.
    async fn deliver_pending_messages(&mut self, queen_id: &QueenId) -> Result<()> {
        // Check if there are messages in the queen's inbox
        let messages: Vec<SwarmMessage> = {
            let mut mb = self.mailbox.lock();
            let mut msgs = Vec::new();
            // Drain up to 5 messages at a time (configurable)
            for _ in 0..5 {
                if let Some(msg) = mb.recv_queen(queen_id) {
                    msgs.push(msg);
                } else {
                    break;
                }
            }
            msgs
        };

        if messages.is_empty() {
            return Ok(());
        }

        eprintln!("[Nydus] Delivering {} pending message(s) to {}", messages.len(), queen_id.0);

        // Get queen handle
        let handle = match self.handles.get(queen_id) {
            Some(h) => h,
            None => {
                eprintln!("[Nydus] Queen {} not found, cannot deliver messages", queen_id.0);
                return Ok(());
            }
        };

        // Check if queen is still idle
        if !matches!(handle.status(), QueenStatus::Idle) {
            eprintln!("[Nydus] Queen {} is no longer idle, re-queueing messages", queen_id.0);
            // Re-queue messages back to inbox
            let mut mb = self.mailbox.lock();
            for msg in messages.iter().rev() {
                mb.send(msg.clone());
            }
            return Ok(());
        }

        // Create synthetic task from message(s)
        let task_id = format!("operator-msg-{}", uuid::Uuid::new_v4());

        // Combine message payloads
        let message_text = messages.iter()
            .map(|msg| {
                if let Some(text) = msg.payload.get("message").and_then(|m| m.as_str()) {
                    text.to_string()
                } else {
                    format!("{}", msg.payload)
                }
            })
            .collect::<Vec<_>>()
            .join("\n\n");

        // Build context from SharedMemory
        let knowledge: HashMap<String, serde_json::Value> = self.memory.query("*")
            .into_iter()
            .map(|entry| (entry.key, entry.value))
            .collect();

        let all_entries = self.memory.query("");
        let knowledge_entries: Vec<String> = all_entries
            .iter()
            .rev()
            .take(10)
            .map(|e| {
                let author_str = match &e.author {
                    AgentId::Queen(qid) => qid.0.clone(),
                    AgentId::Nydus(sid) => sid.0.clone(),
                    AgentId::Validator => "Validator".to_string(),
                    AgentId::Operator => "Operator".to_string(),
                    AgentId::Overlord(iid) => iid.0.clone(),
                    AgentId::Overmind(oid) => oid.0.clone(),
                };
                format!("[{}] {}: {}", author_str, e.key, e.value)
            })
            .collect();

        let other_tasks_summary = self.build_other_tasks_summary(&task_id);

        let context = TaskContext {
            knowledge,
            recent_messages: Vec::new(),
            shared_state: HashMap::new(),
            skill_hint: None,
            knowledge_entries,
            other_tasks_summary: Some(other_tasks_summary),
            rejection_feedback: None,
        };

        let task = Task {
            id: TaskId(task_id.clone()),
            description: format!("Operator message:\n\n{}", message_text),
            status: TaskStatus::Assigned,
            assigned_to: Some(queen_id.clone()),
            priority: 128, // Normal priority
            blocked_by: vec![],
            created_at: Utc::now(),
        };

        match handle.assign(task, context).await {
            Ok(_) => {
                eprintln!("[Nydus] Delivered {} message(s) to {} as task {}", messages.len(), queen_id.0, task_id);
                Ok(())
            }
            Err(e) => {
                eprintln!("[Nydus] Failed to deliver messages to {}: {}", queen_id.0, e);
                // Re-queue messages back to inbox
                let mut mb = self.mailbox.lock();
                for msg in messages.iter().rev() {
                    mb.send(msg.clone());
                }
                Err(e)
            }
        }
    }

    /// Check if all tasks are completed.
    ///
    /// In keep-alive mode, this always returns false (swarm never exits on its own).
    pub fn is_complete(&self) -> bool {
        if self.config.keep_alive {
            return false;
        }
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
            total_queen_cost_usd: self.total_queen_cost_usd,
            total_overlord_cost_usd: self.total_overlord_cost_usd,
            overlord_reviews_completed: self.overlord_reviews_completed,
        }
    }

    /// Get the Nydus ID.
    pub fn id(&self) -> &NydusId {
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

    /// Drain outbox messages (for Operator).
    pub fn drain_outbox(&mut self) -> Vec<SwarmMessage> {
        self.mailbox.lock().drain_outbox()
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

    // ========================================================================
    // Test-only accessors for integration tests
    // ========================================================================

    /// Get reference to the task DAG.
    ///
    /// **WARNING**: This is a test-only accessor. Do not use in production code.
    /// It exposes internal state for integration testing.
    pub fn task_dag(&self) -> &TaskDag {
        &self.task_dag
    }

    /// Get mutable reference to the task DAG.
    ///
    /// **WARNING**: This is a test-only accessor. Do not use in production code.
    /// It exposes internal state for integration testing.
    pub fn task_dag_mut(&mut self) -> &mut TaskDag {
        &mut self.task_dag
    }

    /// Register a Queen handle (simplified version for testing).
    ///
    /// **WARNING**: This is a test-only method. Use `register_queen_actor` in production.
    /// This method bypasses the normal Queen spawning process and directly registers
    /// a handle, which is useful for injecting mock Queens in tests.
    pub fn register_queen(&mut self, queen_id: QueenId, handle: QueenHandle) {
        self.handles.insert(queen_id, handle);
    }

    /// Remove a dead Queen from the registry (before respawning).
    pub async fn unregister_queen(&mut self, queen_id: &QueenId) -> Option<QueenHandle> {
        self.mailbox.lock().unregister_queen(queen_id);

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

/// Parse review task ID to extract queen_id and original task_id.
/// Review task ID format: "review-Q0-prd-1" → ("Q0", "prd-1")
fn parse_review_task_id(review_task_id: &str) -> (QueenId, String) {
    if let Some(rest) = review_task_id.strip_prefix("review-") {
        // Find first hyphen after "review-"
        if let Some(pos) = rest.find('-') {
            let queen_id = &rest[..pos];
            let task_id = &rest[pos + 1..];
            return (QueenId(queen_id.to_string()), task_id.to_string());
        }
    }
    // Fallback if parsing fails
    (QueenId("unknown".to_string()), review_task_id.to_string())
}

/// Extract content from XML-like tags.
fn extract_tag(text: &str, tag: &str) -> Option<String> {
    let open = format!("<{}>", tag);
    let close = format!("</{}>", tag);
    if let Some(start) = text.find(&open) {
        if let Some(end) = text.find(&close) {
            let content = &text[start + open.len()..end];
            return Some(content.trim().to_string());
        }
    }
    None
}

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
    fn test_create_nydus_with_default_config() {
        let config = NydusConfig::default();
        let host = Nydus::new(NydusId::default(), config);
        assert!(host.is_ok());
        let host = host.unwrap();
        assert_eq!(host.queen_count(), 0);
        assert_eq!(host.iteration, 0);
    }

    #[test]
    fn test_add_tasks_to_dag() {
        let config = NydusConfig::default();
        let mut host = Nydus::new(NydusId::default(), config).unwrap();
        host.add_task("task1", "First task", vec![], Priority::High, Complexity::Medium, None, None);
        host.add_task("task2", "Second task", vec!["task1".to_string()], Priority::Normal, Complexity::Simple, None, None);
        let progress = host.progress();
        assert_eq!(progress.total_tasks, 2);
        assert_eq!(progress.blocked, 1);
    }

    #[test]
    fn test_is_complete_returns_false_when_tasks_pending() {
        let config = NydusConfig::default();
        let mut host = Nydus::new(NydusId::default(), config).unwrap();
        host.add_task("task1", "Test task", vec![], Priority::High, Complexity::Trivial, None, None);
        assert!(!host.is_complete());
    }

    #[test]
    fn test_is_complete_returns_true_when_all_done() {
        let config = NydusConfig::default();
        let host = Nydus::new(NydusId::default(), config).unwrap();
        assert!(!host.is_complete());
    }

    #[test]
    fn test_progress_returns_correct_counts() {
        let config = NydusConfig::default();
        let mut host = Nydus::new(NydusId::default(), config).unwrap();
        host.add_task("task1", "T1", vec![], Priority::High, Complexity::Trivial, None, None);
        host.add_task("task2", "T2", vec!["task1".to_string()], Priority::Normal, Complexity::Medium, None, None);
        host.add_task("task3", "T3", vec![], Priority::Low, Complexity::VeryComplex, None, None);
        let progress = host.progress();
        assert_eq!(progress.total_tasks, 3);
        assert_eq!(progress.completed, 0);
        assert_eq!(progress.blocked, 1);
    }

    #[test]
    fn test_drain_outbox_works() {
        let config = NydusConfig::default();
        let mut host = Nydus::new(NydusId::default(), config).unwrap();
        let messages = host.drain_outbox();
        assert_eq!(messages.len(), 0);
    }

    #[tokio::test]
    async fn test_try_schedule_with_handle() {
        let config = NydusConfig::default();
        let mut host = Nydus::new(NydusId::default(), config).unwrap();

        // Create a mock queen handle
        let (cmd_tx, mut cmd_rx) = mpsc::channel(64);
        let (_status_tx, status_rx) = watch::channel(QueenStatus::Idle);
        let handle = QueenHandle::new(
            QueenId("Q0".to_string()),
            cmd_tx,
            status_rx,
        );

        host.handles.insert(QueenId("Q0".to_string()), handle);

        // Add a task
        host.add_task("task1", "Test task", vec![], Priority::High, Complexity::Trivial, None, None);

        // Schedule
        let assigned = host.try_schedule().await.unwrap();
        assert_eq!(assigned, 1);

        // Verify command was sent
        let cmd = cmd_rx.recv().await.unwrap();
        assert!(matches!(cmd, QueenCommand::Assign { .. }));
    }

    #[tokio::test]
    async fn test_handle_event_task_completed() {
        let config = NydusConfig::default();
        let mut host = Nydus::new(NydusId::default(), config).unwrap();

        // Add and assign a task
        host.add_task("T1", "Test task", vec![], Priority::High, Complexity::Trivial, None, None);
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

        // Task should be in Validating status (awaiting Overlord review), not completed
        let stats = host.task_dag.stats();
        assert_eq!(stats.validating, 1);
        assert_eq!(stats.completed, 0);
    }

    #[tokio::test]
    async fn test_handle_event_task_failed() {
        let config = NydusConfig::default();
        let mut host = Nydus::new(NydusId::default(), config).unwrap();

        host.add_task("T1", "Test task", vec![], Priority::High, Complexity::Trivial, None, None);
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
        let config = NydusConfig::default();
        let mut host = Nydus::new(NydusId::default(), config).unwrap();

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
        let config = NydusConfig::default();
        let mut host = Nydus::new(NydusId::default(), config).unwrap();
        assert!(host.shutdown().await.is_ok());
    }

    #[tokio::test]
    async fn test_validated_merge_no_git_isolation() {
        let config = NydusConfig {
            git_isolation: false,
            ..Default::default()
        };
        let mut host = Nydus::new(NydusId::default(), config).unwrap();
        let queen_id = QueenId("Q0".to_string());
        let result = host.validated_merge(&queen_id).await.unwrap();
        assert!(matches!(result, ValidatedMergeResult::NoGitIsolation));
    }

    #[tokio::test]
    async fn test_tick_returns_result() {
        let config = NydusConfig::default();
        let mut host = Nydus::new(NydusId::default(), config).unwrap();

        host.add_task("T1", "Test", vec![], Priority::Normal, Complexity::Trivial, None, None);

        let result = host.tick().await.unwrap();
        assert_eq!(result.iteration, 1);
        assert_eq!(result.tasks_assigned, 0); // No queens registered
    }

    #[tokio::test]
    async fn test_skill_hint_in_task_context() {
        let config = NydusConfig::default();
        let mut host = Nydus::new(NydusId::default(), config).unwrap();

        // Add task with skill_hint
        host.add_task(
            "carousel-task",
            "Create exchange connector using carousel pattern",
            vec![],
            Priority::High,
            Complexity::VeryComplex,
            Some("carousel".to_string()),
            None,
        );

        // Verify the DagTask has the skill_hint
        let dag_task = host.task_dag.get("carousel-task").unwrap();
        assert_eq!(dag_task.skill_hint, Some("carousel".to_string()));

        // Create a mock queen handle to test context propagation
        let (cmd_tx, mut cmd_rx) = mpsc::channel(64);
        let (_status_tx, status_rx) = watch::channel(QueenStatus::Idle);
        let handle = QueenHandle::new(
            QueenId("Q0".to_string()),
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
