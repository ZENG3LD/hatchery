use crate::core::types::{Task, TaskId};
use crate::decomposition::{dag::DagDecomposition, Decomposition};
use anyhow::{anyhow, Result};
use std::time::Instant;

#[derive(Debug, Clone)]
pub struct TdagConfig {
    pub max_dynamic_tasks: usize,
    pub termination_budget: f64,
}

impl Default for TdagConfig {
    fn default() -> Self {
        Self {
            max_dynamic_tasks: 100,
            termination_budget: 10.0, // USD
        }
    }
}

#[derive(Debug, Clone)]
pub struct DynamicTaskEvent {
    pub parent_id: TaskId,
    pub new_task: Task,
    pub discovered_at: Instant,
}

pub struct TdagDecomposition {
    config: TdagConfig,
    dag: DagDecomposition,
    dynamic_task_count: usize,
    accumulated_cost: f64,
    events: Vec<DynamicTaskEvent>,
    start_time: Instant,
}

impl TdagDecomposition {
    pub fn new(config: TdagConfig) -> Self {
        let dag_config = crate::decomposition::dag::DagDecompositionConfig {
            use_llm: false,
            llm_model: "claude-sonnet-4-5".to_string(),
        };

        Self {
            config,
            dag: DagDecomposition::new(dag_config),
            dynamic_task_count: 0,
            accumulated_cost: 0.0,
            events: Vec::new(),
            start_time: Instant::now(),
        }
    }

    /// Get current accumulated cost
    pub fn current_cost(&self) -> f64 {
        self.accumulated_cost
    }

    /// Add cost from task execution
    pub fn add_cost(&mut self, cost: f64) {
        self.accumulated_cost += cost;
    }

    /// Check if we should terminate based on budget
    pub fn should_terminate(&self) -> bool {
        if self.accumulated_cost >= self.config.termination_budget {
            return true;
        }

        if self.dynamic_task_count >= self.config.max_dynamic_tasks {
            return true;
        }

        false
    }

    /// Get remaining budget
    pub fn remaining_budget(&self) -> f64 {
        (self.config.termination_budget - self.accumulated_cost).max(0.0)
    }

    /// Get remaining task capacity
    pub fn remaining_task_capacity(&self) -> usize {
        self.config.max_dynamic_tasks.saturating_sub(self.dynamic_task_count)
    }

    /// Get all dynamic task events
    pub fn get_events(&self) -> &[DynamicTaskEvent] {
        &self.events
    }

    /// Get task creation rate (tasks per second)
    pub fn task_creation_rate(&self) -> f64 {
        let elapsed = self.start_time.elapsed().as_secs_f64();
        if elapsed > 0.0 {
            self.dynamic_task_count as f64 / elapsed
        } else {
            0.0
        }
    }

    /// Get average cost per task
    pub fn average_cost_per_task(&self) -> f64 {
        if self.dynamic_task_count > 0 {
            self.accumulated_cost / self.dynamic_task_count as f64
        } else {
            0.0
        }
    }

    /// Estimate tasks remaining within budget
    pub fn estimated_tasks_within_budget(&self) -> usize {
        let avg_cost = self.average_cost_per_task();
        if avg_cost > 0.0 {
            (self.remaining_budget() / avg_cost) as usize
        } else {
            self.remaining_task_capacity()
        }
    }

    /// Get statistics about the TDAG
    pub fn stats(&self) -> TdagStats {
        TdagStats {
            total_dynamic_tasks: self.dynamic_task_count,
            accumulated_cost: self.accumulated_cost,
            remaining_budget: self.remaining_budget(),
            remaining_capacity: self.remaining_task_capacity(),
            task_creation_rate: self.task_creation_rate(),
            average_cost_per_task: self.average_cost_per_task(),
            elapsed_time: self.start_time.elapsed(),
        }
    }

    /// Check if a task addition is within limits
    fn check_limits(&self) -> Result<()> {
        if self.dynamic_task_count >= self.config.max_dynamic_tasks {
            return Err(anyhow!(
                "Maximum dynamic task limit reached: {}",
                self.config.max_dynamic_tasks
            ));
        }

        if self.accumulated_cost >= self.config.termination_budget {
            return Err(anyhow!(
                "Budget limit reached: ${:.2} / ${:.2}",
                self.accumulated_cost,
                self.config.termination_budget
            ));
        }

        Ok(())
    }

    /// Get inner DAG for advanced operations
    pub fn dag(&self) -> &DagDecomposition {
        &self.dag
    }

    /// Get mutable inner DAG for advanced operations
    pub fn dag_mut(&mut self) -> &mut DagDecomposition {
        &mut self.dag
    }
}

#[derive(Debug, Clone)]
pub struct TdagStats {
    pub total_dynamic_tasks: usize,
    pub accumulated_cost: f64,
    pub remaining_budget: f64,
    pub remaining_capacity: usize,
    pub task_creation_rate: f64,
    pub average_cost_per_task: f64,
    pub elapsed_time: std::time::Duration,
}

impl Decomposition for TdagDecomposition {
    fn decompose(&mut self, task: &Task) -> Result<Vec<Task>> {
        // Delegate to inner DAG
        let subtasks = self.dag.decompose(task)?;

        // Track initial decomposition
        self.dynamic_task_count += subtasks.len();

        Ok(subtasks)
    }

    fn can_decompose(&self, task: &Task) -> bool {
        if self.should_terminate() {
            return false;
        }

        self.dag.can_decompose(task)
    }

    fn add_dynamic_task(&mut self, parent_id: TaskId, subtask: Task) -> Result<TaskId> {
        // Check limits before adding
        self.check_limits()?;

        // Record event
        let event = DynamicTaskEvent {
            parent_id: parent_id.clone(),
            new_task: subtask.clone(),
            discovered_at: Instant::now(),
        };
        self.events.push(event);

        // Add to inner DAG
        let task_id = self.dag.add_dynamic_task(parent_id, subtask)?;

        // Increment counter
        self.dynamic_task_count += 1;

        Ok(task_id)
    }

    fn ready_tasks(&self) -> Vec<TaskId> {
        self.dag.ready_tasks()
    }

    fn mark_complete(&mut self, task_id: TaskId) -> Result<()> {
        self.dag.mark_complete(task_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::types::{TaskStatus};
    use chrono::Utc;
    use uuid::Uuid;

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
    fn test_tdag_creation() {
        let config = TdagConfig {
            max_dynamic_tasks: 50,
            termination_budget: 5.0,
        };
        let tdag = TdagDecomposition::new(config);

        assert_eq!(tdag.current_cost(), 0.0);
        assert_eq!(tdag.remaining_budget(), 5.0);
        assert!(!tdag.should_terminate());
    }

    #[test]
    fn test_budget_tracking() {
        let config = TdagConfig {
            max_dynamic_tasks: 50,
            termination_budget: 5.0,
        };
        let mut tdag = TdagDecomposition::new(config);

        tdag.add_cost(2.5);
        assert_eq!(tdag.current_cost(), 2.5);
        assert_eq!(tdag.remaining_budget(), 2.5);
        assert!(!tdag.should_terminate());

        tdag.add_cost(2.5);
        assert_eq!(tdag.current_cost(), 5.0);
        assert_eq!(tdag.remaining_budget(), 0.0);
        assert!(tdag.should_terminate());
    }

    #[test]
    fn test_task_limit() {
        let config = TdagConfig {
            max_dynamic_tasks: 5,
            termination_budget: 100.0,
        };
        let mut tdag = TdagDecomposition::new(config);

        let parent = create_test_task("Parent task");
        let parent_id = parent.id.clone();

        // Add tasks up to limit
        for i in 0..5 {
            let task = create_test_task(&format!("Dynamic task {}", i));
            let result = tdag.add_dynamic_task(parent_id.clone(), task);
            assert!(result.is_ok());
        }

        // Should fail when limit reached
        let extra_task = create_test_task("Extra task");
        let result = tdag.add_dynamic_task(parent_id, extra_task);
        assert!(result.is_err());
    }

    #[test]
    fn test_decomposition_with_tdag() {
        let config = TdagConfig::default();
        let mut tdag = TdagDecomposition::new(config);

        let task = create_test_task("Research API endpoints and implement connector");
        let subtasks = tdag.decompose(&task).unwrap();

        assert!(!subtasks.is_empty());
        assert!(tdag.dynamic_task_count > 0);
    }

    #[test]
    fn test_tdag_stats() {
        let config = TdagConfig {
            max_dynamic_tasks: 100,
            termination_budget: 10.0,
        };
        let mut tdag = TdagDecomposition::new(config);

        tdag.add_cost(2.0);
        let parent = create_test_task("Parent");
        let child = create_test_task("Child");
        let _ = tdag.add_dynamic_task(parent.id.clone(), child);

        let stats = tdag.stats();
        assert_eq!(stats.total_dynamic_tasks, 1);
        assert_eq!(stats.accumulated_cost, 2.0);
        assert_eq!(stats.remaining_budget, 8.0);
        assert!(stats.average_cost_per_task > 0.0);
    }
}
