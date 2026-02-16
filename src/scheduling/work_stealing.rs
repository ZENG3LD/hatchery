//! Work stealing scheduler: pull-based shared queue with work stealing.

use super::Scheduling;
use crate::core::types::{AgentId, TaskId};
use anyhow::Result;
use parking_lot::Mutex;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Duration;

/// Configuration for work stealing scheduler.
#[derive(Debug, Clone)]
pub struct WorkStealingConfig {
    /// Maximum capacity of queues
    pub queue_capacity: usize,
    /// Number of tasks to steal in one batch
    pub steal_batch_size: usize,
}

impl Default for WorkStealingConfig {
    fn default() -> Self {
        WorkStealingConfig {
            queue_capacity: 1000,
            steal_batch_size: 2,
        }
    }
}

/// Work stealing scheduler with global and local queues.
pub struct WorkStealingScheduling {
    /// Global shared queue
    global_queue: Arc<Mutex<VecDeque<TaskId>>>,
    /// Local per-agent queues: agent_key -> local_queue
    local_queues: Arc<Mutex<HashMap<String, VecDeque<TaskId>>>>,
    /// Active tasks: task_id -> agent_key
    active_tasks: Arc<Mutex<HashMap<String, String>>>,
    /// Completed tasks
    completed_tasks: Arc<Mutex<Vec<TaskId>>>,
    /// Configuration
    config: WorkStealingConfig,
    /// Steal statistics: agent_key -> steal_count
    steal_stats: Arc<Mutex<HashMap<String, usize>>>,
}

impl WorkStealingScheduling {
    /// Create a new work stealing scheduler.
    pub fn new(config: WorkStealingConfig) -> Self {
        WorkStealingScheduling {
            global_queue: Arc::new(Mutex::new(VecDeque::new())),
            local_queues: Arc::new(Mutex::new(HashMap::new())),
            active_tasks: Arc::new(Mutex::new(HashMap::new())),
            completed_tasks: Arc::new(Mutex::new(Vec::new())),
            config,
            steal_stats: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Enqueue a task to the global queue.
    pub fn enqueue(&self, task_id: TaskId) {
        let mut queue = self.global_queue.lock();

        if queue.len() < self.config.queue_capacity {
            queue.push_back(task_id);
        }
    }

    /// Enqueue multiple tasks to the global queue.
    pub fn enqueue_batch(&self, task_ids: Vec<TaskId>) {
        let mut queue = self.global_queue.lock();

        for task_id in task_ids {
            if queue.len() < self.config.queue_capacity {
                queue.push_back(task_id);
            } else {
                break;
            }
        }
    }

    /// Get the global queue length.
    pub fn global_queue_length(&self) -> usize {
        self.global_queue.lock().len()
    }

    /// Get a local queue length for an agent.
    pub fn local_queue_length(&self, agent_key: &str) -> usize {
        let queues = self.local_queues.lock();
        queues.get(agent_key).map(|q| q.len()).unwrap_or(0)
    }

    /// Get total number of tasks across all queues.
    pub fn total_queue_length(&self) -> usize {
        let global_len = self.global_queue.lock().len();
        let local_len: usize = self.local_queues.lock().values().map(|q| q.len()).sum();

        global_len + local_len
    }

    /// Get the number of active tasks.
    pub fn active_count(&self) -> usize {
        self.active_tasks.lock().len()
    }

    /// Get the number of completed tasks.
    pub fn completed_count(&self) -> usize {
        self.completed_tasks.lock().len()
    }

    /// Get steal statistics.
    pub fn steal_statistics(&self) -> HashMap<String, usize> {
        self.steal_stats.lock().clone()
    }

    /// Rebalance tasks across local queues.
    pub fn rebalance(&self) -> usize {
        let queues = self.local_queues.lock();

        if queues.is_empty() {
            return 0;
        }

        // Calculate average queue length
        let total_tasks: usize = queues.values().map(|q| q.len()).sum();
        let avg_len = total_tasks / queues.len();

        // Count imbalanced queues
        let mut imbalanced = 0;

        for (_, queue) in queues.iter() {
            if queue.len() > avg_len * 2 {
                imbalanced += 1;
            }
        }

        imbalanced
    }

    /// Steal tasks from the global queue.
    fn steal_from_global(&self, agent_key: &str) -> Vec<TaskId> {
        let mut global = self.global_queue.lock();
        let mut local_queues = self.local_queues.lock();

        let mut stolen = Vec::new();
        let batch_size = self.config.steal_batch_size;

        for _ in 0..batch_size {
            if let Some(task) = global.pop_front() {
                stolen.push(task);
            } else {
                break;
            }
        }

        // Add to local queue
        if !stolen.is_empty() {
            let local_queue = local_queues
                .entry(agent_key.to_string())
                .or_insert_with(VecDeque::new);

            for task in &stolen {
                local_queue.push_back(task.clone());
            }

            // Update steal stats
            let mut stats = self.steal_stats.lock();
            *stats.entry(agent_key.to_string()).or_insert(0) += stolen.len();
        }

        stolen
    }

    /// Steal tasks from a peer's local queue.
    fn steal_from_peer(&self, agent_key: &str) -> Vec<TaskId> {
        let mut local_queues = self.local_queues.lock();

        // Find the busiest peer (excluding self)
        let mut max_len = 0;
        let mut busiest_peer: Option<String> = None;

        for (peer_key, queue) in local_queues.iter() {
            if peer_key != agent_key && queue.len() > max_len {
                max_len = queue.len();
                busiest_peer = Some(peer_key.clone());
            }
        }

        let mut stolen = Vec::new();

        if let Some(peer_key) = busiest_peer {
            if let Some(peer_queue) = local_queues.get_mut(&peer_key) {
                // Steal half of peer's queue
                let steal_count = peer_queue.len() / 2;

                for _ in 0..steal_count {
                    if let Some(task) = peer_queue.pop_front() {
                        stolen.push(task);
                    }
                }

                // Add to own local queue
                if !stolen.is_empty() {
                    let local_queue = local_queues
                        .entry(agent_key.to_string())
                        .or_insert_with(VecDeque::new);

                    for task in &stolen {
                        local_queue.push_back(task.clone());
                    }

                    // Update steal stats
                    let mut stats = self.steal_stats.lock();
                    *stats.entry(agent_key.to_string()).or_insert(0) += stolen.len();
                }
            }
        }

        stolen
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

    /// Ensure local queue exists for agent.
    fn ensure_local_queue(&self, agent_key: &str) {
        let mut queues = self.local_queues.lock();
        queues.entry(agent_key.to_string()).or_insert_with(VecDeque::new);
    }
}

impl Scheduling for WorkStealingScheduling {
    fn next_task(&mut self, agent_id: AgentId) -> Result<Option<TaskId>> {
        let agent_key = Self::agent_key(&agent_id);
        self.ensure_local_queue(&agent_key);

        // First check local queue
        {
            let mut local_queues = self.local_queues.lock();

            if let Some(local_queue) = local_queues.get_mut(&agent_key) {
                if let Some(task_id) = local_queue.pop_front() {
                    // Mark as active
                    let mut active = self.active_tasks.lock();
                    active.insert(task_id.0.clone(), agent_key.clone());

                    return Ok(Some(task_id));
                }
            }
        }

        // Local queue empty, try stealing from global
        let stolen_global = self.steal_from_global(&agent_key);

        if !stolen_global.is_empty() {
            // Return first stolen task
            let task_id = stolen_global[0].clone();

            // Remove from local queue (we already added it there)
            let mut local_queues = self.local_queues.lock();
            if let Some(local_queue) = local_queues.get_mut(&agent_key) {
                local_queue.pop_front(); // Remove the one we're returning
            }

            // Mark as active
            let mut active = self.active_tasks.lock();
            active.insert(task_id.0.clone(), agent_key.clone());

            return Ok(Some(task_id));
        }

        // Global queue empty, try stealing from peers
        let stolen_peer = self.steal_from_peer(&agent_key);

        if !stolen_peer.is_empty() {
            // Return first stolen task
            let task_id = stolen_peer[0].clone();

            // Remove from local queue
            let mut local_queues = self.local_queues.lock();
            if let Some(local_queue) = local_queues.get_mut(&agent_key) {
                local_queue.pop_front(); // Remove the one we're returning
            }

            // Mark as active
            let mut active = self.active_tasks.lock();
            active.insert(task_id.0.clone(), agent_key.clone());

            return Ok(Some(task_id));
        }

        // No tasks available
        Ok(None)
    }

    fn notify_completion(&mut self, task_id: TaskId, _agent_id: AgentId) -> Result<()> {
        // Remove from active tasks
        let mut active = self.active_tasks.lock();
        active.remove(&task_id.0);

        // Add to completed
        let mut completed = self.completed_tasks.lock();
        completed.push(task_id);

        Ok(())
    }

    fn next_interval(&self) -> Duration {
        Duration::from_millis(50)
    }

    fn should_terminate(&self) -> bool {
        let global_empty = self.global_queue.lock().is_empty();
        let locals_empty = self.local_queues.lock().values().all(|q| q.is_empty());
        let no_active = self.active_tasks.lock().is_empty();

        global_empty && locals_empty && no_active
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::types::QueenId;

    #[test]
    fn test_work_stealing_basic() {
        let config = WorkStealingConfig::default();
        let mut scheduler = WorkStealingScheduling::new(config);

        scheduler.enqueue(TaskId("task1".to_string()));
        scheduler.enqueue(TaskId("task2".to_string()));

        assert_eq!(scheduler.global_queue_length(), 2);

        let agent_id = AgentId::Queen(QueenId("Q0".to_string()));
        let task = scheduler.next_task(agent_id).unwrap();

        assert!(task.is_some());
        assert_eq!(scheduler.active_count(), 1);
    }

    #[test]
    fn test_work_stealing_from_global() {
        let config = WorkStealingConfig {
            steal_batch_size: 3,
            ..Default::default()
        };
        let mut scheduler = WorkStealingScheduling::new(config);

        scheduler.enqueue_batch(vec![
            TaskId("task1".to_string()),
            TaskId("task2".to_string()),
            TaskId("task3".to_string()),
            TaskId("task4".to_string()),
        ]);

        let agent_id = AgentId::Queen(QueenId("Q0".to_string()));

        // Should steal batch from global
        let task1 = scheduler.next_task(agent_id.clone()).unwrap();
        assert!(task1.is_some());

        // Local queue should have remaining stolen tasks
        assert!(scheduler.local_queue_length("queen:Q0") > 0);
    }

    #[test]
    fn test_work_stealing_statistics() {
        let config = WorkStealingConfig::default();
        let mut scheduler = WorkStealingScheduling::new(config);

        scheduler.enqueue(TaskId("task1".to_string()));
        scheduler.enqueue(TaskId("task2".to_string()));

        let agent_id = AgentId::Queen(QueenId("Q0".to_string()));
        scheduler.next_task(agent_id).unwrap();

        let stats = scheduler.steal_statistics();
        assert!(stats.contains_key("queen:Q0"));
    }

    #[test]
    fn test_work_stealing_termination() {
        let config = WorkStealingConfig::default();
        let mut scheduler = WorkStealingScheduling::new(config);

        scheduler.enqueue(TaskId("task1".to_string()));

        let agent_id = AgentId::Queen(QueenId("Q0".to_string()));
        let task = scheduler.next_task(agent_id.clone()).unwrap().unwrap();

        assert!(!scheduler.should_terminate()); // Has active task

        scheduler.notify_completion(task, agent_id).unwrap();
        assert!(scheduler.should_terminate()); // All done
    }
}
