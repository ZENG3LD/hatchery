//! Priority scheduling: priority queue with bottleneck detection.

use super::Scheduling;
use crate::core::types::{AgentId, TaskId};
use anyhow::Result;
use parking_lot::Mutex;
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Task priority level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    /// Lowest priority
    Low,
    /// Normal priority
    Normal,
    /// High priority
    High,
    /// Critical priority with urgency level (0-255)
    Critical(u8),
}

/// Configuration for priority scheduling.
#[derive(Debug, Clone)]
pub struct PriorityConfig {
    /// Priority levels to use
    pub priority_levels: Vec<Priority>,
    /// Starvation threshold (tasks waiting longer than this get promoted)
    pub starvation_threshold: Duration,
}

impl Default for PriorityConfig {
    fn default() -> Self {
        PriorityConfig {
            priority_levels: vec![Priority::Low, Priority::Normal, Priority::High, Priority::Critical(0)],
            starvation_threshold: Duration::from_secs(300), // 5 minutes
        }
    }
}

/// Task with priority and bottleneck score.
#[derive(Debug, Clone)]
pub struct PriorityTask {
    /// Task identifier
    pub task_id: TaskId,
    /// Priority level
    pub priority: Priority,
    /// Bottleneck score (higher = more critical for DAG)
    pub bottleneck_score: f64,
    /// When the task was enqueued
    pub enqueued_at: Instant,
}

impl PriorityTask {
    /// Create a new priority task.
    pub fn new(task_id: TaskId, priority: Priority) -> Self {
        PriorityTask {
            task_id,
            priority,
            bottleneck_score: 0.0,
            enqueued_at: Instant::now(),
        }
    }

    /// Create with bottleneck score.
    pub fn with_bottleneck_score(task_id: TaskId, priority: Priority, bottleneck_score: f64) -> Self {
        PriorityTask {
            task_id,
            priority,
            bottleneck_score,
            enqueued_at: Instant::now(),
        }
    }

    /// Get wait time.
    pub fn wait_time(&self) -> Duration {
        self.enqueued_at.elapsed()
    }
}

impl PartialEq for PriorityTask {
    fn eq(&self, other: &Self) -> bool {
        self.priority == other.priority && self.bottleneck_score == other.bottleneck_score
    }
}

impl Eq for PriorityTask {}

impl Ord for PriorityTask {
    fn cmp(&self, other: &Self) -> Ordering {
        // First compare by priority (reversed because BinaryHeap is max-heap)
        match self.priority.cmp(&other.priority) {
            Ordering::Equal => {
                // Then by bottleneck score
                self.bottleneck_score
                    .partial_cmp(&other.bottleneck_score)
                    .unwrap_or(Ordering::Equal)
            }
            other => other,
        }
    }
}

impl PartialOrd for PriorityTask {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Priority scheduler with bottleneck detection.
pub struct PriorityScheduling {
    /// Priority queue (BinaryHeap is max-heap)
    task_queue: Arc<Mutex<BinaryHeap<PriorityTask>>>,
    /// Active assignments: agent_key -> task_id
    active_assignments: Arc<Mutex<HashMap<String, TaskId>>>,
    /// Task lookup: task_id -> PriorityTask (for promotion)
    task_lookup: Arc<Mutex<HashMap<String, PriorityTask>>>,
    /// Completed tasks
    completed_tasks: Arc<Mutex<Vec<TaskId>>>,
    /// Configuration
    config: PriorityConfig,
}

impl PriorityScheduling {
    /// Create a new priority scheduler.
    pub fn new(config: PriorityConfig) -> Self {
        PriorityScheduling {
            task_queue: Arc::new(Mutex::new(BinaryHeap::new())),
            active_assignments: Arc::new(Mutex::new(HashMap::new())),
            task_lookup: Arc::new(Mutex::new(HashMap::new())),
            completed_tasks: Arc::new(Mutex::new(Vec::new())),
            config,
        }
    }

    /// Add a task with specified priority.
    pub fn add_task_with_priority(&self, task_id: TaskId, priority: Priority) {
        let task = PriorityTask::new(task_id.clone(), priority);

        let mut queue = self.task_queue.lock();
        queue.push(task.clone());

        let mut lookup = self.task_lookup.lock();
        lookup.insert(task_id.0, task);
    }

    /// Add a task with priority and bottleneck score.
    pub fn add_task_with_score(&self, task_id: TaskId, priority: Priority, bottleneck_score: f64) {
        let task = PriorityTask::with_bottleneck_score(task_id.clone(), priority, bottleneck_score);

        let mut queue = self.task_queue.lock();
        queue.push(task.clone());

        let mut lookup = self.task_lookup.lock();
        lookup.insert(task_id.0, task);
    }

    /// Promote a task to higher priority.
    pub fn promote(&self, task_id: &TaskId, new_priority: Priority) -> Result<()> {
        let mut lookup = self.task_lookup.lock();

        if let Some(task) = lookup.get_mut(&task_id.0) {
            task.priority = new_priority;

            // Rebuild heap with new priority
            let mut queue = self.task_queue.lock();
            let tasks: Vec<PriorityTask> = queue.drain().collect();
            for t in tasks {
                if t.task_id.0 == task_id.0 {
                    queue.push(task.clone());
                } else {
                    queue.push(t);
                }
            }
        }

        Ok(())
    }

    /// Check for starvation and promote waiting tasks.
    pub fn starvation_check(&self) -> usize {
        let mut promoted_count = 0;
        let threshold = self.config.starvation_threshold;

        let mut lookup = self.task_lookup.lock();
        let mut queue = self.task_queue.lock();

        // Collect tasks that have been waiting too long
        let mut tasks_to_promote: Vec<String> = Vec::new();

        for (task_id, task) in lookup.iter() {
            if task.wait_time() > threshold && !matches!(task.priority, Priority::Critical(_)) {
                tasks_to_promote.push(task_id.clone());
            }
        }

        // Promote them
        for task_id in tasks_to_promote {
            if let Some(task) = lookup.get_mut(&task_id) {
                task.priority = Priority::Critical(255);
                promoted_count += 1;
            }
        }

        // Rebuild heap if any promotions occurred
        if promoted_count > 0 {
            let tasks: Vec<PriorityTask> = queue.drain().collect();
            for task in tasks {
                if let Some(updated) = lookup.get(&task.task_id.0) {
                    queue.push(updated.clone());
                } else {
                    queue.push(task);
                }
            }
        }

        promoted_count
    }

    /// Get the number of tasks in the queue.
    pub fn queue_length(&self) -> usize {
        self.task_queue.lock().len()
    }

    /// Get the number of active assignments.
    pub fn active_assignments_count(&self) -> usize {
        self.active_assignments.lock().len()
    }

    /// Get the number of completed tasks.
    pub fn completed_count(&self) -> usize {
        self.completed_tasks.lock().len()
    }

    /// Convert AgentId to string key.
    fn agent_key(agent_id: &AgentId) -> String {
        match agent_id {
            AgentId::Nydus(id) => format!("nydus:{}", id.0),
            AgentId::Queen(id) => format!("queen:{}", id.0),
            AgentId::Overlord(id) => format!("overlord:{}", id),
            AgentId::Overmind(id) => format!("overmind:{}", id),
            AgentId::Validator => "validator".to_string(),
            AgentId::Operator => "operator".to_string(),
        }
    }
}

impl Scheduling for PriorityScheduling {
    fn next_task(&mut self, agent_id: AgentId) -> Result<Option<TaskId>> {
        let mut queue = self.task_queue.lock();

        if let Some(priority_task) = queue.pop() {
            let task_id = priority_task.task_id.clone();

            // Record assignment
            let agent_key = Self::agent_key(&agent_id);
            let mut assignments = self.active_assignments.lock();
            assignments.insert(agent_key, task_id.clone());

            // Remove from lookup
            let mut lookup = self.task_lookup.lock();
            lookup.remove(&task_id.0);

            Ok(Some(task_id))
        } else {
            Ok(None)
        }
    }

    fn notify_completion(&mut self, task_id: TaskId, agent_id: AgentId) -> Result<()> {
        let agent_key = Self::agent_key(&agent_id);

        // Remove from assignments
        let mut assignments = self.active_assignments.lock();
        assignments.remove(&agent_key);

        // Add to completed
        let mut completed = self.completed_tasks.lock();
        completed.push(task_id);

        Ok(())
    }

    fn next_interval(&self) -> Duration {
        Duration::from_millis(100)
    }

    fn should_terminate(&self) -> bool {
        let queue = self.task_queue.lock();
        let assignments = self.active_assignments.lock();

        queue.is_empty() && assignments.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::types::QueenId;

    #[test]
    fn test_priority_ordering() {
        let low = PriorityTask::new(TaskId("low".to_string()), Priority::Low);
        let normal = PriorityTask::new(TaskId("normal".to_string()), Priority::Normal);
        let high = PriorityTask::new(TaskId("high".to_string()), Priority::High);
        let critical = PriorityTask::new(TaskId("critical".to_string()), Priority::Critical(100));

        assert!(critical > high);
        assert!(high > normal);
        assert!(normal > low);
    }

    #[test]
    fn test_priority_scheduling_basic() {
        let config = PriorityConfig::default();
        let mut scheduler = PriorityScheduling::new(config);

        scheduler.add_task_with_priority(TaskId("task1".to_string()), Priority::Low);
        scheduler.add_task_with_priority(TaskId("task2".to_string()), Priority::High);
        scheduler.add_task_with_priority(TaskId("task3".to_string()), Priority::Normal);

        let agent_id = AgentId::Queen(QueenId("Q0".to_string()));

        // Should get high priority first
        let task1 = scheduler.next_task(agent_id.clone()).unwrap();
        assert_eq!(task1, Some(TaskId("task2".to_string())));

        // Then normal
        let task2 = scheduler.next_task(agent_id.clone()).unwrap();
        assert_eq!(task2, Some(TaskId("task3".to_string())));

        // Then low
        let task3 = scheduler.next_task(agent_id.clone()).unwrap();
        assert_eq!(task3, Some(TaskId("task1".to_string())));
    }

    #[test]
    fn test_priority_with_bottleneck_score() {
        let config = PriorityConfig::default();
        let mut scheduler = PriorityScheduling::new(config);

        scheduler.add_task_with_score(TaskId("task1".to_string()), Priority::High, 0.5);
        scheduler.add_task_with_score(TaskId("task2".to_string()), Priority::High, 0.9);

        let agent_id = AgentId::Queen(QueenId("Q0".to_string()));

        // Should get higher bottleneck score first
        let task1 = scheduler.next_task(agent_id.clone()).unwrap();
        assert_eq!(task1, Some(TaskId("task2".to_string())));
    }

    #[test]
    fn test_priority_promotion() {
        let config = PriorityConfig::default();
        let scheduler = PriorityScheduling::new(config);

        scheduler.add_task_with_priority(TaskId("task1".to_string()), Priority::Low);

        scheduler
            .promote(&TaskId("task1".to_string()), Priority::Critical(255))
            .unwrap();

        // Verify promotion happened
        let lookup = scheduler.task_lookup.lock();
        let task = lookup.get("task1").unwrap();
        assert_eq!(task.priority, Priority::Critical(255));
    }
}
