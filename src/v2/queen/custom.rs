//! CustomQueen — full control stack for non-Claude backends.
//!
//! This Queen implementation manages workers via API calls or subprocesses,
//! with its own mailbox, task scheduler, and memory. It gives full control
//! over worker lifecycle and communication.
//!
//! This is a scaffold implementation. Full API backend integration comes in Phase 6.

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};
use async_trait::async_trait;
use anyhow::{Result, anyhow};
use crate::v2::types::*;
use super::Queen;

// ============================================================================
// Configuration
// ============================================================================

/// Configuration for CustomQueen.
#[derive(Debug, Clone)]
pub struct CustomQueenConfig {
    /// Backend type (API, Codex, etc.)
    pub backend: QueenBackend,
    /// Maximum number of concurrent workers
    pub max_workers: usize,
    /// Timeout for worker operations
    pub timeout: Duration,
    /// Model name for the coordinator Queen's reasoning
    pub coordinator_model: String,
}

// ============================================================================
// Worker Management
// ============================================================================

/// A worker managed by CustomQueen.
pub struct CustomWorker {
    pub id: WorkerId,
    pub status: WorkerStatus,
    pub current_task: Option<TaskId>,
    pub last_activity: Instant,
}

/// Status of a worker.
#[derive(Debug, Clone)]
pub enum WorkerStatus {
    /// Worker is idle and ready to accept tasks
    Idle,
    /// Worker is busy working on a task
    Busy { task_id: TaskId, progress: f32 },
    /// Worker failed with an error
    Failed { error: String },
    /// Worker is dead/unresponsive
    Dead,
}

// ============================================================================
// Mailbox
// ============================================================================

/// Mailbox for CustomQueen — manages incoming and outgoing messages.
pub struct QueenMailbox {
    inbox: VecDeque<SwarmMessage>,
    worker_inboxes: HashMap<WorkerId, VecDeque<SwarmMessage>>,
    event_log: Vec<SwarmMessage>,
}

impl QueenMailbox {
    pub fn new() -> Self {
        Self {
            inbox: VecDeque::new(),
            worker_inboxes: HashMap::new(),
            event_log: Vec::new(),
        }
    }

    pub fn push_to_worker(&mut self, worker_id: &WorkerId, msg: SwarmMessage) {
        self.worker_inboxes
            .entry(worker_id.clone())
            .or_insert_with(VecDeque::new)
            .push_back(msg.clone());
        self.event_log.push(msg);
    }

    pub fn pop_from_worker(&mut self, worker_id: &WorkerId) -> Option<SwarmMessage> {
        self.worker_inboxes
            .get_mut(worker_id)
            .and_then(|q| q.pop_front())
    }

    pub fn push_to_queen(&mut self, msg: SwarmMessage) {
        self.inbox.push_back(msg.clone());
        self.event_log.push(msg);
    }

    pub fn pop_from_queen(&mut self) -> Option<SwarmMessage> {
        self.inbox.pop_front()
    }
}

// ============================================================================
// Task Scheduler
// ============================================================================

/// Task scheduler for CustomQueen — manages pending and assigned tasks.
pub struct TaskScheduler {
    pending: VecDeque<Task>,
    assigned: HashMap<WorkerId, Task>,
}

impl TaskScheduler {
    pub fn new() -> Self {
        Self {
            pending: VecDeque::new(),
            assigned: HashMap::new(),
        }
    }

    pub fn add_task(&mut self, task: Task) {
        self.pending.push_back(task);
    }

    pub fn assign_next(&mut self, worker_id: &WorkerId) -> Option<Task> {
        if let Some(task) = self.pending.pop_front() {
            self.assigned.insert(worker_id.clone(), task.clone());
            Some(task)
        } else {
            None
        }
    }

    pub fn complete_task(&mut self, worker_id: &WorkerId) -> Option<Task> {
        self.assigned.remove(worker_id)
    }

    pub fn all_complete(&self) -> bool {
        self.pending.is_empty() && self.assigned.is_empty()
    }
}

// ============================================================================
// Memory
// ============================================================================

/// Memory for CustomQueen — stores knowledge and task results.
pub struct QueenMemory {
    knowledge: HashMap<String, serde_json::Value>,
    task_results: HashMap<TaskId, TaskResult>,
}

impl QueenMemory {
    pub fn new() -> Self {
        Self {
            knowledge: HashMap::new(),
            task_results: HashMap::new(),
        }
    }

    pub fn store_result(&mut self, task_id: TaskId, result: TaskResult) {
        self.task_results.insert(task_id, result);
    }

    pub fn get_result(&self, task_id: &TaskId) -> Option<&TaskResult> {
        self.task_results.get(task_id)
    }

    pub fn store_knowledge(&mut self, key: String, value: serde_json::Value) {
        self.knowledge.insert(key, value);
    }

    pub fn get_knowledge(&self, key: &str) -> Option<&serde_json::Value> {
        self.knowledge.get(key)
    }
}

// ============================================================================
// CustomQueen
// ============================================================================

/// CustomQueen — full control stack for non-Claude backends.
///
/// This is a scaffold implementation. Full API backend integration (WorkerBackend enum,
/// HTTP calls, PipeProcess) comes in Phase 6. For now, this just defines the structure
/// and implements the Queen trait with placeholder logic.
pub struct CustomQueen {
    id: QueenId,
    backend: QueenBackend,
    workers: Vec<CustomWorker>,
    mailbox: QueenMailbox,
    task_scheduler: TaskScheduler,
    memory: QueenMemory,
    outbox: VecDeque<SwarmMessage>,
    config: CustomQueenConfig,
}

impl CustomQueen {
    /// Create a new CustomQueen.
    pub async fn new(id: QueenId, config: CustomQueenConfig) -> Result<Self> {
        let workers = Vec::with_capacity(config.max_workers);

        Ok(Self {
            id,
            backend: config.backend.clone(),
            workers,
            mailbox: QueenMailbox::new(),
            task_scheduler: TaskScheduler::new(),
            memory: QueenMemory::new(),
            outbox: VecDeque::new(),
            config,
        })
    }

    /// Decompose a task into sub-tasks.
    /// This is a placeholder — full implementation will use LLM reasoning in Phase 6.
    async fn decompose_task(&self, task: &Task, _context: &TaskContext) -> Result<Vec<Task>> {
        // For now, just return the task itself without decomposition
        Ok(vec![task.clone()])
    }

    /// Spawn a new worker (placeholder for Phase 6).
    async fn spawn_worker(&mut self) -> Result<WorkerId> {
        let worker_id = WorkerId(format!("{}.W{}", self.id.0, self.workers.len()));

        let worker = CustomWorker {
            id: worker_id.clone(),
            status: WorkerStatus::Idle,
            current_task: None,
            last_activity: Instant::now(),
        };

        self.workers.push(worker);
        Ok(worker_id)
    }

    /// Assign a task to a specific worker (placeholder for Phase 6).
    async fn assign_to_worker(&mut self, worker_id: &WorkerId, task: Task) -> Result<()> {
        let worker = self.workers
            .iter_mut()
            .find(|w| &w.id == worker_id)
            .ok_or_else(|| anyhow!("Worker not found"))?;

        worker.status = WorkerStatus::Busy {
            task_id: task.id.clone(),
            progress: 0.0,
        };
        worker.current_task = Some(task.id);

        // Actual API call or subprocess communication will be implemented in Phase 6
        Ok(())
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
        self.backend.clone()
    }

    async fn assign(&mut self, task: Task, context: TaskContext) -> Result<()> {
        // 1. Decompose task into sub-tasks (placeholder in scaffold)
        let sub_tasks = self.decompose_task(&task, &context).await?;

        // 2. Add to scheduler
        for sub_task in sub_tasks {
            self.task_scheduler.add_task(sub_task);
        }

        // 3. Spawn workers if needed
        while self.workers.len() < self.config.max_workers
            && self.workers.len() < self.task_scheduler.pending.len() {
            self.spawn_worker().await?;
        }

        // 4. Assign tasks to idle workers (collect IDs first to avoid borrow conflict)
        let idle_worker_ids: Vec<WorkerId> = self.workers
            .iter()
            .filter(|w| matches!(w.status, WorkerStatus::Idle))
            .map(|w| w.id.clone())
            .collect();

        for worker_id in idle_worker_ids {
            if let Some(sub_task) = self.task_scheduler.assign_next(&worker_id) {
                self.assign_to_worker(&worker_id, sub_task).await?;
            }
        }

        Ok(())
    }

    async fn status(&self) -> QueenStatus {
        if self.task_scheduler.all_complete() {
            QueenStatus::Idle
        } else {
            // Find a task ID to report (scaffold implementation)
            let task_id = self.task_scheduler.assigned
                .values()
                .next()
                .map(|t| t.id.clone())
                .unwrap_or_else(|| TaskId::default());

            QueenStatus::Working {
                task_id,
                progress: 0.5,
                sub_tasks: vec![],
            }
        }
    }

    async fn result(&self) -> Option<TaskResult> {
        // Placeholder — full implementation will track and return actual results
        None
    }

    async fn send_message(&mut self, msg: SwarmMessage) -> Result<()> {
        self.mailbox.push_to_queen(msg);
        Ok(())
    }

    async fn drain_outbox(&mut self) -> Vec<SwarmMessage> {
        self.outbox.drain(..).collect()
    }

    async fn is_alive(&self) -> bool {
        // Placeholder — full implementation will check worker health
        true
    }

    async fn shutdown(&mut self) -> Result<()> {
        // Placeholder — full implementation will shutdown all workers
        Ok(())
    }
}
