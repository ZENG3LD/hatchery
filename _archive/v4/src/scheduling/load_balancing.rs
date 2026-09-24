//! Load balancing scheduler: distributes tasks evenly across agents.

use super::Scheduling;
use crate::core::types::{AgentId, TaskId};
use anyhow::Result;
use parking_lot::Mutex;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Load balancing strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadStrategy {
    /// Cycle through agents in order
    RoundRobin,
    /// Pick agent with fewest active tasks
    LeastLoaded,
    /// Probability inversely proportional to load
    WeightedRandom,
    /// Sample 2 random agents, pick less loaded (power of two choices)
    PowerOfTwo,
}

/// Configuration for load balancing scheduler.
#[derive(Debug, Clone)]
pub struct LoadBalancingConfig {
    /// Load balancing strategy
    pub strategy: LoadStrategy,
    /// Maximum tasks per agent
    pub max_tasks_per_agent: usize,
}

impl Default for LoadBalancingConfig {
    fn default() -> Self {
        LoadBalancingConfig {
            strategy: LoadStrategy::LeastLoaded,
            max_tasks_per_agent: 10,
        }
    }
}

/// Agent load tracking.
#[derive(Debug, Clone)]
pub struct AgentLoad {
    /// Agent key (string representation)
    pub agent_key: String,
    /// Number of active tasks
    pub active_tasks: usize,
    /// Total completed tasks
    pub total_completed: usize,
    /// Average completion time
    pub avg_completion_time: Duration,
    /// Last assignment time
    pub last_assigned: Option<Instant>,
}

impl AgentLoad {
    fn new(agent_key: String) -> Self {
        AgentLoad {
            agent_key,
            active_tasks: 0,
            total_completed: 0,
            avg_completion_time: Duration::from_secs(0),
            last_assigned: None,
        }
    }

    fn load_factor(&self) -> f64 {
        self.active_tasks as f64
    }
}

/// Load balancing scheduler.
pub struct LoadBalancingScheduling {
    /// Task queue (pending tasks)
    task_queue: Arc<Mutex<VecDeque<TaskId>>>,
    /// Agent loads: agent_key -> AgentLoad
    agent_loads: Arc<Mutex<HashMap<String, AgentLoad>>>,
    /// Task assignments: task_id -> agent_key
    task_assignments: Arc<Mutex<HashMap<String, String>>>,
    /// Task start times: task_id -> start_time
    task_start_times: Arc<Mutex<HashMap<String, Instant>>>,
    /// Configuration
    config: LoadBalancingConfig,
    /// Round-robin counter
    round_robin_counter: Arc<Mutex<usize>>,
}

impl LoadBalancingScheduling {
    /// Create a new load balancing scheduler.
    pub fn new(config: LoadBalancingConfig) -> Self {
        LoadBalancingScheduling {
            task_queue: Arc::new(Mutex::new(VecDeque::new())),
            agent_loads: Arc::new(Mutex::new(HashMap::new())),
            task_assignments: Arc::new(Mutex::new(HashMap::new())),
            task_start_times: Arc::new(Mutex::new(HashMap::new())),
            config,
            round_robin_counter: Arc::new(Mutex::new(0)),
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

    /// Get the number of tasks in the queue.
    pub fn queue_length(&self) -> usize {
        self.task_queue.lock().len()
    }

    /// Get the number of active assignments.
    pub fn active_assignments(&self) -> usize {
        self.task_assignments.lock().len()
    }

    /// Get agent utilization (load factor per agent).
    pub fn agent_utilization(&self) -> HashMap<String, f64> {
        let loads = self.agent_loads.lock();
        loads
            .iter()
            .map(|(key, load)| (key.clone(), load.load_factor()))
            .collect()
    }

    /// Rebalance tasks if load is uneven.
    pub fn rebalance(&self) -> usize {
        let loads = self.agent_loads.lock();

        if loads.is_empty() {
            return 0;
        }

        // Calculate average load
        let total_load: usize = loads.values().map(|l| l.active_tasks).sum();
        let avg_load = total_load as f64 / loads.len() as f64;

        // Count agents above average
        loads.values().filter(|l| l.active_tasks as f64 > avg_load * 1.5).count()
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

    /// Ensure agent exists in load tracking.
    fn ensure_agent(&self, agent_key: String) {
        let mut loads = self.agent_loads.lock();
        loads.entry(agent_key.clone()).or_insert_with(|| AgentLoad::new(agent_key));
    }

    /// Select agent using round-robin strategy.
    fn select_round_robin(&self, agent_key: &str) -> bool {
        let loads = self.agent_loads.lock();
        let agent_keys: Vec<String> = loads.keys().cloned().collect();

        if agent_keys.is_empty() {
            return true; // No agents tracked yet
        }

        let mut counter = self.round_robin_counter.lock();
        let selected_idx = *counter % agent_keys.len();
        *counter += 1;

        agent_keys.get(selected_idx).map(|k| k == agent_key).unwrap_or(true)
    }

    /// Select agent using least-loaded strategy.
    fn select_least_loaded(&self, agent_key: &str) -> bool {
        let loads = self.agent_loads.lock();

        if loads.is_empty() {
            return true;
        }

        // Find agent with minimum active tasks
        let min_load = loads.values().map(|l| l.active_tasks).min().unwrap_or(0);

        loads
            .get(agent_key)
            .map(|l| l.active_tasks <= min_load)
            .unwrap_or(true)
    }

    /// Select agent using weighted random strategy.
    fn select_weighted_random(&self, agent_key: &str) -> bool {
        let loads = self.agent_loads.lock();

        if loads.is_empty() {
            return true;
        }

        // Simple implementation: favor agents with fewer active tasks
        let agent_load = loads.get(agent_key).map(|l| l.active_tasks).unwrap_or(0);
        let avg_load: f64 = loads.values().map(|l| l.active_tasks).sum::<usize>() as f64
            / loads.len() as f64;

        // More likely to select if below average
        agent_load as f64 <= avg_load
    }

    /// Select agent using power-of-two strategy.
    fn select_power_of_two(&self, agent_key: &str) -> bool {
        let loads = self.agent_loads.lock();

        if loads.is_empty() {
            return true;
        }

        // Get two random agents and pick the less loaded
        let agent_keys: Vec<&String> = loads.keys().collect();

        if agent_keys.len() < 2 {
            return true;
        }

        // Simple implementation: check if current agent is in bottom half of load
        let mut load_values: Vec<usize> = loads.values().map(|l| l.active_tasks).collect();
        load_values.sort();

        let median_idx = load_values.len() / 2;
        let median_load = load_values.get(median_idx).copied().unwrap_or(0);

        let agent_load = loads.get(agent_key).map(|l| l.active_tasks).unwrap_or(0);
        agent_load <= median_load
    }

    /// Check if agent can accept more tasks based on strategy.
    fn can_assign(&self, agent_key: &str) -> bool {
        let loads = self.agent_loads.lock();

        // Check max tasks limit
        if let Some(load) = loads.get(agent_key) {
            if load.active_tasks >= self.config.max_tasks_per_agent {
                return false;
            }
        }

        match self.config.strategy {
            LoadStrategy::RoundRobin => self.select_round_robin(agent_key),
            LoadStrategy::LeastLoaded => self.select_least_loaded(agent_key),
            LoadStrategy::WeightedRandom => self.select_weighted_random(agent_key),
            LoadStrategy::PowerOfTwo => self.select_power_of_two(agent_key),
        }
    }
}

impl Scheduling for LoadBalancingScheduling {
    fn next_task(&mut self, agent_id: AgentId) -> Result<Option<TaskId>> {
        let agent_key = Self::agent_key(&agent_id);
        self.ensure_agent(agent_key.clone());

        // Check if agent can accept more tasks
        if !self.can_assign(&agent_key) {
            return Ok(None);
        }

        let mut queue = self.task_queue.lock();

        if let Some(task_id) = queue.pop_front() {
            // Update agent load
            let mut loads = self.agent_loads.lock();
            if let Some(load) = loads.get_mut(&agent_key) {
                load.active_tasks += 1;
                load.last_assigned = Some(Instant::now());
            }

            // Record assignment
            let mut assignments = self.task_assignments.lock();
            assignments.insert(task_id.0.clone(), agent_key.clone());

            // Record start time
            let mut start_times = self.task_start_times.lock();
            start_times.insert(task_id.0.clone(), Instant::now());

            Ok(Some(task_id))
        } else {
            Ok(None)
        }
    }

    fn notify_completion(&mut self, task_id: TaskId, agent_id: AgentId) -> Result<()> {
        let agent_key = Self::agent_key(&agent_id);

        // Remove assignment
        let mut assignments = self.task_assignments.lock();
        assignments.remove(&task_id.0);

        // Calculate completion time
        let mut start_times = self.task_start_times.lock();
        let completion_time = start_times
            .remove(&task_id.0)
            .map(|start| start.elapsed())
            .unwrap_or(Duration::from_secs(0));

        // Update agent load
        let mut loads = self.agent_loads.lock();
        if let Some(load) = loads.get_mut(&agent_key) {
            if load.active_tasks > 0 {
                load.active_tasks -= 1;
            }
            load.total_completed += 1;

            // Update average completion time
            let total_time = load.avg_completion_time.as_secs_f64() * (load.total_completed - 1) as f64
                + completion_time.as_secs_f64();
            load.avg_completion_time = Duration::from_secs_f64(total_time / load.total_completed as f64);
        }

        Ok(())
    }

    fn next_interval(&self) -> Duration {
        Duration::from_millis(100)
    }

    fn should_terminate(&self) -> bool {
        let queue = self.task_queue.lock();
        let assignments = self.task_assignments.lock();

        queue.is_empty() && assignments.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::types::QueenId;

    #[test]
    fn test_load_balancing_basic() {
        let config = LoadBalancingConfig::default();
        let mut scheduler = LoadBalancingScheduling::new(config);

        scheduler.add_task(TaskId("task1".to_string()));
        assert_eq!(scheduler.queue_length(), 1);

        let agent_id = AgentId::Queen(QueenId("Q0".to_string()));
        let task = scheduler.next_task(agent_id.clone()).unwrap();
        assert_eq!(task, Some(TaskId("task1".to_string())));
        assert_eq!(scheduler.active_assignments(), 1);

        scheduler.notify_completion(TaskId("task1".to_string()), agent_id).unwrap();
        assert_eq!(scheduler.active_assignments(), 0);
    }

    #[test]
    fn test_load_balancing_max_tasks() {
        let config = LoadBalancingConfig {
            strategy: LoadStrategy::LeastLoaded,
            max_tasks_per_agent: 2,
        };
        let mut scheduler = LoadBalancingScheduling::new(config);

        scheduler.add_tasks(vec![
            TaskId("task1".to_string()),
            TaskId("task2".to_string()),
            TaskId("task3".to_string()),
        ]);

        let agent_id = AgentId::Queen(QueenId("Q0".to_string()));

        // First two tasks should succeed
        assert!(scheduler.next_task(agent_id.clone()).unwrap().is_some());
        assert!(scheduler.next_task(agent_id.clone()).unwrap().is_some());

        // Third task should fail (max limit reached)
        assert!(scheduler.next_task(agent_id.clone()).unwrap().is_none());
    }

    #[test]
    fn test_agent_utilization() {
        let config = LoadBalancingConfig::default();
        let mut scheduler = LoadBalancingScheduling::new(config);

        scheduler.add_task(TaskId("task1".to_string()));

        let agent_id = AgentId::Queen(QueenId("Q0".to_string()));
        scheduler.next_task(agent_id).unwrap();

        let utilization = scheduler.agent_utilization();
        assert!(utilization.contains_key("queen:Q0"));
    }
}
