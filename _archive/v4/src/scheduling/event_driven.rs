//! Event-driven scheduling: tasks dispatched immediately when events arrive.

use super::Scheduling;
use crate::core::types::{AgentId, TaskId};
use anyhow::Result;
use parking_lot::Mutex;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Duration;

/// Configuration for event-driven scheduling.
#[derive(Debug, Clone)]
pub struct EventDrivenConfig {
    /// Whether to enable wakeup notifications for new tasks
    pub wakeup_enabled: bool,
}

impl Default for EventDrivenConfig {
    fn default() -> Self {
        EventDrivenConfig {
            wakeup_enabled: true,
        }
    }
}

/// Event-driven scheduler that dispatches tasks immediately when they arrive.
pub struct EventDrivenScheduling {
    /// Task queue (FIFO)
    task_queue: Arc<Mutex<VecDeque<TaskId>>>,
    /// Current agent assignments: agent_key -> task_id
    agent_assignments: Arc<Mutex<HashMap<String, TaskId>>>,
    /// Completed tasks
    completed_tasks: Arc<Mutex<Vec<TaskId>>>,
    /// Configuration
    config: EventDrivenConfig,
    /// Wakeup flag (set when new task added)
    wakeup_flag: Arc<Mutex<bool>>,
}

impl EventDrivenScheduling {
    /// Create a new event-driven scheduler.
    pub fn new(config: EventDrivenConfig) -> Self {
        EventDrivenScheduling {
            task_queue: Arc::new(Mutex::new(VecDeque::new())),
            agent_assignments: Arc::new(Mutex::new(HashMap::new())),
            completed_tasks: Arc::new(Mutex::new(Vec::new())),
            config,
            wakeup_flag: Arc::new(Mutex::new(false)),
        }
    }

    /// Add a task to the queue.
    pub fn add_task(&self, task_id: TaskId) {
        let mut queue = self.task_queue.lock();
        queue.push_back(task_id);

        if self.config.wakeup_enabled {
            let mut wakeup = self.wakeup_flag.lock();
            *wakeup = true;
        }
    }

    /// Add multiple tasks to the queue.
    pub fn add_tasks(&self, task_ids: Vec<TaskId>) {
        let mut queue = self.task_queue.lock();
        for task_id in task_ids {
            queue.push_back(task_id);
        }

        if self.config.wakeup_enabled {
            let mut wakeup = self.wakeup_flag.lock();
            *wakeup = true;
        }
    }

    /// Check if wakeup flag is set and clear it.
    pub fn check_and_clear_wakeup(&self) -> bool {
        let mut wakeup = self.wakeup_flag.lock();
        let result = *wakeup;
        *wakeup = false;
        result
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

impl Scheduling for EventDrivenScheduling {
    fn next_task(&mut self, agent_id: AgentId) -> Result<Option<TaskId>> {
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
        // Event-driven: no polling, return minimal duration
        Duration::from_millis(0)
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
    fn test_event_driven_basic() {
        let config = EventDrivenConfig::default();
        let mut scheduler = EventDrivenScheduling::new(config);

        // Add tasks
        scheduler.add_task(TaskId("task1".to_string()));
        scheduler.add_task(TaskId("task2".to_string()));

        assert_eq!(scheduler.queue_length(), 2);
        assert!(!scheduler.should_terminate());

        // Request task
        let agent_id = AgentId::Queen(QueenId("Q0".to_string()));
        let task1 = scheduler.next_task(agent_id.clone()).unwrap();
        assert_eq!(task1, Some(TaskId("task1".to_string())));
        assert_eq!(scheduler.queue_length(), 1);
        assert_eq!(scheduler.active_assignments(), 1);

        // Complete task
        scheduler
            .notify_completion(TaskId("task1".to_string()), agent_id)
            .unwrap();
        assert_eq!(scheduler.active_assignments(), 0);
        assert_eq!(scheduler.completed_count(), 1);
    }

    #[test]
    fn test_event_driven_wakeup() {
        let config = EventDrivenConfig {
            wakeup_enabled: true,
        };
        let scheduler = EventDrivenScheduling::new(config);

        scheduler.add_task(TaskId("task1".to_string()));
        assert!(scheduler.check_and_clear_wakeup());
        assert!(!scheduler.check_and_clear_wakeup());
    }

    #[test]
    fn test_event_driven_termination() {
        let config = EventDrivenConfig::default();
        let mut scheduler = EventDrivenScheduling::new(config);

        scheduler.add_task(TaskId("task1".to_string()));
        assert!(!scheduler.should_terminate());

        let agent_id = AgentId::Queen(QueenId("Q0".to_string()));
        let task = scheduler.next_task(agent_id.clone()).unwrap().unwrap();
        assert!(!scheduler.should_terminate()); // Still has active assignment

        scheduler.notify_completion(task, agent_id).unwrap();
        assert!(scheduler.should_terminate()); // No queue, no assignments
    }
}
