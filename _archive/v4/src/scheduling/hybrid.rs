//! Hybrid scheduling: combines event-driven + timer-based.

use super::{EventDrivenConfig, EventDrivenScheduling, Scheduling, TimerBasedConfig, TimerBasedScheduling};
use crate::core::types::{AgentId, TaskId};
use anyhow::Result;
use std::time::Duration;

/// Configuration for hybrid scheduling.
#[derive(Debug, Clone)]
pub struct HybridSchedulingConfig {
    /// Configuration for event-driven component
    pub event_config: EventDrivenConfig,
    /// Configuration for timer-based component
    pub timer_config: TimerBasedConfig,
}

impl Default for HybridSchedulingConfig {
    fn default() -> Self {
        HybridSchedulingConfig {
            event_config: EventDrivenConfig::default(),
            timer_config: TimerBasedConfig::default(),
        }
    }
}

/// Hybrid scheduler combining event-driven and timer-based strategies.
///
/// Events for immediate tasks, timer for periodic checks.
pub struct HybridScheduling {
    /// Event-driven scheduler for immediate tasks
    event_scheduler: EventDrivenScheduling,
    /// Timer-based scheduler for periodic tasks
    timer_scheduler: TimerBasedScheduling,
    /// Configuration
    config: HybridSchedulingConfig,
}

impl HybridScheduling {
    /// Create a new hybrid scheduler.
    pub fn new(config: HybridSchedulingConfig) -> Self {
        let event_scheduler = EventDrivenScheduling::new(config.event_config.clone());
        let timer_scheduler = TimerBasedScheduling::new(config.timer_config.clone());

        HybridScheduling {
            event_scheduler,
            timer_scheduler,
            config,
        }
    }

    /// Add a task to the immediate (event-driven) queue.
    ///
    /// High-priority tasks that should be dispatched immediately.
    pub fn add_task_immediate(&self, task_id: TaskId) {
        self.event_scheduler.add_task(task_id);
    }

    /// Add multiple tasks to the immediate queue.
    pub fn add_tasks_immediate(&self, task_ids: Vec<TaskId>) {
        self.event_scheduler.add_tasks(task_ids);
    }

    /// Add a task to the periodic (timer-based) queue.
    ///
    /// Normal-priority tasks dispatched on timer intervals.
    pub fn add_task_periodic(&self, task_id: TaskId) {
        self.timer_scheduler.add_task(task_id);
    }

    /// Add multiple tasks to the periodic queue.
    pub fn add_tasks_periodic(&self, task_ids: Vec<TaskId>) {
        self.timer_scheduler.add_tasks(task_ids);
    }

    /// Get the number of immediate tasks in queue.
    pub fn immediate_queue_length(&self) -> usize {
        self.event_scheduler.queue_length()
    }

    /// Get the number of periodic tasks in queue.
    pub fn periodic_queue_length(&self) -> usize {
        self.timer_scheduler.queue_length()
    }

    /// Get total queue length.
    pub fn total_queue_length(&self) -> usize {
        self.immediate_queue_length() + self.periodic_queue_length()
    }

    /// Get the number of active assignments across both schedulers.
    pub fn active_assignments(&self) -> usize {
        self.event_scheduler.active_assignments() + self.timer_scheduler.active_assignments()
    }

    /// Get the total number of completed tasks.
    pub fn completed_count(&self) -> usize {
        self.event_scheduler.completed_count() + self.timer_scheduler.completed_count()
    }

    /// Check if wakeup flag is set for event-driven scheduler.
    pub fn check_and_clear_wakeup(&self) -> bool {
        self.event_scheduler.check_and_clear_wakeup()
    }

    /// Get the timer-based tick count.
    pub fn tick_count(&self) -> u64 {
        self.timer_scheduler.tick_count()
    }
}

impl Scheduling for HybridScheduling {
    fn next_task(&mut self, agent_id: AgentId) -> Result<Option<TaskId>> {
        // First check event-driven queue (high priority)
        if let Some(task_id) = self.event_scheduler.next_task(agent_id.clone())? {
            return Ok(Some(task_id));
        }

        // Then check timer-based queue (normal priority)
        if let Some(task_id) = self.timer_scheduler.next_task(agent_id)? {
            return Ok(Some(task_id));
        }

        Ok(None)
    }

    fn notify_completion(&mut self, task_id: TaskId, agent_id: AgentId) -> Result<()> {
        // Notify both schedulers (only the one with the assignment will match)
        // We ignore errors here since one scheduler won't have the assignment
        let _ = self.event_scheduler.notify_completion(task_id.clone(), agent_id.clone());
        let _ = self.timer_scheduler.notify_completion(task_id, agent_id);

        Ok(())
    }

    fn next_interval(&self) -> Duration {
        // Return the minimum interval between event and timer schedulers
        let event_interval = self.event_scheduler.next_interval();
        let timer_interval = self.timer_scheduler.next_interval();

        std::cmp::min(event_interval, timer_interval)
    }

    fn should_terminate(&self) -> bool {
        // Terminate only when both schedulers agree
        self.event_scheduler.should_terminate() && self.timer_scheduler.should_terminate()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::types::QueenId;

    #[test]
    fn test_hybrid_immediate_tasks() {
        let config = HybridSchedulingConfig::default();
        let mut scheduler = HybridScheduling::new(config);

        scheduler.add_task_immediate(TaskId("immediate1".to_string()));
        scheduler.add_task_immediate(TaskId("immediate2".to_string()));

        assert_eq!(scheduler.immediate_queue_length(), 2);
        assert_eq!(scheduler.periodic_queue_length(), 0);

        let agent_id = AgentId::Queen(QueenId("Q0".to_string()));
        let task = scheduler.next_task(agent_id).unwrap();
        assert_eq!(task, Some(TaskId("immediate1".to_string())));
    }

    #[test]
    fn test_hybrid_periodic_tasks() {
        let config = HybridSchedulingConfig::default();
        let mut scheduler = HybridScheduling::new(config);

        scheduler.add_task_periodic(TaskId("periodic1".to_string()));
        scheduler.add_task_periodic(TaskId("periodic2".to_string()));

        assert_eq!(scheduler.immediate_queue_length(), 0);
        assert_eq!(scheduler.periodic_queue_length(), 2);

        let agent_id = AgentId::Queen(QueenId("Q0".to_string()));
        let task = scheduler.next_task(agent_id).unwrap();
        assert_eq!(task, Some(TaskId("periodic1".to_string())));
    }

    #[test]
    fn test_hybrid_priority_order() {
        let config = HybridSchedulingConfig::default();
        let mut scheduler = HybridScheduling::new(config);

        // Add both immediate and periodic tasks
        scheduler.add_task_periodic(TaskId("periodic1".to_string()));
        scheduler.add_task_immediate(TaskId("immediate1".to_string()));
        scheduler.add_task_periodic(TaskId("periodic2".to_string()));

        let agent_id = AgentId::Queen(QueenId("Q0".to_string()));

        // Should get immediate task first
        let task1 = scheduler.next_task(agent_id.clone()).unwrap();
        assert_eq!(task1, Some(TaskId("immediate1".to_string())));

        // Then periodic tasks
        let task2 = scheduler.next_task(agent_id.clone()).unwrap();
        assert_eq!(task2, Some(TaskId("periodic1".to_string())));

        let task3 = scheduler.next_task(agent_id).unwrap();
        assert_eq!(task3, Some(TaskId("periodic2".to_string())));
    }

    #[test]
    fn test_hybrid_termination() {
        let config = HybridSchedulingConfig::default();
        let mut scheduler = HybridScheduling::new(config);

        scheduler.add_task_immediate(TaskId("task1".to_string()));
        scheduler.add_task_periodic(TaskId("task2".to_string()));

        assert!(!scheduler.should_terminate());

        let agent_id = AgentId::Queen(QueenId("Q0".to_string()));

        // Process immediate task
        let task1 = scheduler.next_task(agent_id.clone()).unwrap().unwrap();
        scheduler.notify_completion(task1, agent_id.clone()).unwrap();
        assert!(!scheduler.should_terminate()); // Still has periodic task

        // Process periodic task
        let task2 = scheduler.next_task(agent_id.clone()).unwrap().unwrap();
        scheduler.notify_completion(task2, agent_id).unwrap();
        assert!(scheduler.should_terminate()); // Both queues empty
    }

    #[test]
    fn test_hybrid_total_queue_length() {
        let config = HybridSchedulingConfig::default();
        let scheduler = HybridScheduling::new(config);

        scheduler.add_task_immediate(TaskId("task1".to_string()));
        scheduler.add_task_immediate(TaskId("task2".to_string()));
        scheduler.add_task_periodic(TaskId("task3".to_string()));

        assert_eq!(scheduler.total_queue_length(), 3);
        assert_eq!(scheduler.immediate_queue_length(), 2);
        assert_eq!(scheduler.periodic_queue_length(), 1);
    }
}
