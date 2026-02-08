//! Task DAG (Directed Acyclic Graph) for dependency-aware task scheduling.
//!
//! This module provides a dependency-aware task scheduler that automatically handles
//! task ordering based on dependencies, tracks critical paths, and manages task state
//! transitions.
//!
//! The TaskDag uses its own local types (DagTask, DagTaskStatus, etc.) to avoid conflicts
//! with the v2::types module, which serves a different purpose in the architecture.

use std::collections::{HashMap, VecDeque};
use chrono::{DateTime, Utc};
use serde::{Serialize, Deserialize};
use crate::core::types::QueenId;

// ============================================================================
// DAG-specific types (separate from v2::types to avoid conflicts)
// ============================================================================

/// Priority level for tasks in the DAG.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Priority {
    Low = 0,
    Normal = 1,
    High = 2,
    Critical = 3,
}

/// Estimated complexity of a task.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum Complexity {
    Trivial,
    Simple,
    Medium,
    Complex,
    VeryComplex,
}

/// Status of a task within the DAG.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DagTaskStatus {
    /// Task is blocked by dependencies
    Blocked,
    /// Task is ready for assignment
    Ready,
    /// Task has been assigned to a Queen
    Assigned(QueenId),
    /// Task is currently being worked on
    InProgress,
    /// Task implementation complete, awaiting validation
    Validating,
    /// Task completed successfully
    Completed,
    /// Task failed with error details
    Failed { error: String, attempts: usize },
}

/// A task within the dependency graph.
#[derive(Debug, Clone)]
pub struct DagTask {
    /// Unique task identifier (string-based for DAG context)
    pub id: String,
    /// Human-readable description of the task
    pub description: String,
    /// Current status of the task
    pub status: DagTaskStatus,
    /// Which Queen is assigned to this task (if any)
    pub assigned_to: Option<QueenId>,
    /// List of task IDs that must complete before this task can start
    pub blocked_by: Vec<String>,
    /// List of task IDs that are blocked by this task (reverse dependencies)
    pub blocks: Vec<String>,
    /// Priority level for task scheduling
    pub priority: Priority,
    /// Estimated complexity for resource allocation
    pub estimated_complexity: Complexity,
    /// Result of the task (if completed)
    pub result: Option<DagTaskResult>,
    /// When this task was created
    pub created_at: DateTime<Utc>,
    /// When this task started execution
    pub started_at: Option<DateTime<Utc>>,
    /// When this task completed
    pub completed_at: Option<DateTime<Utc>>,
}

/// Result of a completed task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DagTaskResult {
    /// Whether the task succeeded
    pub success: bool,
    /// Output description or error message
    pub output: String,
    /// List of files modified by this task
    pub files_modified: Vec<String>,
}

// ============================================================================
// TaskDag Implementation
// ============================================================================

/// Dependency-aware task scheduler using a directed acyclic graph.
pub struct TaskDag {
    tasks: HashMap<String, DagTask>,
}

impl TaskDag {
    /// Create a new empty task DAG.
    pub fn new() -> Self {
        Self {
            tasks: HashMap::new(),
        }
    }

    /// Add a task to the DAG, automatically populating reverse dependencies.
    ///
    /// When a task is added, this method updates the `blocks` field of all tasks
    /// that this task depends on (listed in `blocked_by`).
    pub fn add_task(&mut self, task: DagTask) {
        // Update reverse dependencies - add this task to the blocks list
        // of all tasks it depends on
        for dep_id in &task.blocked_by {
            if let Some(dep_task) = self.tasks.get_mut(dep_id) {
                if !dep_task.blocks.contains(&task.id) {
                    dep_task.blocks.push(task.id.clone());
                }
            }
        }
        self.tasks.insert(task.id.clone(), task);
    }

    /// Get tasks that are ready for assignment.
    ///
    /// A task is ready if:
    /// - Its status is Ready
    /// - All tasks in its `blocked_by` list have status Completed
    pub fn ready_tasks(&self) -> Vec<&DagTask> {
        self.tasks
            .values()
            .filter(|t| {
                matches!(t.status, DagTaskStatus::Ready)
                    && t.blocked_by.iter().all(|dep_id| {
                        self.tasks
                            .get(dep_id)
                            .map(|dep| matches!(dep.status, DagTaskStatus::Completed))
                            .unwrap_or(false)
                    })
            })
            .collect()
    }

    /// Refresh task readiness, automatically transitioning Blocked tasks to Ready
    /// when their dependencies are met.
    pub fn refresh_readiness(&mut self) {
        // Collect IDs of completed tasks
        let completed_ids: Vec<String> = self
            .tasks
            .values()
            .filter(|t| matches!(t.status, DagTaskStatus::Completed))
            .map(|t| t.id.clone())
            .collect();

        // Check blocked tasks and transition to Ready if deps are met
        for task in self.tasks.values_mut() {
            if matches!(task.status, DagTaskStatus::Blocked) {
                let all_deps_met = task.blocked_by.iter().all(|dep_id| {
                    completed_ids.contains(dep_id)
                });
                if all_deps_met {
                    task.status = DagTaskStatus::Ready;
                }
            }
        }
    }

    /// Assign a task to a Queen.
    ///
    /// Updates the task status to Assigned and sets the assigned_to field.
    pub fn assign(&mut self, task_id: &str, queen_id: QueenId) {
        if let Some(task) = self.tasks.get_mut(task_id) {
            task.status = DagTaskStatus::Assigned(queen_id.clone());
            task.assigned_to = Some(queen_id);
            task.started_at = Some(Utc::now());
        }
    }

    /// Mark a task as completed with its result.
    ///
    /// This also triggers a refresh of readiness to unblock dependent tasks.
    pub fn complete(&mut self, task_id: &str, result: DagTaskResult) {
        if let Some(task) = self.tasks.get_mut(task_id) {
            task.status = DagTaskStatus::Completed;
            task.completed_at = Some(Utc::now());
            task.result = Some(result);
        }
        self.refresh_readiness();
    }

    /// Mark a task as failed with an error message.
    ///
    /// Increments the attempt counter if the task was already in a failed state.
    pub fn fail(&mut self, task_id: &str, error: String) {
        if let Some(task) = self.tasks.get_mut(task_id) {
            let attempts = if let DagTaskStatus::Failed { attempts, .. } = task.status {
                attempts + 1
            } else {
                1
            };
            task.status = DagTaskStatus::Failed { error, attempts };
        }
    }

    /// Compute the critical path through the DAG.
    ///
    /// The critical path is the longest chain of dependencies from start to finish.
    /// Uses iterative topological sort with path length tracking to avoid lifetime issues.
    ///
    /// Returns a vector of task IDs representing the critical path, ordered from
    /// start to end.
    pub fn critical_path(&self) -> Vec<String> {
        // Calculate longest path using topological sort
        let mut in_degree: HashMap<String, usize> = HashMap::new();
        let mut longest_path: HashMap<String, usize> = HashMap::new();
        let mut predecessor: HashMap<String, Option<String>> = HashMap::new();

        // Initialize
        for (id, task) in &self.tasks {
            in_degree.insert(id.clone(), task.blocked_by.len());
            longest_path.insert(id.clone(), 0);
            predecessor.insert(id.clone(), None);
        }

        // Find tasks with no dependencies (start nodes)
        let mut queue: VecDeque<String> = self.tasks
            .iter()
            .filter(|(_, task)| task.blocked_by.is_empty())
            .map(|(id, _)| id.clone())
            .collect();

        // Process topologically
        while let Some(task_id) = queue.pop_front() {
            if let Some(task) = self.tasks.get(&task_id) {
                let current_length = longest_path.get(&task_id).copied().unwrap_or(0);

                // Update successors (tasks that depend on this task)
                for successor_id in &task.blocks {
                    let new_length = current_length + 1;
                    let current_successor_length = longest_path.get(successor_id).copied().unwrap_or(0);

                    if new_length > current_successor_length {
                        longest_path.insert(successor_id.clone(), new_length);
                        predecessor.insert(successor_id.clone(), Some(task_id.clone()));
                    }

                    // Decrease in-degree
                    if let Some(degree) = in_degree.get_mut(successor_id) {
                        *degree -= 1;
                        if *degree == 0 {
                            queue.push_back(successor_id.clone());
                        }
                    }
                }
            }
        }

        // Backtrack from the task with the longest path
        let mut path = Vec::new();
        if let Some((end_id, _)) = longest_path.iter().max_by_key(|(_, &len)| len) {
            let mut current = Some(end_id.clone());
            while let Some(id) = current {
                path.push(id.clone());
                current = predecessor.get(&id).and_then(|p| p.clone());
            }
            path.reverse();
        }

        path
    }

    /// Get a task by ID.
    pub fn get(&self, task_id: &str) -> Option<&DagTask> {
        self.tasks.get(task_id)
    }

    /// Get all tasks in the DAG.
    pub fn all_tasks(&self) -> Vec<&DagTask> {
        self.tasks.values().collect()
    }

    /// Get statistics about task status distribution.
    pub fn stats(&self) -> DagStats {
        let mut stats = DagStats {
            total: self.tasks.len(),
            blocked: 0,
            ready: 0,
            in_progress: 0,
            completed: 0,
            failed: 0,
        };

        for task in self.tasks.values() {
            match &task.status {
                DagTaskStatus::Blocked => stats.blocked += 1,
                DagTaskStatus::Ready => stats.ready += 1,
                DagTaskStatus::Assigned(_) | DagTaskStatus::InProgress | DagTaskStatus::Validating => {
                    stats.in_progress += 1
                }
                DagTaskStatus::Completed => stats.completed += 1,
                DagTaskStatus::Failed { .. } => stats.failed += 1,
            }
        }

        stats
    }
}

impl Default for TaskDag {
    fn default() -> Self {
        Self::new()
    }
}

/// Statistics about the current state of the task DAG.
#[derive(Debug, Clone)]
pub struct DagStats {
    /// Total number of tasks
    pub total: usize,
    /// Number of blocked tasks
    pub blocked: usize,
    /// Number of ready tasks
    pub ready: usize,
    /// Number of in-progress tasks (Assigned, InProgress, Validating)
    pub in_progress: usize,
    /// Number of completed tasks
    pub completed: usize,
    /// Number of failed tasks
    pub failed: usize,
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn make_task(id: &str, blocked_by: Vec<&str>) -> DagTask {
        DagTask {
            id: id.to_string(),
            description: format!("Task {}", id),
            status: if blocked_by.is_empty() {
                DagTaskStatus::Ready
            } else {
                DagTaskStatus::Blocked
            },
            assigned_to: None,
            blocked_by: blocked_by.into_iter().map(String::from).collect(),
            blocks: Vec::new(),
            priority: Priority::Normal,
            estimated_complexity: Complexity::Medium,
            result: None,
            created_at: Utc::now(),
            started_at: None,
            completed_at: None,
        }
    }

    #[test]
    fn test_add_task() {
        let mut dag = TaskDag::new();
        let task_a = make_task("A", vec![]);
        dag.add_task(task_a);

        assert_eq!(dag.tasks.len(), 1);
        assert!(dag.tasks.contains_key("A"));
    }

    #[test]
    fn test_ready_tasks_no_deps() {
        let mut dag = TaskDag::new();
        dag.add_task(make_task("A", vec![]));
        dag.add_task(make_task("B", vec![]));

        let ready = dag.ready_tasks();
        assert_eq!(ready.len(), 2);
    }

    #[test]
    fn test_blocked_then_ready() {
        let mut dag = TaskDag::new();

        // Add task A (no dependencies)
        dag.add_task(make_task("A", vec![]));

        // Add task B (depends on A)
        dag.add_task(make_task("B", vec!["A"]));

        // Initially, only A should be ready
        let ready = dag.ready_tasks();
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].id, "A");

        // Complete task A
        dag.complete("A", DagTaskResult {
            success: true,
            output: "Done".to_string(),
            files_modified: vec![],
        });

        // Now B should be ready
        let ready = dag.ready_tasks();
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].id, "B");
    }

    #[test]
    fn test_assign_and_complete() {
        let mut dag = TaskDag::new();
        dag.add_task(make_task("A", vec![]));

        let queen_id = QueenId("Q1".to_string());
        dag.assign("A", queen_id.clone());

        let task = dag.get("A").unwrap();
        assert_eq!(task.assigned_to, Some(queen_id.clone()));
        assert!(matches!(task.status, DagTaskStatus::Assigned(_)));

        dag.complete("A", DagTaskResult {
            success: true,
            output: "Success".to_string(),
            files_modified: vec!["file1.rs".to_string()],
        });

        let task = dag.get("A").unwrap();
        assert!(matches!(task.status, DagTaskStatus::Completed));
        assert!(task.result.is_some());
        assert!(task.completed_at.is_some());
    }

    #[test]
    fn test_fail_task() {
        let mut dag = TaskDag::new();
        dag.add_task(make_task("A", vec![]));

        dag.fail("A", "Something went wrong".to_string());

        let task = dag.get("A").unwrap();
        match &task.status {
            DagTaskStatus::Failed { error, attempts } => {
                assert_eq!(error, "Something went wrong");
                assert_eq!(*attempts, 1);
            }
            _ => panic!("Expected Failed status"),
        }

        // Fail again to test attempt counter
        dag.fail("A", "Still broken".to_string());
        let task = dag.get("A").unwrap();
        match &task.status {
            DagTaskStatus::Failed { error, attempts } => {
                assert_eq!(error, "Still broken");
                assert_eq!(*attempts, 2);
            }
            _ => panic!("Expected Failed status"),
        }
    }

    #[test]
    fn test_critical_path() {
        let mut dag = TaskDag::new();

        // Create a simple chain: A -> B -> C
        dag.add_task(make_task("A", vec![]));
        dag.add_task(make_task("B", vec!["A"]));
        dag.add_task(make_task("C", vec!["B"]));

        let path = dag.critical_path();
        assert_eq!(path, vec!["A", "B", "C"]);
    }

    #[test]
    fn test_critical_path_complex() {
        let mut dag = TaskDag::new();

        // Create a diamond pattern:
        //     A
        //    / \
        //   B   C
        //    \ /
        //     D
        dag.add_task(make_task("A", vec![]));
        dag.add_task(make_task("B", vec!["A"]));
        dag.add_task(make_task("C", vec!["A"]));
        dag.add_task(make_task("D", vec!["B", "C"]));

        let path = dag.critical_path();
        // Should be A -> (B or C) -> D
        assert_eq!(path.len(), 3);
        assert_eq!(path[0], "A");
        assert_eq!(path[2], "D");
        assert!(path[1] == "B" || path[1] == "C");
    }

    #[test]
    fn test_stats() {
        let mut dag = TaskDag::new();

        dag.add_task(make_task("A", vec![]));  // Ready
        dag.add_task(make_task("B", vec!["A"]));  // Blocked
        dag.add_task(make_task("C", vec![]));  // Ready

        // Complete one task
        dag.complete("A", DagTaskResult {
            success: true,
            output: "Done".to_string(),
            files_modified: vec![],
        });

        // Fail one task
        dag.fail("C", "Error".to_string());

        let stats = dag.stats();
        assert_eq!(stats.total, 3);
        assert_eq!(stats.completed, 1);
        assert_eq!(stats.failed, 1);
        assert_eq!(stats.ready, 1);  // B became ready after A completed
    }

    #[test]
    fn test_reverse_dependencies() {
        let mut dag = TaskDag::new();

        dag.add_task(make_task("A", vec![]));
        dag.add_task(make_task("B", vec!["A"]));
        dag.add_task(make_task("C", vec!["A"]));

        // A should have B and C in its blocks list
        let task_a = dag.get("A").unwrap();
        assert_eq!(task_a.blocks.len(), 2);
        assert!(task_a.blocks.contains(&"B".to_string()));
        assert!(task_a.blocks.contains(&"C".to_string()));
    }

    #[test]
    fn test_all_tasks() {
        let mut dag = TaskDag::new();
        dag.add_task(make_task("A", vec![]));
        dag.add_task(make_task("B", vec![]));
        dag.add_task(make_task("C", vec![]));

        let all = dag.all_tasks();
        assert_eq!(all.len(), 3);
    }
}
