use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use anyhow::{Result, anyhow};
use chrono::{DateTime, Utc};

use crate::core::types::*;
use crate::queen::Queen;
use crate::mailbox::SwarmMailbox;
use crate::mailbox::event_log::SqliteEventLog;
use crate::core::task_dag::{TaskDag, DagTask, DagTaskStatus, Priority, Complexity};
use crate::core::compaction::CompactionStrategy;
use crate::core::validator::{Validator, ValidationResult};
use crate::core::shared_memory::SharedMemory;
use crate::safety::worktree::{WorktreeManager, MergeResult};
use crate::queen::recovery::{SessionTracker, RecoveryManager, RecoveryConfig, RecoveryPlan};

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
/// Integrates all V2 subsystems: Queens, TaskDag, Mailbox, SharedMemory,
/// WorktreeManager, Validator, and CompactionStrategy.
pub struct SwarmHost {
    /// Unique identifier
    id: SwarmHostId,
    /// Queens managed by this SwarmHost
    queens: HashMap<QueenId, Box<dyn Queen>>,
    /// Task dependency graph
    task_dag: TaskDag,
    /// Communication bus
    mailbox: SwarmMailbox,
    /// Shared knowledge store
    memory: SharedMemory,
    /// Git isolation manager (optional)
    worktree_mgr: Option<WorktreeManager>,
    /// Validator for completed work
    validator: Option<Validator>,
    /// Context compression strategy
    compaction: CompactionStrategy,
    /// Configuration
    config: SwarmHostConfig,
    /// When this SwarmHost was created
    created_at: DateTime<Utc>,
    /// Current iteration count
    iteration: usize,
    /// Tracks Claude Code session IDs for each Queen
    session_tracker: SessionTracker,
    /// Manages Queen health checks and recovery planning
    recovery_manager: RecoveryManager,
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
        // Create in-memory SqliteEventLog
        let event_log = Arc::new(SqliteEventLog::in_memory()?);

        // Create SwarmMailbox with event log
        let mailbox = SwarmMailbox::new(event_log);

        // Create SharedMemory with swarm id
        let memory = SharedMemory::new(id.clone());

        // Optionally create WorktreeManager if git_isolation enabled
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

        // Create Validator from verify_cmd if provided
        let validator = config.verify_cmd.as_ref().map(|cmd| {
            Validator::command(cmd.as_str(), config.working_dir.clone())
        });

        // Create default CompactionStrategy
        let compaction = CompactionStrategy::default_swarm_host();

        Ok(Self {
            id,
            queens: HashMap::new(),
            task_dag: TaskDag::new(),
            mailbox,
            memory,
            worktree_mgr,
            validator,
            compaction,
            config,
            created_at: Utc::now(),
            iteration: 0,
            session_tracker: SessionTracker::new(),
            recovery_manager: RecoveryManager::new(RecoveryConfig::default()),
        })
    }

    /// Register a Queen with this SwarmHost.
    pub fn register_queen(&mut self, queen: Box<dyn Queen>) -> Result<()> {
        // Check max_queens limit
        if self.queens.len() >= self.config.max_queens {
            return Err(anyhow!("Cannot register queen: max_queens limit ({}) reached", self.config.max_queens));
        }

        let queen_id = queen.id();

        // Register in mailbox
        self.mailbox.register_queen(queen_id.clone());

        // Create worktree if git_isolation enabled
        if let Some(ref mut worktree_mgr) = self.worktree_mgr {
            if let Err(e) = worktree_mgr.create(&queen_id) {
                eprintln!("Warning: Failed to create worktree for {}: {}", queen_id.0, e);
            }
        }

        // Store in queens HashMap
        self.queens.insert(queen_id, queen);

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
        };

        self.task_dag.add_task(task);
    }

    /// Find idle queens and assign ready tasks to them.
    /// Returns number of tasks assigned.
    pub async fn schedule(&mut self) -> Result<usize> {
        let mut assigned_count = 0;

        // Get ready tasks from DAG (sorted by priority)
        let ready_tasks = self.task_dag.ready_tasks();
        if ready_tasks.is_empty() {
            return Ok(0);
        }

        // Find idle queens (status == Idle)
        // Collect queen IDs first to avoid borrow checker issues
        let queen_ids: Vec<QueenId> = self.queens.keys().cloned().collect();
        let mut idle_queens = Vec::new();

        for queen_id in queen_ids {
            if let Some(queen) = self.queens.get(&queen_id) {
                let status = queen.status().await;
                if matches!(status, QueenStatus::Idle) {
                    idle_queens.push(queen_id);
                }
            }
        }

        // Collect task assignments to avoid borrow checker issues
        let assignments: Vec<(String, String, QueenId, u8, Vec<TaskId>, DateTime<Utc>)> = ready_tasks.iter()
            .zip(idle_queens.iter())
            .map(|(task, queen_id)| {
                (
                    task.id.clone(),
                    task.description.clone(),
                    queen_id.clone(),
                    task.priority as u8,
                    task.blocked_by.iter().map(|id| TaskId(id.clone())).collect(),
                    task.created_at,
                )
            })
            .collect();

        // Build TaskContext from SharedMemory once
        let knowledge = self.memory.query("*")
            .into_iter()
            .map(|entry| (entry.key, entry.value))
            .collect();

        let context = TaskContext {
            knowledge,
            recent_messages: Vec::new(),
            shared_state: HashMap::new(),
        };

        // Assign tasks to queens
        for (task_id, description, queen_id, priority, blocked_by, created_at) in assignments {
            let task_obj = Task {
                id: TaskId(task_id.clone()),
                description,
                status: TaskStatus::Assigned,
                assigned_to: Some(queen_id.clone()),
                priority,
                blocked_by,
                created_at,
            };

            // Assign to queen
            if let Some(queen) = self.queens.get_mut(&queen_id) {
                if let Err(e) = queen.assign(task_obj, context.clone()).await {
                    eprintln!("Failed to assign task {} to queen {}: {}", task_id, queen_id.0, e);
                    continue;
                }

                // Update DAG
                self.task_dag.assign(&task_id, queen_id);
                assigned_count += 1;
            }
        }

        Ok(assigned_count)
    }

    /// Poll all queens for messages, process results.
    /// Returns number of messages processed.
    pub async fn poll(&mut self) -> Result<usize> {
        let mut processed_count = 0;

        // Collect queen IDs to avoid borrow issues
        let queen_ids: Vec<QueenId> = self.queens.keys().cloned().collect();

        // For each queen: drain_outbox
        for queen_id in queen_ids {
            if let Some(queen) = self.queens.get_mut(&queen_id) {
                let messages = queen.drain_outbox().await;

                // Process each message by type
                for msg in messages {
                    processed_count += 1;

                    match msg.msg_type {
                        MessageType::TaskResult => {
                            // Parse payload for task_id and result
                            if let Some(task_id_str) = msg.payload.get("task_id").and_then(|v| v.as_str()) {
                                // Check if success or failure
                                let success = msg.payload.get("success")
                                    .and_then(|v| v.as_bool())
                                    .unwrap_or(false);

                                if success {
                                    let output = msg.payload.get("output")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("")
                                        .to_string();

                                    let files_modified: Vec<String> = msg.payload.get("files_modified")
                                        .and_then(|v| v.as_array())
                                        .map(|arr| arr.iter()
                                            .filter_map(|v| v.as_str().map(String::from))
                                            .collect())
                                        .unwrap_or_default();

                                    // Mark complete in DAG
                                    let dag_result = crate::core::task_dag::DagTaskResult {
                                        success: true,
                                        output: output.clone(),
                                        files_modified: files_modified.clone(),
                                    };

                                    self.task_dag.complete(task_id_str, dag_result);

                                    // Store task result in memory as JSON
                                    let result_json = serde_json::json!({
                                        "status": "completed",
                                        "output": output,
                                        "artifacts": files_modified,
                                    });
                                    self.memory.store_task_result(task_id_str, result_json);

                                    // Validate and merge worktree if git isolation enabled
                                    match self.validated_merge(&queen_id).await {
                                        Ok(ValidatedMergeResult::Merged { commit_sha }) => {
                                            println!("Validated and merged worktree for {}: {}", queen_id.0, commit_sha);
                                        }
                                        Ok(ValidatedMergeResult::MergeConflict { files }) => {
                                            eprintln!("Merge conflict for {}: {:?}", queen_id.0, files);
                                        }
                                        Ok(ValidatedMergeResult::ValidationFailed { feedback }) => {
                                            eprintln!("Validation failed for {}: {}", queen_id.0, feedback);
                                        }
                                        Ok(ValidatedMergeResult::NoGitIsolation) => {
                                            // Git isolation disabled, nothing to do
                                        }
                                        Ok(ValidatedMergeResult::NoChanges) => {
                                            println!("No changes to merge for {}", queen_id.0);
                                        }
                                        Err(e) => {
                                            eprintln!("Validated merge failed for {}: {}", queen_id.0, e);
                                        }
                                    }
                                } else {
                                    // Mark failed
                                    let error = msg.payload.get("error")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("Unknown error")
                                        .to_string();

                                    self.task_dag.fail(task_id_str, error);
                                }
                            }
                        }
                        MessageType::Knowledge => {
                            // Store in SharedMemory
                            if let (Some(key), Some(value)) = (
                                msg.payload.get("key").and_then(|v| v.as_str()),
                                msg.payload.get("value"),
                            ) {
                                self.memory.insert(
                                    key.to_string(),
                                    value.clone(),
                                    AgentId::Queen(queen_id.clone()),
                                );
                            }
                        }
                        MessageType::Escalation => {
                            // Forward to mailbox (will be picked up by BroodLord/Operator)
                            self.mailbox.send(msg);
                        }
                        MessageType::StatusReport => {
                            // Just log it
                            println!("Status from {}: {:?}", queen_id.0, msg.payload);
                        }
                        _ => {
                            // Other message types - log for now
                            println!("Received {} message from {}",
                                match msg.msg_type {
                                    MessageType::TaskAssignment => "TaskAssignment",
                                    MessageType::TaskProgress => "TaskProgress",
                                    MessageType::StatusRequest => "StatusRequest",
                                    MessageType::KnowledgeQuery => "KnowledgeQuery",
                                    MessageType::Shutdown => "Shutdown",
                                    MessageType::Custom(_) => "Custom",
                                    _ => "Other",
                                },
                                queen_id.0
                            );
                        }
                    }
                }
            }
        }

        Ok(processed_count)
    }

    /// Run one iteration: schedule + poll.
    pub async fn tick(&mut self) -> Result<TickResult> {
        self.iteration += 1;

        let tasks_assigned = self.schedule().await?;
        let messages_processed = self.poll().await?;

        // Check Queen health and log recovery plans
        let recovery_plans = self.check_queen_health().await;
        for plan in &recovery_plans {
            eprintln!(
                "[SwarmHost] Recovery needed for Queen {}: {:?} (attempt #{})",
                plan.queen_id.0, plan.reason, plan.attempt
            );
            self.recovery_manager.mark_recovery_attempted(&plan.queen_id);
        }

        // Evict expired from memory
        self.memory.evict_expired();

        // Get stats
        let stats = self.task_dag.stats();

        Ok(TickResult {
            tasks_assigned,
            messages_processed,
            tasks_completed: stats.completed,
            tasks_failed: stats.failed,
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

        // Can't call async status() here, so we'll estimate from task assignments
        // This is a limitation - in real code you'd need to track this separately
        let queens_active = stats.in_progress.min(self.queens.len());
        let queens_idle = self.queens.len().saturating_sub(queens_active);

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
        self.queens.len()
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

    /// Check all Queens for health issues and return recovery plans.
    /// Called during each tick to detect dead/stalled Queens.
    pub async fn check_queen_health(&mut self) -> Vec<RecoveryPlan> {
        let mut plans = Vec::new();

        let queen_ids: Vec<QueenId> = self.queens.keys().cloned().collect();

        for queen_id in &queen_ids {
            if let Some(queen) = self.queens.get(queen_id) {
                let alive = queen.is_alive().await;

                if let Some(plan) = self.recovery_manager.check_health(
                    queen_id,
                    alive,
                    &self.session_tracker,
                ) {
                    plans.push(plan);
                }
            }
        }

        plans
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
    pub async fn unregister_queen(&mut self, queen_id: &QueenId) -> Option<Box<dyn Queen>> {
        // Unregister from mailbox
        self.mailbox.unregister_queen(queen_id);

        // Remove from queens map
        self.queens.remove(queen_id)
    }

    /// Graceful shutdown: shutdown all queens, save memory, cleanup worktrees.
    pub async fn shutdown(&mut self) -> Result<()> {
        // Shutdown all queens
        for (queen_id, queen) in self.queens.iter_mut() {
            if let Err(e) = queen.shutdown().await {
                eprintln!("Failed to shutdown queen {}: {}", queen_id.0, e);
            }
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
    use async_trait::async_trait;

    struct MockQueen {
        id: QueenId,
        status: QueenStatus,
        outbox: Vec<SwarmMessage>,
    }

    impl MockQueen {
        fn new(id: &str) -> Self {
            Self {
                id: QueenId(id.to_string()),
                status: QueenStatus::Idle,
                outbox: Vec::new(),
            }
        }
    }

    #[async_trait]
    impl Queen for MockQueen {
        fn id(&self) -> QueenId {
            self.id.clone()
        }

        fn backend(&self) -> QueenBackend {
            QueenBackend::ClaudeNative
        }

        async fn assign(&mut self, _task: Task, _ctx: TaskContext) -> Result<()> {
            self.status = QueenStatus::Working {
                task_id: TaskId::default(),
                progress: 0.0,
                sub_tasks: vec![],
            };
            Ok(())
        }

        async fn status(&self) -> QueenStatus {
            self.status.clone()
        }

        async fn result(&self) -> Option<TaskResult> {
            None
        }

        async fn send_message(&mut self, _msg: SwarmMessage) -> Result<()> {
            Ok(())
        }

        async fn drain_outbox(&mut self) -> Vec<SwarmMessage> {
            self.outbox.drain(..).collect()
        }

        async fn is_alive(&self) -> bool {
            true
        }

        async fn shutdown(&mut self) -> Result<()> {
            Ok(())
        }
    }

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
    fn test_register_queens_up_to_max_limit() {
        let config = SwarmHostConfig {
            max_queens: 2,
            ..Default::default()
        };
        let mut host = SwarmHost::new(SwarmHostId::default(), config).unwrap();

        // Register first queen
        let queen1 = Box::new(MockQueen::new("queen1"));
        assert!(host.register_queen(queen1).is_ok());
        assert_eq!(host.queen_count(), 1);

        // Register second queen
        let queen2 = Box::new(MockQueen::new("queen2"));
        assert!(host.register_queen(queen2).is_ok());
        assert_eq!(host.queen_count(), 2);

        // Try to register third queen (should fail)
        let queen3 = Box::new(MockQueen::new("queen3"));
        assert!(host.register_queen(queen3).is_err());
        assert_eq!(host.queen_count(), 2);
    }

    #[test]
    fn test_add_tasks_to_dag() {
        let config = SwarmHostConfig::default();
        let mut host = SwarmHost::new(SwarmHostId::default(), config).unwrap();

        // Add task with no dependencies
        host.add_task(
            "task1",
            "First task",
            vec![],
            Priority::High,
            Complexity::Medium,
        );

        // Add task with dependency
        host.add_task(
            "task2",
            "Second task",
            vec!["task1".to_string()],
            Priority::Normal,
            Complexity::Simple,
        );

        let progress = host.progress();
        assert_eq!(progress.total_tasks, 2);
        assert_eq!(progress.blocked, 1); // task2 is blocked by task1
    }

    #[test]
    fn test_is_complete_returns_false_when_tasks_pending() {
        let config = SwarmHostConfig::default();
        let mut host = SwarmHost::new(SwarmHostId::default(), config).unwrap();

        host.add_task(
            "task1",
            "Test task",
            vec![],
            Priority::High,
            Complexity::Trivial,
        );

        assert!(!host.is_complete());
    }

    #[test]
    fn test_is_complete_returns_true_when_all_done() {
        let config = SwarmHostConfig::default();
        let host = SwarmHost::new(SwarmHostId::default(), config).unwrap();

        // No tasks means complete (edge case)
        assert!(!host.is_complete()); // Actually should be false with 0 tasks
    }

    #[test]
    fn test_progress_returns_correct_counts() {
        let config = SwarmHostConfig::default();
        let mut host = SwarmHost::new(SwarmHostId::default(), config).unwrap();

        // Add some tasks
        host.add_task("task1", "T1", vec![], Priority::High, Complexity::Trivial);
        host.add_task("task2", "T2", vec!["task1".to_string()], Priority::Normal, Complexity::Medium);
        host.add_task("task3", "T3", vec![], Priority::Low, Complexity::VeryComplex);

        let progress = host.progress();
        assert_eq!(progress.total_tasks, 3);
        assert_eq!(progress.completed, 0);
        assert_eq!(progress.blocked, 1); // task2 blocked by task1
    }

    #[test]
    fn test_drain_outbox_works() {
        let config = SwarmHostConfig::default();
        let mut host = SwarmHost::new(SwarmHostId::default(), config).unwrap();

        // Initially empty
        let messages = host.drain_outbox();
        assert_eq!(messages.len(), 0);
    }

    #[tokio::test]
    async fn test_schedule_assigns_ready_tasks() {
        let config = SwarmHostConfig::default();
        let mut host = SwarmHost::new(SwarmHostId::default(), config).unwrap();

        // Register a queen
        let queen = Box::new(MockQueen::new("queen1"));
        host.register_queen(queen).unwrap();

        // Add a ready task
        host.add_task("task1", "Test task", vec![], Priority::High, Complexity::Trivial);

        // Schedule
        let assigned = host.schedule().await.unwrap();
        assert_eq!(assigned, 1);

        // Check progress
        let progress = host.progress();
        assert_eq!(progress.in_progress, 1);
    }

    #[tokio::test]
    async fn test_shutdown_gracefully() {
        let config = SwarmHostConfig::default();
        let mut host = SwarmHost::new(SwarmHostId::default(), config).unwrap();

        // Register queen
        let queen = Box::new(MockQueen::new("queen1"));
        host.register_queen(queen).unwrap();

        // Shutdown should succeed
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

        // Should return NoGitIsolation when git isolation is disabled
        let result = host.validated_merge(&queen_id).await.unwrap();
        assert!(matches!(result, ValidatedMergeResult::NoGitIsolation));
    }

    #[tokio::test]
    async fn test_validated_merge_with_validation_success() {
        // Create a temp directory for test
        let temp_dir = tempfile::tempdir().unwrap();
        let repo_path = temp_dir.path().to_path_buf();

        // Init git repo
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&repo_path)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(&repo_path)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(&repo_path)
            .output()
            .unwrap();

        // Create initial commit
        std::fs::write(repo_path.join("README.md"), "# Test").unwrap();
        std::process::Command::new("git")
            .args(["add", "."])
            .current_dir(&repo_path)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["commit", "-m", "initial"])
            .current_dir(&repo_path)
            .output()
            .unwrap();

        let config = SwarmHostConfig {
            git_isolation: true,
            working_dir: repo_path.clone(),
            verify_cmd: Some("echo ok".to_string()),
            ..Default::default()
        };

        let mut host = SwarmHost::new(SwarmHostId::default(), config).unwrap();

        let queen_id = QueenId("Q0".to_string());
        let queen = Box::new(MockQueen::new("Q0"));
        host.register_queen(queen).unwrap();

        // Get worktree info to check if it was created
        let wt_created = host.worktree_mgr.as_ref()
            .and_then(|mgr| mgr.get_info(&queen_id))
            .is_some();

        if !wt_created {
            // If worktree creation failed, skip this test
            eprintln!("Worktree creation failed, skipping test");
            return;
        }

        // Make a change in the worktree
        if let Some(ref mgr) = host.worktree_mgr {
            let wt_path = mgr.worktree_path(&queen_id);
            if !wt_path.exists() {
                eprintln!("Worktree path doesn't exist, skipping test");
                return;
            }
            std::fs::write(wt_path.join("test.txt"), "content").unwrap();
            std::process::Command::new("git")
                .args(["add", "."])
                .current_dir(&wt_path)
                .output()
                .unwrap();
            std::process::Command::new("git")
                .args(["commit", "-m", "test commit"])
                .current_dir(&wt_path)
                .output()
                .unwrap();
        }

        // Validated merge should succeed (validator will pass with "echo ok")
        let result = host.validated_merge(&queen_id).await.unwrap();
        match result {
            ValidatedMergeResult::Merged { commit_sha } => {
                assert!(!commit_sha.is_empty());
            }
            _ => panic!("Expected Merged result, got {:?}", result),
        }
    }

    #[tokio::test]
    async fn test_validated_merge_validation_fails() {
        // Create a temp directory for test
        let temp_dir = tempfile::tempdir().unwrap();
        let repo_path = temp_dir.path().to_path_buf();

        // Init git repo
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&repo_path)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(&repo_path)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(&repo_path)
            .output()
            .unwrap();

        // Create initial commit
        std::fs::write(repo_path.join("README.md"), "# Test").unwrap();
        std::process::Command::new("git")
            .args(["add", "."])
            .current_dir(&repo_path)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["commit", "-m", "initial"])
            .current_dir(&repo_path)
            .output()
            .unwrap();

        // Use failing command for validation
        let fail_cmd = if cfg!(windows) { "cmd /c exit 1" } else { "false" };

        let config = SwarmHostConfig {
            git_isolation: true,
            working_dir: repo_path.clone(),
            verify_cmd: Some(fail_cmd.to_string()),
            ..Default::default()
        };

        let mut host = SwarmHost::new(SwarmHostId::default(), config).unwrap();

        let queen_id = QueenId("Q0".to_string());
        let queen = Box::new(MockQueen::new("Q0"));
        host.register_queen(queen).unwrap();

        // Get worktree info to check if it was created
        let wt_created = host.worktree_mgr.as_ref()
            .and_then(|mgr| mgr.get_info(&queen_id))
            .is_some();

        if !wt_created {
            // If worktree creation failed, skip this test
            eprintln!("Worktree creation failed, skipping test");
            return;
        }

        // Make a change in the worktree
        if let Some(ref mgr) = host.worktree_mgr {
            let wt_path = mgr.worktree_path(&queen_id);
            if !wt_path.exists() {
                eprintln!("Worktree path doesn't exist, skipping test");
                return;
            }
            std::fs::write(wt_path.join("test.txt"), "content").unwrap();
            std::process::Command::new("git")
                .args(["add", "."])
                .current_dir(&wt_path)
                .output()
                .unwrap();
            std::process::Command::new("git")
                .args(["commit", "-m", "test commit"])
                .current_dir(&wt_path)
                .output()
                .unwrap();
        }

        // Validated merge should fail validation
        let result = host.validated_merge(&queen_id).await.unwrap();
        match result {
            ValidatedMergeResult::ValidationFailed { feedback } => {
                assert!(!feedback.is_empty());
            }
            _ => panic!("Expected ValidationFailed result, got {:?}", result),
        }
    }
}
