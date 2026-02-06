//! SharedMemory — thread-safe knowledge store for the swarm.
//!
//! In-memory HashMap backed by file persistence.
//! All agents (Coordinator + Workers) communicate through this shared state.

use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub type TaskId = usize;
pub type WorkerId = usize;

// ── State ────────────────────────────────────────────────────────────

/// Full shared memory state (serializable).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedMemoryState {
    pub version: u32,
    pub knowledge: HashMap<String, KnowledgeEntry>,
    pub tasks: Vec<SwarmTask>,
    pub messages: VecDeque<Message>,
    pub results: HashMap<TaskId, TaskResult>,
    pub metadata: Metadata,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeEntry {
    pub key: String,
    pub value: serde_json::Value,
    pub author: WorkerId,
    pub timestamp: DateTime<Utc>,
    pub ttl_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmTask {
    pub id: TaskId,
    pub text: String,
    pub status: TaskStatus,
    pub dependencies: Vec<TaskId>,
    pub assigned_to: Option<WorkerId>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    /// Original line number in PRD (for marking checkboxes).
    pub prd_line: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskStatus {
    Blocked,
    Ready,
    Assigned,
    InProgress,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub from: WorkerId,
    pub to: Target,
    pub text: String,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Target {
    Worker(WorkerId),
    Coordinator,
    All,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskResult {
    pub task_id: TaskId,
    pub worker_id: WorkerId,
    pub status: TaskStatus,
    pub verification_output: String,
    pub error_message: Option<String>,
    pub duration_secs: u64,
    pub git_commit_sha: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Metadata {
    pub worker_count: usize,
    pub coordinator_model: String,
    pub worker_model: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// ── SharedMemory handle ──────────────────────────────────────────────

/// Thread-safe shared memory with file persistence.
#[derive(Clone)]
pub struct SharedMemory {
    state: Arc<RwLock<SharedMemoryState>>,
    persist_path: Option<PathBuf>,
}

impl SharedMemory {
    /// Create new empty shared memory.
    pub fn new(persist_path: Option<PathBuf>, worker_count: usize) -> Self {
        let state = SharedMemoryState {
            version: 1,
            knowledge: HashMap::new(),
            tasks: Vec::new(),
            messages: VecDeque::new(),
            results: HashMap::new(),
            metadata: Metadata {
                worker_count,
                coordinator_model: String::new(),
                worker_model: String::new(),
                created_at: Utc::now(),
                updated_at: Utc::now(),
            },
        };
        Self {
            state: Arc::new(RwLock::new(state)),
            persist_path,
        }
    }

    /// Load from file, falling back to empty state on error.
    pub fn load(path: &Path, worker_count: usize) -> Self {
        let state = std::fs::read_to_string(path)
            .ok()
            .and_then(|json| serde_json::from_str::<SharedMemoryState>(&json).ok())
            .unwrap_or_else(|| SharedMemoryState {
                version: 1,
                knowledge: HashMap::new(),
                tasks: Vec::new(),
                messages: VecDeque::new(),
                results: HashMap::new(),
                metadata: Metadata {
                    worker_count,
                    coordinator_model: String::new(),
                    worker_model: String::new(),
                    created_at: Utc::now(),
                    updated_at: Utc::now(),
                },
            });

        Self {
            state: Arc::new(RwLock::new(state)),
            persist_path: Some(path.to_path_buf()),
        }
    }

    // ── Knowledge ────────────────────────────────────────────────────

    /// Set a knowledge entry. Overwrites existing.
    pub fn set_knowledge(&self, key: String, value: serde_json::Value, author: WorkerId) {
        let mut state = self.state.write();
        state.knowledge.insert(
            key.clone(),
            KnowledgeEntry {
                key,
                value,
                author,
                timestamp: Utc::now(),
                ttl_seconds: 3600,
            },
        );
        state.metadata.updated_at = Utc::now();
        drop(state);
        self.persist();
    }

    /// Get knowledge matching a glob-like prefix pattern (e.g. "api.*" matches "api.auth").
    pub fn get_knowledge(&self, pattern: &str) -> HashMap<String, serde_json::Value> {
        let state = self.state.read();
        let prefix = pattern.trim_end_matches('*');
        state
            .knowledge
            .iter()
            .filter(|(k, entry)| {
                k.starts_with(prefix) && !is_expired(entry)
            })
            .map(|(k, v)| (k.clone(), v.value.clone()))
            .collect()
    }

    /// Get all non-expired knowledge as formatted string (for prompt injection).
    pub fn knowledge_summary(&self) -> String {
        let state = self.state.read();
        let mut lines = Vec::new();
        for (key, entry) in &state.knowledge {
            if !is_expired(entry) {
                lines.push(format!("  {} = {}", key, entry.value));
            }
        }
        if lines.is_empty() {
            "(none)".to_string()
        } else {
            lines.join("\n")
        }
    }

    /// Prune expired knowledge entries.
    pub fn prune_expired(&self) {
        let mut state = self.state.write();
        state.knowledge.retain(|_, entry| !is_expired(entry));
        drop(state);
        self.persist();
    }

    // ── Tasks ────────────────────────────────────────────────────────

    /// Set tasks from PRD parsing.
    pub fn set_tasks(&self, tasks: Vec<SwarmTask>) {
        let mut state = self.state.write();
        state.tasks = tasks;
        state.metadata.updated_at = Utc::now();
        drop(state);
        self.persist();
    }

    /// Get IDs of tasks in Ready status.
    pub fn get_ready_tasks(&self) -> Vec<TaskId> {
        let state = self.state.read();
        state
            .tasks
            .iter()
            .filter(|t| t.status == TaskStatus::Ready)
            .map(|t| t.id)
            .collect()
    }

    /// Update a task's status.
    pub fn update_task_status(&self, task_id: TaskId, status: TaskStatus) {
        let mut state = self.state.write();
        if let Some(task) = state.tasks.iter_mut().find(|t| t.id == task_id) {
            task.status = status;
            match status {
                TaskStatus::InProgress => task.started_at = Some(Utc::now()),
                TaskStatus::Completed | TaskStatus::Failed => task.completed_at = Some(Utc::now()),
                _ => {}
            }
        }
        state.metadata.updated_at = Utc::now();
        drop(state);
        self.persist();
    }

    /// Assign a task to a worker.
    pub fn assign_task(&self, task_id: TaskId, worker_id: WorkerId) {
        let mut state = self.state.write();
        if let Some(task) = state.tasks.iter_mut().find(|t| t.id == task_id) {
            task.status = TaskStatus::Assigned;
            task.assigned_to = Some(worker_id);
        }
        state.metadata.updated_at = Utc::now();
        drop(state);
        self.persist();
    }

    /// Get task text by ID.
    pub fn get_task_text(&self, task_id: TaskId) -> Option<String> {
        let state = self.state.read();
        state.tasks.iter().find(|t| t.id == task_id).map(|t| t.text.clone())
    }

    /// Recalculate task readiness based on dependency completion.
    /// Blocked tasks with all deps completed → Ready.
    pub fn refresh_task_readiness(&self) {
        let mut state = self.state.write();
        let completed: Vec<TaskId> = state
            .tasks
            .iter()
            .filter(|t| t.status == TaskStatus::Completed)
            .map(|t| t.id)
            .collect();

        for task in &mut state.tasks {
            if task.status == TaskStatus::Blocked {
                let deps_met = task.dependencies.iter().all(|dep| completed.contains(dep));
                if deps_met {
                    task.status = TaskStatus::Ready;
                }
            }
        }
        state.metadata.updated_at = Utc::now();
        drop(state);
        self.persist();
    }

    /// Get progress: (completed, total).
    pub fn progress(&self) -> (usize, usize) {
        let state = self.state.read();
        let done = state.tasks.iter().filter(|t| t.status == TaskStatus::Completed).count();
        (done, state.tasks.len())
    }

    // ── Messages ─────────────────────────────────────────────────────

    /// Send a message.
    pub fn send_message(&self, from: WorkerId, to: Target, text: String) {
        let mut state = self.state.write();
        state.messages.push_back(Message {
            from,
            to,
            text,
            timestamp: Utc::now(),
        });
        // Cap at 100 messages
        while state.messages.len() > 100 {
            state.messages.pop_front();
        }
        state.metadata.updated_at = Utc::now();
        drop(state);
        self.persist();
    }

    /// Get pending messages for a worker (drains them).
    pub fn drain_messages(&self, worker_id: WorkerId) -> Vec<Message> {
        let mut state = self.state.write();
        let (mine, others): (VecDeque<_>, VecDeque<_>) =
            state.messages.drain(..).partition(|m| match m.to {
                Target::Worker(id) => id == worker_id,
                Target::All => true,
                Target::Coordinator => false,
            });
        state.messages = others;
        mine.into_iter().collect()
    }

    /// Get pending messages for the coordinator (drains them).
    pub fn drain_coordinator_messages(&self) -> Vec<Message> {
        let mut state = self.state.write();
        let (mine, others): (VecDeque<_>, VecDeque<_>) =
            state.messages.drain(..).partition(|m| matches!(m.to, Target::Coordinator));
        state.messages = others;
        mine.into_iter().collect()
    }

    // ── Results ──────────────────────────────────────────────────────

    /// Record a task result.
    pub fn set_result(&self, result: TaskResult) {
        let task_id = result.task_id;
        let status = result.status;
        let mut state = self.state.write();
        state.results.insert(task_id, result);
        // Also update task status
        if let Some(task) = state.tasks.iter_mut().find(|t| t.id == task_id) {
            task.status = status;
            task.completed_at = Some(Utc::now());
        }
        state.metadata.updated_at = Utc::now();
        drop(state);
        self.persist();
    }

    // ── Snapshot ─────────────────────────────────────────────────────

    /// Get a full snapshot of the state (for serialization/reporting).
    pub fn snapshot(&self) -> SharedMemoryState {
        self.state.read().clone()
    }

    // ── Persistence ──────────────────────────────────────────────────

    fn persist(&self) {
        let Some(path) = &self.persist_path else {
            return;
        };
        let state = self.state.read();
        let Ok(json) = serde_json::to_string_pretty(&*state) else {
            return;
        };
        drop(state);

        // Atomic write: tmp → rename
        let tmp_path = path.with_extension("json.tmp");
        if std::fs::write(&tmp_path, &json).is_ok() {
            let _ = std::fs::rename(&tmp_path, path);
        }
    }
}

fn is_expired(entry: &KnowledgeEntry) -> bool {
    let elapsed = Utc::now()
        .signed_duration_since(entry.timestamp)
        .num_seconds();
    elapsed > entry.ttl_seconds as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_knowledge_crud() {
        let mem = SharedMemory::new(None, 4);
        mem.set_knowledge("api.auth".into(), serde_json::json!("bearer"), 0);
        mem.set_knowledge("api.url".into(), serde_json::json!("https://example.com"), 1);
        mem.set_knowledge("test.setup".into(), serde_json::json!(["step1", "step2"]), 0);

        let api = mem.get_knowledge("api.*");
        assert_eq!(api.len(), 2);
        assert_eq!(api["api.auth"], serde_json::json!("bearer"));

        let all = mem.get_knowledge("*");
        assert_eq!(all.len(), 3);
    }

    #[test]
    fn test_task_readiness() {
        let mem = SharedMemory::new(None, 2);
        mem.set_tasks(vec![
            SwarmTask {
                id: 1,
                text: "Task A".into(),
                status: TaskStatus::Ready,
                dependencies: vec![],
                assigned_to: None,
                started_at: None,
                completed_at: None,
                prd_line: 0,
            },
            SwarmTask {
                id: 2,
                text: "Task B (depends on A)".into(),
                status: TaskStatus::Blocked,
                dependencies: vec![1],
                assigned_to: None,
                started_at: None,
                completed_at: None,
                prd_line: 1,
            },
        ]);

        // Only task 1 is ready
        assert_eq!(mem.get_ready_tasks(), vec![1]);

        // Complete task 1
        mem.update_task_status(1, TaskStatus::Completed);
        mem.refresh_task_readiness();

        // Now task 2 should be ready
        assert_eq!(mem.get_ready_tasks(), vec![2]);
    }

    #[test]
    fn test_messages() {
        let mem = SharedMemory::new(None, 2);
        mem.send_message(0, Target::Worker(1), "hello from w0".into());
        mem.send_message(1, Target::Worker(0), "hello from w1".into());
        mem.send_message(0, Target::All, "broadcast".into());

        // Worker 1 gets: "hello from w0" + broadcast
        let msgs = mem.drain_messages(1);
        assert_eq!(msgs.len(), 2);

        // Worker 0 gets: "hello from w1" (broadcast already drained for All)
        let msgs = mem.drain_messages(0);
        assert_eq!(msgs.len(), 1);
    }

    #[test]
    fn test_knowledge_summary() {
        let mem = SharedMemory::new(None, 2);
        mem.set_knowledge("api.auth".into(), serde_json::json!("bearer"), 0);
        let summary = mem.knowledge_summary();
        assert!(summary.contains("api.auth"));
        assert!(summary.contains("bearer"));
    }
}
