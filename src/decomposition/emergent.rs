use crate::core::types::{Task, TaskId, TaskStatus};
use crate::decomposition::Decomposition;
use anyhow::{anyhow, Result};
use chrono::Utc;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmergentStrategy {
    /// Cast wide net, explore many possibilities
    Explore,
    /// Deepen existing work, focus on completion
    Exploit,
    /// Balance between exploration and exploitation
    Balanced,
}

impl EmergentStrategy {
    /// Select strategy based on completion ratio
    pub fn auto_select(completed: usize, total: usize) -> Self {
        if total == 0 {
            return Self::Explore;
        }

        let ratio = completed as f64 / total as f64;

        if ratio < 0.3 {
            Self::Explore
        } else if ratio < 0.7 {
            Self::Balanced
        } else {
            Self::Exploit
        }
    }

    /// Get exploration weight (0.0 = pure exploitation, 1.0 = pure exploration)
    pub fn exploration_weight(&self) -> f64 {
        match self {
            Self::Explore => 0.8,
            Self::Balanced => 0.5,
            Self::Exploit => 0.2,
        }
    }
}

#[derive(Debug, Clone)]
pub struct EmergentConfig {
    pub max_tasks: usize,
    pub cost_budget_usd: f64,
    pub time_limit: Duration,
}

impl Default for EmergentConfig {
    fn default() -> Self {
        Self {
            max_tasks: 50,
            cost_budget_usd: 5.0,
            time_limit: Duration::from_secs(3600), // 1 hour
        }
    }
}

pub struct EmergentDecomposition {
    config: EmergentConfig,
    task_registry: HashMap<TaskId, Task>,
    parent_map: HashMap<TaskId, TaskId>,
    completed_tasks: HashMap<TaskId, bool>,
    accumulated_cost: f64,
    start_time: Instant,
    strategy: EmergentStrategy,
}

impl EmergentDecomposition {
    pub fn new(config: EmergentConfig) -> Self {
        Self {
            config,
            task_registry: HashMap::new(),
            parent_map: HashMap::new(),
            completed_tasks: HashMap::new(),
            accumulated_cost: 0.0,
            start_time: Instant::now(),
            strategy: EmergentStrategy::Explore,
        }
    }

    /// Check if budget is exceeded
    fn check_budget(&self) -> Result<()> {
        if self.accumulated_cost >= self.config.cost_budget_usd {
            return Err(anyhow!(
                "Budget exceeded: ${:.2} / ${:.2}",
                self.accumulated_cost,
                self.config.cost_budget_usd
            ));
        }
        Ok(())
    }

    /// Check if time limit is exceeded
    fn check_time(&self) -> Result<()> {
        let elapsed = self.start_time.elapsed();
        if elapsed >= self.config.time_limit {
            return Err(anyhow!(
                "Time limit exceeded: {:?} / {:?}",
                elapsed,
                self.config.time_limit
            ));
        }
        Ok(())
    }

    /// Check if task count is exceeded
    fn check_task_count(&self) -> Result<()> {
        if self.task_registry.len() >= self.config.max_tasks {
            return Err(anyhow!(
                "Task limit exceeded: {} / {}",
                self.task_registry.len(),
                self.config.max_tasks
            ));
        }
        Ok(())
    }

    /// Run all guard checks
    fn check_guards(&self) -> Result<()> {
        self.check_budget()?;
        self.check_time()?;
        self.check_task_count()?;
        Ok(())
    }

    /// Add cost from task execution
    pub fn add_cost(&mut self, cost: f64) {
        self.accumulated_cost += cost;
    }

    /// Get current accumulated cost
    pub fn current_cost(&self) -> f64 {
        self.accumulated_cost
    }

    /// Get remaining budget
    pub fn remaining_budget(&self) -> f64 {
        (self.config.cost_budget_usd - self.accumulated_cost).max(0.0)
    }

    /// Get remaining time
    pub fn remaining_time(&self) -> Duration {
        self.config
            .time_limit
            .saturating_sub(self.start_time.elapsed())
    }

    /// Get remaining task capacity
    pub fn remaining_capacity(&self) -> usize {
        self.config.max_tasks.saturating_sub(self.task_registry.len())
    }

    /// Update strategy based on completion ratio
    pub fn update_strategy(&mut self) {
        let total = self.task_registry.len();
        let completed = self.completed_tasks.values().filter(|&&v| v).count();
        self.strategy = EmergentStrategy::auto_select(completed, total);
    }

    /// Get current strategy
    pub fn get_strategy(&self) -> EmergentStrategy {
        self.strategy
    }

    /// Set strategy explicitly
    pub fn set_strategy(&mut self, strategy: EmergentStrategy) {
        self.strategy = strategy;
    }

    /// Generate emergent subtasks based on task description
    fn generate_emergent_tasks(&self, task: &Task) -> Vec<Task> {
        let description = task.description.to_lowercase();
        let mut subtasks = Vec::new();

        // Analyze task for potential emergent work
        if description.contains("research") {
            subtasks.push(self.create_subtask(
                task,
                "Initial literature review and API documentation scan",
                vec![],
            ));

            if self.strategy.exploration_weight() > 0.5 {
                subtasks.push(self.create_subtask(
                    task,
                    "Explore alternative approaches and similar implementations",
                    vec![],
                ));
            }

            subtasks.push(self.create_subtask(
                task,
                "Deep dive into critical endpoints and authentication flows",
                vec![subtasks[0].id.clone()],
            ));

            if self.strategy != EmergentStrategy::Exploit {
                subtasks.push(self.create_subtask(
                    task,
                    "Investigate edge cases and error handling patterns",
                    vec![subtasks[0].id.clone()],
                ));
            }
        } else if description.contains("implement") || description.contains("code") {
            subtasks.push(self.create_subtask(
                task,
                "Design initial architecture and module boundaries",
                vec![],
            ));

            subtasks.push(self.create_subtask(
                task,
                "Implement core functionality",
                vec![subtasks[0].id.clone()],
            ));

            if self.strategy != EmergentStrategy::Exploit {
                subtasks.push(self.create_subtask(
                    task,
                    "Add comprehensive error handling and validation",
                    vec![subtasks[1].id.clone()],
                ));
            }

            if self.strategy == EmergentStrategy::Explore {
                subtasks.push(self.create_subtask(
                    task,
                    "Implement advanced features and optimizations",
                    vec![subtasks[1].id.clone()],
                ));
            }
        } else if description.contains("test") {
            subtasks.push(self.create_subtask(
                task,
                "Write core unit tests",
                vec![],
            ));

            if self.strategy != EmergentStrategy::Exploit {
                subtasks.push(self.create_subtask(
                    task,
                    "Write integration tests",
                    vec![],
                ));
            }

            if self.strategy == EmergentStrategy::Explore {
                subtasks.push(self.create_subtask(
                    task,
                    "Write property-based tests and fuzz tests",
                    vec![],
                ));
            }

            subtasks.push(self.create_subtask(
                task,
                "Run test suite and analyze coverage",
                vec![subtasks[0].id.clone()],
            ));
        } else {
            // Generic decomposition
            subtasks.push(self.create_subtask(
                task,
                format!("Analyze and plan: {}", task.description),
                vec![],
            ));

            subtasks.push(self.create_subtask(
                task,
                format!("Execute primary work: {}", task.description),
                vec![subtasks[0].id.clone()],
            ));

            if self.strategy != EmergentStrategy::Exploit {
                subtasks.push(self.create_subtask(
                    task,
                    format!("Verify and validate: {}", task.description),
                    vec![subtasks[1].id.clone()],
                ));
            }
        }

        subtasks
    }

    fn create_subtask(&self, parent: &Task, description: impl Into<String>, blocked_by: Vec<TaskId>) -> Task {
        Task {
            id: TaskId(Uuid::new_v4().to_string()),
            description: description.into(),
            status: if blocked_by.is_empty() {
                TaskStatus::Ready
            } else {
                TaskStatus::Blocked
            },
            assigned_to: None,
            priority: parent.priority,
            blocked_by,
            created_at: Utc::now(),
        }
    }

    /// Get task statistics
    pub fn stats(&self) -> EmergentStats {
        let total = self.task_registry.len();
        let completed = self.completed_tasks.values().filter(|&&v| v).count();

        EmergentStats {
            total_tasks: total,
            completed_tasks: completed,
            pending_tasks: total - completed,
            accumulated_cost: self.accumulated_cost,
            remaining_budget: self.remaining_budget(),
            elapsed_time: self.start_time.elapsed(),
            remaining_time: self.remaining_time(),
            current_strategy: self.strategy,
            completion_ratio: if total > 0 {
                completed as f64 / total as f64
            } else {
                0.0
            },
        }
    }

    /// Get all child tasks of a parent
    pub fn get_children(&self, parent_id: &TaskId) -> Vec<TaskId> {
        self.parent_map
            .iter()
            .filter(|(_, p)| *p == parent_id)
            .map(|(child, _)| child.clone())
            .collect()
    }
}

#[derive(Debug, Clone)]
pub struct EmergentStats {
    pub total_tasks: usize,
    pub completed_tasks: usize,
    pub pending_tasks: usize,
    pub accumulated_cost: f64,
    pub remaining_budget: f64,
    pub elapsed_time: Duration,
    pub remaining_time: Duration,
    pub current_strategy: EmergentStrategy,
    pub completion_ratio: f64,
}

impl Decomposition for EmergentDecomposition {
    fn decompose(&mut self, task: &Task) -> Result<Vec<Task>> {
        self.check_guards()?;

        let subtasks = self.generate_emergent_tasks(task);

        // Register tasks
        for subtask in &subtasks {
            self.task_registry.insert(subtask.id.clone(), subtask.clone());
            self.parent_map.insert(subtask.id.clone(), task.id.clone());
            self.completed_tasks.insert(subtask.id.clone(), false);
        }

        // Update strategy based on new state
        self.update_strategy();

        Ok(subtasks)
    }

    fn can_decompose(&self, task: &Task) -> bool {
        if self.should_terminate() {
            return false;
        }

        !task.description.is_empty() && task.description.len() > 15
    }

    fn add_dynamic_task(&mut self, parent_id: TaskId, subtask: Task) -> Result<TaskId> {
        self.check_guards()?;

        let task_id = subtask.id.clone();
        self.task_registry.insert(task_id.clone(), subtask);
        self.parent_map.insert(task_id.clone(), parent_id);
        self.completed_tasks.insert(task_id.clone(), false);

        // Update strategy
        self.update_strategy();

        Ok(task_id)
    }

    fn ready_tasks(&self) -> Vec<TaskId> {
        let mut ready = Vec::new();

        for (task_id, task) in &self.task_registry {
            if *self.completed_tasks.get(task_id).unwrap_or(&false) {
                continue;
            }

            let all_deps_done = task.blocked_by.iter().all(|dep_id| {
                self.completed_tasks
                    .get(dep_id)
                    .copied()
                    .unwrap_or(true)
            });

            if all_deps_done {
                ready.push(task_id.clone());
            }
        }

        ready
    }

    fn mark_complete(&mut self, task_id: TaskId) -> Result<()> {
        if !self.task_registry.contains_key(&task_id) {
            return Err(anyhow!("Task not found: {:?}", task_id));
        }

        self.completed_tasks.insert(task_id, true);

        // Update strategy based on new completion state
        self.update_strategy();

        Ok(())
    }
}

impl EmergentDecomposition {
    pub fn should_terminate(&self) -> bool {
        self.check_guards().is_err()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_task(description: &str) -> Task {
        Task {
            id: TaskId(Uuid::new_v4().to_string()),
            description: description.to_string(),
            status: TaskStatus::Ready,
            assigned_to: None,
            priority: 5,
            blocked_by: Vec::new(),
            created_at: Utc::now(),
        }
    }

    #[test]
    fn test_emergent_strategy_selection() {
        assert_eq!(EmergentStrategy::auto_select(1, 10), EmergentStrategy::Explore);
        assert_eq!(EmergentStrategy::auto_select(5, 10), EmergentStrategy::Balanced);
        assert_eq!(EmergentStrategy::auto_select(8, 10), EmergentStrategy::Exploit);
    }

    #[test]
    fn test_emergent_decomposition() {
        let config = EmergentConfig::default();
        let mut emergent = EmergentDecomposition::new(config);

        let task = create_test_task("Research API endpoints and authentication methods");
        let subtasks = emergent.decompose(&task).unwrap();

        assert!(!subtasks.is_empty());
        assert_eq!(emergent.get_strategy(), EmergentStrategy::Explore);
    }

    #[test]
    fn test_budget_guards() {
        let config = EmergentConfig {
            max_tasks: 100,
            cost_budget_usd: 1.0,
            time_limit: Duration::from_secs(3600),
        };
        let mut emergent = EmergentDecomposition::new(config);

        emergent.add_cost(0.5);
        assert!(!emergent.should_terminate());

        emergent.add_cost(0.6);
        assert!(emergent.should_terminate());
    }

    #[test]
    fn test_task_limit_guards() {
        let config = EmergentConfig {
            max_tasks: 3,
            cost_budget_usd: 100.0,
            time_limit: Duration::from_secs(3600),
        };
        let mut emergent = EmergentDecomposition::new(config);

        let task1 = create_test_task("Task 1");
        let task2 = create_test_task("Task 2");
        let task3 = create_test_task("Task 3");

        let parent_id = TaskId(Uuid::new_v4().to_string());

        assert!(emergent.add_dynamic_task(parent_id.clone(), task1).is_ok());
        assert!(emergent.add_dynamic_task(parent_id.clone(), task2).is_ok());
        assert!(emergent.add_dynamic_task(parent_id.clone(), task3).is_ok());

        let task4 = create_test_task("Task 4");
        assert!(emergent.add_dynamic_task(parent_id, task4).is_err());
    }

    #[test]
    fn test_strategy_updates() {
        let config = EmergentConfig::default();
        let mut emergent = EmergentDecomposition::new(config);

        let task = create_test_task("Research something interesting");
        let subtasks = emergent.decompose(&task).unwrap();

        assert_eq!(emergent.get_strategy(), EmergentStrategy::Explore);

        // Complete most tasks to trigger strategy change
        for subtask in &subtasks[..subtasks.len() - 1] {
            emergent.mark_complete(subtask.id.clone()).unwrap();
        }

        // Strategy should shift towards exploit
        assert_ne!(emergent.get_strategy(), EmergentStrategy::Explore);
    }
}
