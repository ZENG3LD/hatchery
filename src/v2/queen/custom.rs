//! CustomQueen — full control stack for non-Claude backends.
//!
//! This Queen implementation manages workers via API calls or subprocesses,
//! with its own mailbox, task scheduler (DAG-aware), and memory. It gives full control
//! over worker lifecycle and communication.

use std::collections::{HashMap, VecDeque};
use std::time::Duration;
use async_trait::async_trait;
use anyhow::{Result, anyhow};
use chrono::{DateTime, Utc};
use crate::v2::types::*;
use crate::v2::queen::Queen;
use crate::v2::task_dag::{TaskDag, DagTask, DagTaskStatus, Priority, Complexity, DagTaskResult};

// ============================================================================
// Worker
// ============================================================================

/// Status of a worker.
#[derive(Debug, Clone, PartialEq)]
pub enum WorkerStatus {
    /// Worker is idle and ready to accept tasks
    Idle,
    /// Worker is busy working on a task
    Working { task_id: String },
    /// Worker is dead/unresponsive
    Dead { since: DateTime<Utc> },
}

/// Information about a worker managed by CustomQueen.
#[derive(Debug, Clone)]
pub struct WorkerInfo {
    /// Unique worker identifier
    pub id: WorkerId,
    /// Current status of the worker
    pub status: WorkerStatus,
    /// Number of tasks completed by this worker
    pub tasks_completed: usize,
    /// Number of characters processed (for context estimation)
    pub chars_processed: usize,
    /// When this worker was started
    pub started_at: DateTime<Utc>,
}

// ============================================================================
// QueenMailbox
// ============================================================================

/// Mailbox for CustomQueen — manages incoming and outgoing messages.
pub struct QueenMailbox {
    /// Per-worker inboxes
    worker_inboxes: HashMap<WorkerId, VecDeque<SwarmMessage>>,
    /// Outgoing messages (to SwarmHost)
    outbox: VecDeque<SwarmMessage>,
    /// Event log (all messages for audit)
    event_log: Vec<SwarmMessage>,
}

impl QueenMailbox {
    pub fn new() -> Self {
        Self {
            worker_inboxes: HashMap::new(),
            outbox: VecDeque::new(),
            event_log: Vec::new(),
        }
    }

    /// Send a message to a specific worker.
    pub fn send_to_worker(&mut self, worker_id: &WorkerId, msg: SwarmMessage) {
        self.worker_inboxes
            .entry(worker_id.clone())
            .or_insert_with(VecDeque::new)
            .push_back(msg.clone());
        self.event_log.push(msg);
    }

    /// Receive a message from a specific worker's inbox.
    pub fn recv_from_worker(&mut self, worker_id: &WorkerId) -> Option<SwarmMessage> {
        self.worker_inboxes
            .get_mut(worker_id)
            .and_then(|q| q.pop_front())
    }

    /// Broadcast a message to all workers.
    pub fn broadcast_to_workers(&mut self, msg: SwarmMessage) {
        for (_, inbox) in self.worker_inboxes.iter_mut() {
            inbox.push_back(msg.clone());
        }
        self.event_log.push(msg);
    }

    /// Add a message to the outbox (to be sent to SwarmHost).
    pub fn push_outbox(&mut self, msg: SwarmMessage) {
        self.outbox.push_back(msg.clone());
        self.event_log.push(msg);
    }

    /// Drain all messages from the outbox.
    pub fn drain_outbox(&mut self) -> Vec<SwarmMessage> {
        self.outbox.drain(..).collect()
    }

    /// Drain all events from the event log.
    pub fn drain_events(&mut self) -> Vec<SwarmMessage> {
        let events = self.event_log.clone();
        self.event_log.clear();
        events
    }

    /// Register a new worker (create its inbox).
    pub fn register_worker(&mut self, worker_id: &WorkerId) {
        self.worker_inboxes.entry(worker_id.clone()).or_insert_with(VecDeque::new);
    }

    /// Unregister a worker (remove its inbox).
    pub fn unregister_worker(&mut self, worker_id: &WorkerId) {
        self.worker_inboxes.remove(worker_id);
    }
}

// ============================================================================
// TaskScheduler
// ============================================================================

/// Task scheduler with DAG-aware dependency tracking.
pub struct TaskScheduler {
    dag: TaskDag,
}

impl TaskScheduler {
    pub fn new() -> Self {
        Self { dag: TaskDag::new() }
    }

    /// Add a task to the scheduler.
    pub fn add_task(&mut self, id: &str, description: &str, blocked_by: Vec<String>, priority: Priority) {
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
            estimated_complexity: Complexity::Medium,
            result: None,
            created_at: Utc::now(),
            started_at: None,
            completed_at: None,
        };
        self.dag.add_task(task);
    }

    /// Get ready task IDs, sorted by priority (highest first).
    pub fn ready_tasks(&self) -> Vec<String> {
        let mut tasks = self.dag.ready_tasks();
        // Sort by priority (highest first)
        tasks.sort_by(|a, b| b.priority.cmp(&a.priority));
        tasks.into_iter().map(|t| t.id.clone()).collect()
    }

    /// Assign a task to a worker.
    pub fn assign_to_worker(&mut self, task_id: &str, worker_id: &WorkerId) {
        // Create a QueenId wrapper for the worker
        let queen_id = QueenId(format!("W:{}", worker_id.0));
        self.dag.assign(task_id, queen_id);
    }

    /// Mark a task as completed.
    pub fn complete_task(&mut self, task_id: &str) {
        let result = DagTaskResult {
            success: true,
            output: "Task completed".to_string(),
            files_modified: Vec::new(),
        };
        self.dag.complete(task_id, result);
    }

    /// Mark a task as failed.
    pub fn fail_task(&mut self, task_id: &str, error: &str) {
        self.dag.fail(task_id, error.to_string());
    }

    /// Check if all tasks are complete.
    pub fn is_complete(&self) -> bool {
        let stats = self.dag.stats();
        stats.total > 0 && stats.completed == stats.total
    }

    /// Get statistics (completed, total).
    pub fn stats(&self) -> (usize, usize) {
        let stats = self.dag.stats();
        (stats.completed, stats.total)
    }
}

// ============================================================================
// CustomQueen Configuration
// ============================================================================

/// Configuration for CustomQueen.
#[derive(Debug, Clone)]
pub struct CustomQueenConfig {
    /// Queen ID
    pub queen_id: String,
    /// Maximum number of concurrent workers
    pub max_workers: usize,
    /// Model name for the coordinator Queen's reasoning
    pub model: String,
    /// Timeout for worker operations
    pub timeout: Duration,
    /// Maximum context tokens (default 200_000)
    pub max_context_tokens: usize,
}

impl Default for CustomQueenConfig {
    fn default() -> Self {
        Self {
            queen_id: "CQ0".to_string(),
            max_workers: 4,
            model: "claude-sonnet-4-5-20250929".to_string(),
            timeout: Duration::from_secs(300),
            max_context_tokens: 200_000,
        }
    }
}

// ============================================================================
// CustomQueen
// ============================================================================

/// CustomQueen — full control stack for non-Claude backends.
pub struct CustomQueen {
    id: QueenId,
    config: CustomQueenConfig,
    workers: HashMap<WorkerId, WorkerInfo>,
    mailbox: QueenMailbox,
    scheduler: TaskScheduler,
    status: QueenStatus,
    current_task: Option<TaskId>,
    result: Option<TaskResult>,
}

impl CustomQueen {
    /// Create a new CustomQueen.
    pub fn new(config: CustomQueenConfig) -> Self {
        let id = QueenId(config.queen_id.clone());
        Self {
            id,
            config,
            workers: HashMap::new(),
            mailbox: QueenMailbox::new(),
            scheduler: TaskScheduler::new(),
            status: QueenStatus::Idle,
            current_task: None,
            result: None,
        }
    }

    /// Spawn a worker. Returns the worker ID.
    pub fn spawn_worker(&mut self, worker_tag: &str) -> Result<WorkerId> {
        if self.workers.len() >= self.config.max_workers {
            return Err(anyhow!("Maximum worker count reached"));
        }

        let worker_id = WorkerId(format!("{}.{}", self.id.0, worker_tag));
        let worker_info = WorkerInfo {
            id: worker_id.clone(),
            status: WorkerStatus::Idle,
            tasks_completed: 0,
            chars_processed: 0,
            started_at: Utc::now(),
        };

        self.workers.insert(worker_id.clone(), worker_info);
        self.mailbox.register_worker(&worker_id);

        Ok(worker_id)
    }

    /// Check if a worker is healthy.
    pub fn check_worker_health(&self, worker_id: &WorkerId) -> bool {
        if let Some(worker) = self.workers.get(worker_id) {
            !matches!(worker.status, WorkerStatus::Dead { .. })
        } else {
            false
        }
    }

    /// Restart a dead worker (replace it with a new one).
    pub fn restart_dead_worker(&mut self, worker_id: &WorkerId) -> Result<WorkerId> {
        if let Some(_old_worker) = self.workers.remove(worker_id) {
            self.mailbox.unregister_worker(worker_id);

            // Extract tag from old worker ID
            let tag = worker_id.0.split('.').last().unwrap_or("W0");
            self.spawn_worker(tag)
        } else {
            Err(anyhow!("Worker not found"))
        }
    }

    /// Get active (non-dead) worker count.
    pub fn active_worker_count(&self) -> usize {
        self.workers.iter()
            .filter(|(_, w)| !matches!(w.status, WorkerStatus::Dead { .. }))
            .count()
    }

    /// Estimate context usage for a worker (chars/4 / max_context_tokens).
    pub fn estimate_context_usage(&self, worker_id: &WorkerId) -> f32 {
        if let Some(worker) = self.workers.get(worker_id) {
            let estimated_tokens = worker.chars_processed / 4;
            (estimated_tokens as f32) / (self.config.max_context_tokens as f32)
        } else {
            0.0
        }
    }

    /// Schedule: assign ready tasks to idle workers.
    /// Returns the number of tasks assigned.
    pub fn schedule(&mut self) -> usize {
        let ready = self.scheduler.ready_tasks();
        let idle_workers: Vec<WorkerId> = self.workers.iter()
            .filter(|(_, w)| matches!(w.status, WorkerStatus::Idle))
            .map(|(id, _)| id.clone())
            .collect();

        let mut assigned = 0;
        for (task_id, worker_id) in ready.iter().zip(idle_workers.iter()) {
            self.scheduler.assign_to_worker(task_id, worker_id);
            if let Some(worker) = self.workers.get_mut(worker_id) {
                worker.status = WorkerStatus::Working { task_id: task_id.clone() };
            }
            assigned += 1;
        }
        assigned
    }

    /// Mark a worker's task as complete and transition worker to Idle.
    pub fn complete_worker_task(&mut self, worker_id: &WorkerId) {
        if let Some(worker) = self.workers.get_mut(worker_id) {
            if let WorkerStatus::Working { ref task_id } = worker.status {
                self.scheduler.complete_task(task_id);
                worker.tasks_completed += 1;
            }
            worker.status = WorkerStatus::Idle;
        }
    }

    /// Mark a worker's task as failed.
    pub fn fail_worker_task(&mut self, worker_id: &WorkerId, error: &str) {
        if let Some(worker) = self.workers.get_mut(worker_id) {
            if let WorkerStatus::Working { ref task_id } = worker.status {
                self.scheduler.fail_task(task_id, error);
            }
            worker.status = WorkerStatus::Idle;
        }
    }
}

// ============================================================================
// Queen Trait Implementation
// ============================================================================

#[async_trait]
impl Queen for CustomQueen {
    fn id(&self) -> QueenId {
        self.id.clone()
    }

    fn backend(&self) -> QueenBackend {
        QueenBackend::ApiGeneric {
            base_url: "http://localhost:8080".to_string(),
            model: self.config.model.clone(),
        }
    }

    async fn assign(&mut self, task: Task, _context: TaskContext) -> Result<()> {
        // Store the current task
        self.current_task = Some(task.id.clone());

        // Decompose task into sub-tasks (for now, just create one sub-task)
        // In a real implementation, this would use LLM reasoning to break down the task
        self.scheduler.add_task(
            &task.id.0,
            &task.description,
            Vec::new(), // No dependencies for top-level task
            Priority::Normal,
        );

        // Update status
        self.status = QueenStatus::Working {
            task_id: task.id.clone(),
            progress: 0.0,
            sub_tasks: Vec::new(),
        };

        // Spawn initial workers if needed
        let workers_needed = self.config.max_workers.min(1); // Start with at least 1 worker
        for i in 0..workers_needed {
            if self.workers.is_empty() {
                self.spawn_worker(&format!("W{}", i))?;
            }
        }

        // Schedule tasks
        self.schedule();

        Ok(())
    }

    async fn status(&self) -> QueenStatus {
        self.status.clone()
    }

    async fn result(&self) -> Option<TaskResult> {
        self.result.clone()
    }

    async fn send_message(&mut self, msg: SwarmMessage) -> Result<()> {
        // Handle incoming messages (placeholder for now)
        self.mailbox.push_outbox(msg);
        Ok(())
    }

    async fn drain_outbox(&mut self) -> Vec<SwarmMessage> {
        self.mailbox.drain_outbox()
    }

    async fn is_alive(&self) -> bool {
        // CustomQueen is alive if it has at least one active worker
        self.active_worker_count() > 0 || self.workers.is_empty()
    }

    async fn shutdown(&mut self) -> Result<()> {
        // Mark all workers as dead
        let now = Utc::now();
        for worker in self.workers.values_mut() {
            worker.status = WorkerStatus::Dead { since: now };
        }
        self.status = QueenStatus::Dead;
        Ok(())
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_custom_queen_with_default_config() {
        let config = CustomQueenConfig::default();
        let queen = CustomQueen::new(config);

        assert_eq!(queen.id.0, "CQ0");
        assert_eq!(queen.workers.len(), 0);
        assert!(matches!(queen.status, QueenStatus::Idle));
    }

    #[test]
    fn test_spawn_workers_up_to_max() {
        let config = CustomQueenConfig {
            max_workers: 3,
            ..Default::default()
        };
        let mut queen = CustomQueen::new(config);

        // Spawn 3 workers
        for i in 0..3 {
            let result = queen.spawn_worker(&format!("W{}", i));
            assert!(result.is_ok());
        }

        assert_eq!(queen.workers.len(), 3);
        assert_eq!(queen.active_worker_count(), 3);

        // Try to spawn a 4th worker (should fail)
        let result = queen.spawn_worker("W3");
        assert!(result.is_err());
    }

    #[test]
    fn test_add_tasks_to_scheduler_and_verify_ready_tasks() {
        let config = CustomQueenConfig::default();
        let mut queen = CustomQueen::new(config);

        // Add tasks to scheduler
        queen.scheduler.add_task("T1", "Task 1", vec![], Priority::High);
        queen.scheduler.add_task("T2", "Task 2", vec!["T1".to_string()], Priority::Normal);
        queen.scheduler.add_task("T3", "Task 3", vec![], Priority::Low);

        // Get ready tasks (should be T1 and T3, sorted by priority)
        let ready = queen.scheduler.ready_tasks();
        assert_eq!(ready.len(), 2);
        assert_eq!(ready[0], "T1"); // High priority first
        assert_eq!(ready[1], "T3"); // Low priority second
    }

    #[test]
    fn test_schedule_assigns_tasks_to_idle_workers() {
        let config = CustomQueenConfig::default();
        let mut queen = CustomQueen::new(config);

        // Spawn workers
        queen.spawn_worker("W0").unwrap();
        queen.spawn_worker("W1").unwrap();

        // Add tasks
        queen.scheduler.add_task("T1", "Task 1", vec![], Priority::Normal);
        queen.scheduler.add_task("T2", "Task 2", vec![], Priority::Normal);

        // Schedule
        let assigned = queen.schedule();
        assert_eq!(assigned, 2);

        // Check that workers are now working
        for worker in queen.workers.values() {
            assert!(matches!(worker.status, WorkerStatus::Working { .. }));
        }
    }

    #[test]
    fn test_complete_worker_task_transitions_status() {
        let config = CustomQueenConfig::default();
        let mut queen = CustomQueen::new(config);

        // Spawn a worker
        let worker_id = queen.spawn_worker("W0").unwrap();

        // Add a task and schedule it
        queen.scheduler.add_task("T1", "Task 1", vec![], Priority::Normal);
        queen.schedule();

        // Complete the task
        queen.complete_worker_task(&worker_id);

        // Check that worker is idle
        let worker = queen.workers.get(&worker_id).unwrap();
        assert!(matches!(worker.status, WorkerStatus::Idle));
        assert_eq!(worker.tasks_completed, 1);

        // Check scheduler stats
        let (completed, total) = queen.scheduler.stats();
        assert_eq!(completed, 1);
        assert_eq!(total, 1);
    }

    #[test]
    fn test_mailbox_send_and_recv() {
        let mut mailbox = QueenMailbox::new();
        let worker_id = WorkerId("W0".to_string());
        mailbox.register_worker(&worker_id);

        // Create a test message
        let msg = SwarmMessage {
            id: "msg1".to_string(),
            from: AgentId::Queen(QueenId("Q0".to_string())),
            to: AgentId::Queen(QueenId("W0".to_string())),
            msg_type: MessageType::TaskAssignment,
            payload: serde_json::Value::Null,
            timestamp: Utc::now(),
            correlation_id: None,
            visibility: Visibility::default_internal(),
        };

        // Send message to worker
        mailbox.send_to_worker(&worker_id, msg.clone());

        // Receive message from worker
        let received = mailbox.recv_from_worker(&worker_id);
        assert!(received.is_some());
        assert_eq!(received.unwrap().id, "msg1");
    }

    #[test]
    fn test_mailbox_broadcast() {
        let mut mailbox = QueenMailbox::new();
        let worker1 = WorkerId("W0".to_string());
        let worker2 = WorkerId("W1".to_string());
        mailbox.register_worker(&worker1);
        mailbox.register_worker(&worker2);

        // Create a test message
        let msg = SwarmMessage {
            id: "broadcast1".to_string(),
            from: AgentId::Queen(QueenId("Q0".to_string())),
            to: AgentId::Queen(QueenId("ALL".to_string())),
            msg_type: MessageType::StatusRequest,
            payload: serde_json::Value::Null,
            timestamp: Utc::now(),
            correlation_id: None,
            visibility: Visibility::default_internal(),
        };

        // Broadcast
        mailbox.broadcast_to_workers(msg);

        // Both workers should receive it
        assert!(mailbox.recv_from_worker(&worker1).is_some());
        assert!(mailbox.recv_from_worker(&worker2).is_some());
    }

    #[test]
    fn test_estimate_context_usage() {
        let config = CustomQueenConfig::default();
        let mut queen = CustomQueen::new(config);
        let worker_id = queen.spawn_worker("W0").unwrap();

        // Simulate processing some characters
        if let Some(worker) = queen.workers.get_mut(&worker_id) {
            worker.chars_processed = 40_000; // 10k tokens
        }

        // Estimate usage
        let usage = queen.estimate_context_usage(&worker_id);
        assert!((usage - 0.05).abs() < 0.001); // 10k / 200k = 0.05
    }

    #[test]
    fn test_is_complete_logic() {
        let mut scheduler = TaskScheduler::new();

        // Add tasks
        scheduler.add_task("T1", "Task 1", vec![], Priority::Normal);
        scheduler.add_task("T2", "Task 2", vec![], Priority::Normal);

        // Not complete yet
        assert!(!scheduler.is_complete());

        // Complete both tasks
        scheduler.complete_task("T1");
        scheduler.complete_task("T2");

        // Should be complete now
        assert!(scheduler.is_complete());
    }

    #[tokio::test]
    async fn test_queen_trait_assign() {
        let config = CustomQueenConfig::default();
        let mut queen = CustomQueen::new(config);

        let task = Task {
            id: TaskId("T1".to_string()),
            description: "Test task".to_string(),
            status: TaskStatus::Ready,
            assigned_to: None,
            priority: 128,
            blocked_by: Vec::new(),
            created_at: Utc::now(),
        };

        let context = TaskContext {
            knowledge: HashMap::new(),
            recent_messages: Vec::new(),
            shared_state: HashMap::new(),
        };

        // Assign task
        let result = queen.assign(task, context).await;
        assert!(result.is_ok());

        // Check that queen is working
        let status = queen.status().await;
        assert!(matches!(status, QueenStatus::Working { .. }));
    }

    #[tokio::test]
    async fn test_queen_trait_shutdown() {
        let config = CustomQueenConfig::default();
        let mut queen = CustomQueen::new(config);

        // Spawn a worker
        queen.spawn_worker("W0").unwrap();

        // Shutdown
        let result = queen.shutdown().await;
        assert!(result.is_ok());

        // Check that all workers are dead
        for worker in queen.workers.values() {
            assert!(matches!(worker.status, WorkerStatus::Dead { .. }));
        }

        // Check that queen is dead
        let status = queen.status().await;
        assert!(matches!(status, QueenStatus::Dead));
    }
}
