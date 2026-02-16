//! PRD-based task decomposition.
//!
//! Wraps the PRD parser (`crate::prd`) as a `Decomposition` trait implementation,
//! converting `cli::Task` to `core::types::Task` and managing dependency tracking.

use crate::core::types::{Task, TaskId, TaskStatus};
use crate::decomposition::Decomposition;
use anyhow::{Context, Result};
use chrono::Utc;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

/// Configuration for PRD-based decomposition.
#[derive(Debug, Clone)]
pub struct PrdDecompositionConfig {
    /// Path to the PRD markdown file.
    pub prd_path: PathBuf,
    /// Whether to respect existing checkboxes (mark [x] as completed).
    pub respect_existing_checkboxes: bool,
}

impl Default for PrdDecompositionConfig {
    fn default() -> Self {
        Self {
            prd_path: PathBuf::from("PRD.md"),
            respect_existing_checkboxes: true,
        }
    }
}

/// PRD-based task decomposition.
///
/// Parses a markdown PRD file with checkbox tasks and converts them into a
/// dependency-aware task graph compatible with the `Decomposition` trait.
pub struct PrdDecomposition {
    config: PrdDecompositionConfig,
    /// All tasks parsed from PRD, keyed by task ID (e.g., "prd-1").
    tasks: HashMap<String, Task>,
    /// Dependency graph: task_id -> list of task_ids it depends on.
    dependencies: HashMap<String, Vec<String>>,
    /// Track which tasks have been completed.
    completed: HashSet<String>,
    /// Skill hints extracted from task descriptions.
    skill_hints: HashMap<String, String>,
    /// Verification commands extracted from task descriptions.
    verify_cmds: HashMap<String, String>,
}

impl PrdDecomposition {
    /// Create a new PRD decomposition from configuration.
    pub fn new(config: PrdDecompositionConfig) -> Result<Self> {
        let mut decomp = Self {
            config,
            tasks: HashMap::new(),
            dependencies: HashMap::new(),
            completed: HashSet::new(),
            skill_hints: HashMap::new(),
            verify_cmds: HashMap::new(),
        };

        // Parse the PRD file immediately
        decomp.parse_prd()?;

        Ok(decomp)
    }

    /// Create a new PRD decomposition from a file path.
    pub fn from_path(prd_path: PathBuf) -> Result<Self> {
        let config = PrdDecompositionConfig {
            prd_path,
            respect_existing_checkboxes: true,
        };
        Self::new(config)
    }

    /// Parse the PRD file and populate internal state.
    fn parse_prd(&mut self) -> Result<()> {
        // Use the existing PRD parser
        let cli_tasks = crate::prd::parse_prd(&self.config.prd_path)
            .with_context(|| format!("Failed to parse PRD: {}", self.config.prd_path.display()))?;

        // Convert cli::Task to core::types::Task
        for cli_task in cli_tasks {
            let task_id = format!("prd-{}", cli_task.id);

            // Determine initial status
            let status = if self.config.respect_existing_checkboxes && cli_task.done {
                TaskStatus::Completed
            } else if cli_task.dependencies.is_empty() {
                TaskStatus::Ready
            } else {
                TaskStatus::Blocked
            };

            // Convert dependencies from "prd-1" strings to TaskId
            let blocked_by: Vec<TaskId> = cli_task
                .dependencies
                .iter()
                .map(|dep_id| TaskId(dep_id.clone()))
                .collect();

            // Create the core::types::Task
            let task = Task {
                id: TaskId(task_id.clone()),
                description: cli_task.description.clone(),
                status,
                assigned_to: None,
                priority: 100, // Default priority; could be inferred from order
                blocked_by,
                created_at: Utc::now(),
            };

            // Store task
            self.tasks.insert(task_id.clone(), task);

            // Store dependencies for easy lookup
            self.dependencies.insert(
                task_id.clone(),
                cli_task.dependencies.clone(),
            );

            // Store skill hint if present
            if let Some(skill) = cli_task.skill_hint {
                self.skill_hints.insert(task_id.clone(), skill);
            }

            // Store verification command if present
            if let Some(cmd) = cli_task.verify_cmd {
                self.verify_cmds.insert(task_id.clone(), cmd);
            }

            // Track completed tasks
            if self.config.respect_existing_checkboxes && cli_task.done {
                self.completed.insert(task_id.clone());
            }
        }

        Ok(())
    }

    /// Reload the PRD file and update internal state.
    ///
    /// Useful for detecting changes made to the PRD file by external processes.
    pub fn reload(&mut self) -> Result<()> {
        // Clear current state
        self.tasks.clear();
        self.dependencies.clear();
        self.completed.clear();
        self.skill_hints.clear();
        self.verify_cmds.clear();

        // Re-parse
        self.parse_prd()
    }

    /// Get the skill hint for a task, if any.
    pub fn skill_hint(&self, task_id: &TaskId) -> Option<&str> {
        self.skill_hints.get(&task_id.0).map(|s| s.as_str())
    }

    /// Get the verification command for a task, if any.
    pub fn verify_cmd(&self, task_id: &TaskId) -> Option<&str> {
        self.verify_cmds.get(&task_id.0).map(|s| s.as_str())
    }

    /// Get the path to the PRD file.
    pub fn prd_path(&self) -> &PathBuf {
        &self.config.prd_path
    }

    /// Get all tasks (for inspection/debugging).
    pub fn all_tasks(&self) -> Vec<&Task> {
        self.tasks.values().collect()
    }

    /// Check if all dependencies for a task are completed.
    fn are_dependencies_completed(&self, task_id: &str) -> bool {
        if let Some(deps) = self.dependencies.get(task_id) {
            deps.iter().all(|dep_id| self.completed.contains(dep_id))
        } else {
            true // No dependencies = ready
        }
    }

    /// Update the status of all tasks based on current completion state.
    fn update_task_statuses(&mut self) {
        let task_ids: Vec<String> = self.tasks.keys().cloned().collect();

        for task_id in task_ids {
            // Skip already completed tasks
            if self.completed.contains(&task_id) {
                if let Some(task) = self.tasks.get_mut(&task_id) {
                    task.status = TaskStatus::Completed;
                }
                continue;
            }

            // Check if dependencies are satisfied
            let deps_done = self.are_dependencies_completed(&task_id);

            if let Some(task) = self.tasks.get_mut(&task_id) {
                match task.status {
                    TaskStatus::Completed => {
                        // Already completed, don't change
                    }
                    TaskStatus::InProgress | TaskStatus::Assigned | TaskStatus::Validating => {
                        // Keep current status if actively being worked on
                    }
                    _ => {
                        // Update based on dependencies
                        task.status = if deps_done {
                            TaskStatus::Ready
                        } else {
                            TaskStatus::Blocked
                        };
                    }
                }
            }
        }
    }
}

impl Decomposition for PrdDecomposition {
    /// Decompose a task into subtasks.
    ///
    /// For PRD decomposition, the top-level task is decomposed into all tasks
    /// from the PRD file. This is typically called once at initialization.
    fn decompose(&mut self, _task: &Task) -> Result<Vec<Task>> {
        // Return all tasks from the PRD
        Ok(self.tasks.values().cloned().collect())
    }

    /// Check if this decomposition can handle the given task.
    ///
    /// PRD decomposition is typically used for the root task only.
    fn can_decompose(&self, task: &Task) -> bool {
        // Can decompose if the task is a root/meta task
        // For simplicity, we check if it's not one of our PRD tasks
        !self.tasks.contains_key(&task.id.0)
    }

    /// Add a dynamic task discovered at runtime.
    ///
    /// This allows tasks to be added during execution, e.g., from subtask discoveries.
    fn add_dynamic_task(&mut self, parent_id: TaskId, subtask: Task) -> Result<TaskId> {
        let new_task_id = subtask.id.clone();

        // Add dependency on parent task
        let mut new_task = subtask;
        if !new_task.blocked_by.contains(&parent_id) {
            new_task.blocked_by.push(parent_id.clone());
        }

        // Determine status based on dependencies
        let deps_done = new_task
            .blocked_by
            .iter()
            .all(|dep_id| self.completed.contains(&dep_id.0));

        new_task.status = if deps_done {
            TaskStatus::Ready
        } else {
            TaskStatus::Blocked
        };

        // Store the task
        self.tasks.insert(new_task_id.0.clone(), new_task);

        // Update dependencies map
        let dep_ids: Vec<String> = self
            .tasks
            .get(&new_task_id.0)
            .map(|t| t.blocked_by.iter().map(|id| id.0.clone()).collect())
            .unwrap_or_default();
        self.dependencies.insert(new_task_id.0.clone(), dep_ids);

        // Update statuses
        self.update_task_statuses();

        Ok(new_task_id)
    }

    /// Get all tasks that are ready to execute (no blocking dependencies).
    fn ready_tasks(&self) -> Vec<TaskId> {
        self.tasks
            .values()
            .filter(|task| {
                task.status == TaskStatus::Ready && !self.completed.contains(&task.id.0)
            })
            .map(|task| task.id.clone())
            .collect()
    }

    /// Mark a task as complete and update dependency graph.
    fn mark_complete(&mut self, task_id: TaskId) -> Result<()> {
        // Mark task as completed
        self.completed.insert(task_id.0.clone());

        // Update task status
        if let Some(task) = self.tasks.get_mut(&task_id.0) {
            task.status = TaskStatus::Completed;
        } else {
            anyhow::bail!("Task not found: {}", task_id.0);
        }

        // Update statuses of all tasks (unblock dependent tasks)
        self.update_task_statuses();

        // Optionally update the PRD file on disk
        // (This could be done here or externally via `crate::prd::mark_task_done`)

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const TEST_PRD: &str = r#"# Test PRD

## Track A: Foundation

- [ ] Task 1: Build core module
- [ ] Task 2: Add basic API

## Track B: Advanced — depends on prd-1 AND prd-2

- [ ] Task 3: Create CLI **USE /carousel SKILL**
- [ ] Task 4: Add commands. Verify: `cargo test` passes.

## Track C: Independent

- [x] Task 5: Write documentation
"#;

    fn create_test_prd() -> (PathBuf, tempfile::TempDir) {
        let temp_dir = tempfile::tempdir().unwrap();
        let prd_path = temp_dir.path().join("test_prd.md");
        let mut file = std::fs::File::create(&prd_path).unwrap();
        file.write_all(TEST_PRD.as_bytes()).unwrap();
        (prd_path, temp_dir)
    }

    #[test]
    fn test_prd_decomposition_parse() {
        let (prd_path, _temp_dir) = create_test_prd();
        let decomp = PrdDecomposition::from_path(prd_path).unwrap();

        // Should have 5 tasks
        assert_eq!(decomp.tasks.len(), 5);

        // Check task IDs
        assert!(decomp.tasks.contains_key("prd-1"));
        assert!(decomp.tasks.contains_key("prd-5"));

        // Check task 5 is marked completed
        assert!(decomp.completed.contains("prd-5"));
        let task5 = decomp.tasks.get("prd-5").unwrap();
        assert_eq!(task5.status, TaskStatus::Completed);
    }

    #[test]
    fn test_ready_tasks() {
        let (prd_path, _temp_dir) = create_test_prd();
        let decomp = PrdDecomposition::from_path(prd_path).unwrap();

        let ready = decomp.ready_tasks();

        // Tasks 1, 2 should be ready (no dependencies)
        // Task 5 is completed, so not ready
        // Tasks 3, 4 are blocked by prd-1 and prd-2
        assert_eq!(ready.len(), 2);
        assert!(ready.iter().any(|id| id.0 == "prd-1"));
        assert!(ready.iter().any(|id| id.0 == "prd-2"));
    }

    #[test]
    fn test_mark_complete_unblocks() {
        let (prd_path, _temp_dir) = create_test_prd();
        let mut decomp = PrdDecomposition::from_path(prd_path).unwrap();

        // Initially tasks 3 and 4 are blocked
        let task3 = decomp.tasks.get("prd-3").unwrap();
        assert_eq!(task3.status, TaskStatus::Blocked);

        // Complete task 1
        decomp.mark_complete(TaskId("prd-1".to_string())).unwrap();

        // Task 3 still blocked (needs prd-2 as well)
        let task3 = decomp.tasks.get("prd-3").unwrap();
        assert_eq!(task3.status, TaskStatus::Blocked);

        // Complete task 2
        decomp.mark_complete(TaskId("prd-2".to_string())).unwrap();

        // Now task 3 and 4 should be ready
        let task3 = decomp.tasks.get("prd-3").unwrap();
        assert_eq!(task3.status, TaskStatus::Ready);
        let task4 = decomp.tasks.get("prd-4").unwrap();
        assert_eq!(task4.status, TaskStatus::Ready);

        let ready = decomp.ready_tasks();
        assert_eq!(ready.len(), 2);
    }

    #[test]
    fn test_skill_hint_extraction() {
        let (prd_path, _temp_dir) = create_test_prd();
        let decomp = PrdDecomposition::from_path(prd_path).unwrap();

        // Task 3 has skill hint
        let hint = decomp.skill_hint(&TaskId("prd-3".to_string()));
        assert_eq!(hint, Some("carousel"));

        // Task 1 has no skill hint
        let hint = decomp.skill_hint(&TaskId("prd-1".to_string()));
        assert_eq!(hint, None);
    }

    #[test]
    fn test_verify_cmd_extraction() {
        let (prd_path, _temp_dir) = create_test_prd();
        let decomp = PrdDecomposition::from_path(prd_path).unwrap();

        // Task 4 has verify command
        let cmd = decomp.verify_cmd(&TaskId("prd-4".to_string()));
        assert_eq!(cmd, Some("cargo test"));

        // Task 1 has no verify command
        let cmd = decomp.verify_cmd(&TaskId("prd-1".to_string()));
        assert_eq!(cmd, None);
    }

    #[test]
    fn test_add_dynamic_task() {
        let (prd_path, _temp_dir) = create_test_prd();
        let mut decomp = PrdDecomposition::from_path(prd_path).unwrap();

        // Add a dynamic task that depends on prd-1
        let dynamic_task = Task {
            id: TaskId("dynamic-1".to_string()),
            description: "Dynamic task".to_string(),
            status: TaskStatus::Blocked,
            assigned_to: None,
            priority: 50,
            blocked_by: vec![],
            created_at: Utc::now(),
        };

        let new_id = decomp
            .add_dynamic_task(TaskId("prd-1".to_string()), dynamic_task)
            .unwrap();

        assert_eq!(new_id.0, "dynamic-1");

        // Task should be blocked until prd-1 is done
        let task = decomp.tasks.get("dynamic-1").unwrap();
        assert_eq!(task.status, TaskStatus::Blocked);

        // Complete prd-1
        decomp.mark_complete(TaskId("prd-1".to_string())).unwrap();

        // Dynamic task should now be ready
        let task = decomp.tasks.get("dynamic-1").unwrap();
        assert_eq!(task.status, TaskStatus::Ready);
    }
}
