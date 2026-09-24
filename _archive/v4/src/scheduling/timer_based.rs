//! Timer-based scheduling: fixed-interval heartbeat scheduling.

use super::Scheduling;
use crate::core::types::{AgentId, TaskId};
use anyhow::Result;
use parking_lot::Mutex;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Duration;

/// Configuration for timer-based scheduling.
#[derive(Debug, Clone)]
pub struct TimerBasedConfig {
    /// Fixed interval between scheduling checks
    pub interval: Duration,
}

impl Default for TimerBasedConfig {
    fn default() -> Self {
        TimerBasedConfig {
            interval: Duration::from_secs(1),
        }
    }
}

/// Timer-based scheduler with fixed-interval heartbeat.
pub struct TimerBasedScheduling {
    /// Task queue (FIFO)
    task_queue: Arc<Mutex<VecDeque<TaskId>>>,
    /// Current agent assignments: agent_key -> task_id
    agent_assignments: Arc<Mutex<HashMap<String, TaskId>>>,
    /// Completed tasks
    completed_tasks: Arc<Mutex<Vec<TaskId>>>,
    /// Configuration
    config: TimerBasedConfig,
    /// Number of ticks elapsed
    tick_count: Arc<Mutex<u64>>,
}

impl TimerBasedScheduling {
    /// Create a new timer-based scheduler.
    pub fn new(config: TimerBasedConfig) -> Self {
        TimerBasedScheduling {
            task_queue: Arc::new(Mutex::new(VecDeque::new())),
            agent_assignments: Arc::new(Mutex::new(HashMap::new())),
            completed_tasks: Arc::new(Mutex::new(Vec::new())),
            config,
            tick_count: Arc::new(Mutex::new(0)),
        }
    }

    /// Add a task to the queue.
    pub fn add_task(&self, task_id: TaskId) {
        let mut queue = self.task_queue.lock();
        queue.push_back(task_id);
    }

    /// Add multiple tasks to the queue.
    pub fn add_tasks(&self, task_ids: Vec<TaskId>) {
        let mut queue = self.task_queue.lock();
        for task_id in task_ids {
            queue.push_back(task_id);
        }
    }

    /// Increment the tick counter.
    pub fn tick(&self) {
        let mut ticks = self.tick_count.lock();
        *ticks += 1;
    }

    /// Get the current tick count.
    pub fn tick_count(&self) -> u64 {
        *self.tick_count.lock()
    }

    /// Get the number of tasks in the queue.
    pub fn queue_length(&self) -> usize {
        self.task_queue.lock().len()
    }

    /// Get the number of active assignments.
    pub fn active_assignments(&self) -> usize {
        self.agent_assignments.lock().len()
    }

    /// Get the number of completed tasks.
    pub fn completed_count(&self) -> usize {
        self.completed_tasks.lock().len()
    }

    /// Convert AgentId to string key for HashMap.
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

impl Scheduling for TimerBasedScheduling {
    fn next_task(&mut self, agent_id: AgentId) -> Result<Option<TaskId>> {
        // Increment tick on each scheduling check
        self.tick();

        let mut queue = self.task_queue.lock();

        if let Some(task_id) = queue.pop_front() {
            // Assign to agent
            let agent_key = Self::agent_key(&agent_id);
            let mut assignments = self.agent_assignments.lock();
            assignments.insert(agent_key, task_id.clone());

            Ok(Some(task_id))
        } else {
            Ok(None)
        }
    }

    fn notify_completion(&mut self, task_id: TaskId, agent_id: AgentId) -> Result<()> {
        let agent_key = Self::agent_key(&agent_id);

        // Remove from agent assignments
        let mut assignments = self.agent_assignments.lock();
        assignments.remove(&agent_key);

        // Add to completed tasks
        let mut completed = self.completed_tasks.lock();
        completed.push(task_id);

        Ok(())
    }

    fn next_interval(&self) -> Duration {
        self.config.interval
    }

    fn should_terminate(&self) -> bool {
        // Terminate when queue is empty AND no active assignments
        let queue = self.task_queue.lock();
        let assignments = self.agent_assignments.lock();

        queue.is_empty() && assignments.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::types::QueenId;

    #[test]
    fn test_timer_based_basic() {
        let config = TimerBasedConfig {
            interval: Duration::from_millis(500),
        };
        let mut scheduler = TimerBasedScheduling::new(config);

        // Add tasks
        scheduler.add_task(TaskId("task1".to_string()));
        scheduler.add_task(TaskId("task2".to_string()));

        assert_eq!(scheduler.queue_length(), 2);
        assert_eq!(scheduler.tick_count(), 0);

        // Request task (increments tick)
        let agent_id = AgentId::Queen(QueenId("Q0".to_string()));
        let task1 = scheduler.next_task(agent_id.clone()).unwrap();
        assert_eq!(task1, Some(TaskId("task1".to_string())));
        assert_eq!(scheduler.tick_count(), 1);
        assert_eq!(scheduler.queue_length(), 1);

        // Complete task
        scheduler
            .notify_completion(TaskId("task1".to_string()), agent_id)
            .unwrap();
        assert_eq!(scheduler.completed_count(), 1);
    }

    #[test]
    fn test_timer_based_interval() {
        let config = TimerBasedConfig {
            interval: Duration::from_millis(250),
        };
        let scheduler = TimerBasedScheduling::new(config);

        assert_eq!(scheduler.next_interval(), Duration::from_millis(250));
    }

    #[test]
    fn test_timer_based_tick_increment() {
        let config = TimerBasedConfig::default();
        let mut scheduler = TimerBasedScheduling::new(config);

        scheduler.add_task(TaskId("task1".to_string()));

        let agent_id = AgentId::Queen(QueenId("Q0".to_string()));

        // Each next_task call increments tick
        scheduler.next_task(agent_id.clone()).unwrap();
        assert_eq!(scheduler.tick_count(), 1);

        scheduler.next_task(agent_id.clone()).unwrap();
        assert_eq!(scheduler.tick_count(), 2);

        scheduler.next_task(agent_id.clone()).unwrap();
        assert_eq!(scheduler.tick_count(), 3);
    }
}
