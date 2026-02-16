use crate::core::types::{Task, TaskId, TaskStatus};
use crate::decomposition::Decomposition;
use anyhow::{anyhow, Result};
use chrono::Utc;
use std::collections::{HashMap, HashSet, VecDeque};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct DagDecompositionConfig {
    pub use_llm: bool,
    pub llm_model: String,
}

impl Default for DagDecompositionConfig {
    fn default() -> Self {
        Self {
            use_llm: false,
            llm_model: "claude-sonnet-4-5".to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct DagTask {
    pub task: Task,
    pub blocked_by: Vec<TaskId>,
    pub blocks: Vec<TaskId>,
    pub completed: bool,
    pub retry_count: u32,
    pub bottleneck_score: f64,
}

impl DagTask {
    fn new(task: Task) -> Self {
        Self {
            blocked_by: task.blocked_by.clone(),
            task,
            blocks: Vec::new(),
            completed: false,
            retry_count: 0,
            bottleneck_score: 0.0,
        }
    }
}

pub struct DagDecomposition {
    config: DagDecompositionConfig,
    tasks: HashMap<TaskId, DagTask>,
}

impl DagDecomposition {
    pub fn new(config: DagDecompositionConfig) -> Self {
        Self {
            config,
            tasks: HashMap::new(),
        }
    }

    /// Parse task description for dependency hints
    fn extract_dependencies(&self, description: &str) -> Vec<String> {
        let mut deps = Vec::new();
        let lower = description.to_lowercase();

        // Look for patterns like "after X", "depends on Y", "requires Z"
        let patterns = [
            ("after ", " "),
            ("depends on ", " "),
            ("requires ", " "),
            ("needs ", " "),
            ("following ", " "),
        ];

        for (prefix, suffix) in &patterns {
            if let Some(start_idx) = lower.find(prefix) {
                let after_prefix = &description[start_idx + prefix.len()..];
                if let Some(end_idx) = after_prefix.find(suffix) {
                    let dep = after_prefix[..end_idx].trim().to_string();
                    if !dep.is_empty() && !deps.contains(&dep) {
                        deps.push(dep);
                    }
                } else {
                    // Take rest of string if no suffix found
                    let dep = after_prefix.trim().to_string();
                    if !dep.is_empty() && !deps.contains(&dep) {
                        deps.push(dep);
                    }
                }
            }
        }

        deps
    }

    /// Generate subtasks based on task type/description
    fn generate_subtasks(&self, task: &Task) -> Vec<Task> {
        let description = task.description.to_lowercase();
        let mut subtasks = Vec::new();

        // Research tasks
        if description.contains("research") || description.contains("investigate") {
            subtasks.push(self.create_subtask(
                task,
                "Gather initial documentation and resources",
                vec![],
            ));
            subtasks.push(self.create_subtask(
                task,
                "Analyze API endpoints and authentication methods",
                vec![subtasks[0].id.clone()],
            ));
            subtasks.push(self.create_subtask(
                task,
                "Document findings and create research summary",
                vec![subtasks[1].id.clone()],
            ));
        }
        // Implementation tasks
        else if description.contains("implement") || description.contains("code") || description.contains("write") {
            subtasks.push(self.create_subtask(
                task,
                "Design module structure and interfaces",
                vec![],
            ));
            subtasks.push(self.create_subtask(
                task,
                "Implement core functionality",
                vec![subtasks[0].id.clone()],
            ));
            subtasks.push(self.create_subtask(
                task,
                "Add error handling and validation",
                vec![subtasks[1].id.clone()],
            ));
            subtasks.push(self.create_subtask(
                task,
                "Write documentation and examples",
                vec![subtasks[2].id.clone()],
            ));
        }
        // Testing tasks
        else if description.contains("test") {
            subtasks.push(self.create_subtask(
                task,
                "Write unit tests",
                vec![],
            ));
            subtasks.push(self.create_subtask(
                task,
                "Write integration tests",
                vec![],
            ));
            subtasks.push(self.create_subtask(
                task,
                "Run test suite and verify coverage",
                vec![subtasks[0].id.clone(), subtasks[1].id.clone()],
            ));
        }
        // Debug tasks
        else if description.contains("debug") || description.contains("fix") {
            subtasks.push(self.create_subtask(
                task,
                "Reproduce the issue",
                vec![],
            ));
            subtasks.push(self.create_subtask(
                task,
                "Identify root cause",
                vec![subtasks[0].id.clone()],
            ));
            subtasks.push(self.create_subtask(
                task,
                "Implement fix and verify",
                vec![subtasks[1].id.clone()],
            ));
        }
        // Generic decomposition
        else {
            subtasks.push(self.create_subtask(
                task,
                format!("Analyze requirements: {}", task.description),
                vec![],
            ));
            subtasks.push(self.create_subtask(
                task,
                format!("Execute: {}", task.description),
                vec![subtasks[0].id.clone()],
            ));
            subtasks.push(self.create_subtask(
                task,
                format!("Verify completion: {}", task.description),
                vec![subtasks[1].id.clone()],
            ));
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

    /// Check for cycles using DFS
    fn has_cycle(&self, start: &TaskId, target: &TaskId) -> bool {
        let mut visited = HashSet::new();
        let mut stack = vec![start.clone()];

        while let Some(current) = stack.pop() {
            if &current == target {
                return true;
            }

            if visited.contains(&current) {
                continue;
            }
            visited.insert(current.clone());

            if let Some(dag_task) = self.tasks.get(&current) {
                for dep in &dag_task.blocks {
                    stack.push(dep.clone());
                }
            }
        }

        false
    }

    /// Calculate bottleneck score (number of downstream dependents)
    fn calculate_bottleneck_score(&self, task_id: &TaskId) -> f64 {
        let mut count = 0;
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        queue.push_back(task_id.clone());

        while let Some(current) = queue.pop_front() {
            if visited.contains(&current) {
                continue;
            }
            visited.insert(current.clone());
            count += 1;

            if let Some(dag_task) = self.tasks.get(&current) {
                for dependent in &dag_task.blocks {
                    queue.push_back(dependent.clone());
                }
            }
        }

        (count - 1) as f64 // Subtract self
    }

    /// Update bottleneck scores for all tasks
    fn update_bottleneck_scores(&mut self) {
        let task_ids: Vec<TaskId> = self.tasks.keys().cloned().collect();

        for task_id in task_ids {
            let score = self.calculate_bottleneck_score(&task_id);
            if let Some(task) = self.tasks.get_mut(&task_id) {
                task.bottleneck_score = score;
            }
        }
    }

    /// Get topological ordering of all tasks
    pub fn topological_sort(&self) -> Result<Vec<TaskId>> {
        let mut in_degree: HashMap<TaskId, usize> = HashMap::new();
        let mut result = Vec::new();

        // Initialize in-degrees
        for (task_id, dag_task) in &self.tasks {
            in_degree.entry(task_id.clone()).or_insert(0);
            for dep in &dag_task.blocked_by {
                *in_degree.entry(dep.clone()).or_insert(0);
            }
        }

        for (task_id, dag_task) in &self.tasks {
            let count = dag_task.blocked_by.len();
            in_degree.insert(task_id.clone(), count);
        }

        // Kahn's algorithm
        let mut queue: VecDeque<TaskId> = in_degree
            .iter()
            .filter(|(_, &degree)| degree == 0)
            .map(|(id, _)| id.clone())
            .collect();

        while let Some(task_id) = queue.pop_front() {
            result.push(task_id.clone());

            if let Some(dag_task) = self.tasks.get(&task_id) {
                for dependent in &dag_task.blocks {
                    if let Some(degree) = in_degree.get_mut(dependent) {
                        *degree = degree.saturating_sub(1);
                        if *degree == 0 {
                            queue.push_back(dependent.clone());
                        }
                    }
                }
            }
        }

        // Check for cycles
        if result.len() != self.tasks.len() {
            return Err(anyhow!("Cycle detected in task graph"));
        }

        Ok(result)
    }

    /// Get all tasks that are direct or transitive dependencies of the given task
    pub fn get_all_dependencies(&self, task_id: &TaskId) -> HashSet<TaskId> {
        let mut deps = HashSet::new();
        let mut queue = VecDeque::new();

        if let Some(dag_task) = self.tasks.get(task_id) {
            for dep in &dag_task.blocked_by {
                queue.push_back(dep.clone());
            }
        }

        while let Some(dep_id) = queue.pop_front() {
            if deps.contains(&dep_id) {
                continue;
            }
            deps.insert(dep_id.clone());

            if let Some(dag_task) = self.tasks.get(&dep_id) {
                for transitive_dep in &dag_task.blocked_by {
                    queue.push_back(transitive_dep.clone());
                }
            }
        }

        deps
    }

    /// Get critical path (longest path through the DAG)
    pub fn critical_path(&self) -> Vec<TaskId> {
        let topo_order = match self.topological_sort() {
            Ok(order) => order,
            Err(_) => return Vec::new(),
        };

        let mut distances: HashMap<TaskId, usize> = HashMap::new();
        let mut predecessors: HashMap<TaskId, Option<TaskId>> = HashMap::new();

        // Initialize
        for task_id in &topo_order {
            distances.insert(task_id.clone(), 0);
            predecessors.insert(task_id.clone(), None);
        }

        // Calculate longest paths
        for task_id in &topo_order {
            let current_dist = distances[task_id];

            if let Some(dag_task) = self.tasks.get(task_id) {
                for dependent in &dag_task.blocks {
                    let new_dist = current_dist + 1;
                    if new_dist > distances.get(dependent).copied().unwrap_or(0) {
                        distances.insert(dependent.clone(), new_dist);
                        predecessors.insert(dependent.clone(), Some(task_id.clone()));
                    }
                }
            }
        }

        // Find task with maximum distance
        let end_task = distances
            .iter()
            .max_by_key(|(_, &dist)| dist)
            .map(|(id, _)| id.clone());

        if let Some(mut current) = end_task {
            let mut path = vec![current.clone()];

            while let Some(Some(pred)) = predecessors.get(&current) {
                path.push(pred.clone());
                current = pred.clone();
            }

            path.reverse();
            path
        } else {
            Vec::new()
        }
    }

    /// Get parallelizable task sets (tasks that can run concurrently)
    pub fn parallelizable_sets(&self) -> Vec<Vec<TaskId>> {
        let topo_order = match self.topological_sort() {
            Ok(order) => order,
            Err(_) => return Vec::new(),
        };

        let mut levels: HashMap<TaskId, usize> = HashMap::new();

        // Assign levels (maximum depth from any root)
        for task_id in &topo_order {
            let max_dep_level = if let Some(dag_task) = self.tasks.get(task_id) {
                dag_task
                    .blocked_by
                    .iter()
                    .filter_map(|dep| levels.get(dep))
                    .max()
                    .copied()
                    .unwrap_or(0)
            } else {
                0
            };

            levels.insert(task_id.clone(), max_dep_level + 1);
        }

        // Group by level
        let max_level = levels.values().max().copied().unwrap_or(0);
        let mut sets = vec![Vec::new(); max_level];

        for (task_id, &level) in &levels {
            if level > 0 && level <= max_level {
                sets[level - 1].push(task_id.clone());
            }
        }

        sets.into_iter().filter(|set| !set.is_empty()).collect()
    }
}

impl Decomposition for DagDecomposition {
    fn decompose(&mut self, task: &Task) -> Result<Vec<Task>> {
        let subtasks = self.generate_subtasks(task);

        // Register all subtasks in the DAG
        for subtask in &subtasks {
            let dag_task = DagTask::new(subtask.clone());
            self.tasks.insert(subtask.id.clone(), dag_task);
        }

        // Build forward edges (blocks relationships)
        for subtask in &subtasks {
            for dep_id in &subtask.blocked_by {
                if let Some(dep_task) = self.tasks.get_mut(dep_id) {
                    if !dep_task.blocks.contains(&subtask.id) {
                        dep_task.blocks.push(subtask.id.clone());
                    }
                }
            }
        }

        // Update bottleneck scores
        self.update_bottleneck_scores();

        Ok(subtasks)
    }

    fn can_decompose(&self, task: &Task) -> bool {
        // Can decompose any task that isn't already atomic
        !task.description.is_empty() && task.description.len() > 20
    }

    fn add_dynamic_task(&mut self, parent_id: TaskId, subtask: Task) -> Result<TaskId> {
        // Check for cycle before adding
        for dep in &subtask.blocked_by {
            if self.has_cycle(dep, &parent_id) {
                return Err(anyhow!("Adding task would create a cycle"));
            }
        }

        let task_id = subtask.id.clone();
        let mut dag_task = DagTask::new(subtask);

        // Add parent as dependency if it exists
        if self.tasks.contains_key(&parent_id) {
            dag_task.blocked_by.push(parent_id.clone());

            // Update parent's blocks list
            if let Some(parent) = self.tasks.get_mut(&parent_id) {
                parent.blocks.push(task_id.clone());
            }
        }

        // Build forward edges
        for dep_id in &dag_task.blocked_by {
            if let Some(dep_task) = self.tasks.get_mut(dep_id) {
                if !dep_task.blocks.contains(&task_id) {
                    dep_task.blocks.push(task_id.clone());
                }
            }
        }

        self.tasks.insert(task_id.clone(), dag_task);
        self.update_bottleneck_scores();

        Ok(task_id)
    }

    fn ready_tasks(&self) -> Vec<TaskId> {
        let mut ready = Vec::new();

        for (task_id, dag_task) in &self.tasks {
            if dag_task.completed {
                continue;
            }

            // Check if all dependencies are completed
            let all_deps_done = dag_task
                .blocked_by
                .iter()
                .all(|dep_id| {
                    self.tasks
                        .get(dep_id)
                        .map(|t| t.completed)
                        .unwrap_or(true)
                });

            if all_deps_done {
                ready.push(task_id.clone());
            }
        }

        // Sort by bottleneck score (prioritize critical path)
        ready.sort_by(|a, b| {
            let score_a = self.tasks.get(a).map(|t| t.bottleneck_score).unwrap_or(0.0);
            let score_b = self.tasks.get(b).map(|t| t.bottleneck_score).unwrap_or(0.0);
            score_b.partial_cmp(&score_a).unwrap_or(std::cmp::Ordering::Equal)
        });

        ready
    }

    fn mark_complete(&mut self, task_id: TaskId) -> Result<()> {
        let dag_task = self
            .tasks
            .get_mut(&task_id)
            .ok_or_else(|| anyhow!("Task not found: {:?}", task_id))?;

        dag_task.completed = true;
        dag_task.task.status = TaskStatus::Completed;

        // Update dependent tasks' status
        let blocks = dag_task.blocks.clone();
        for dependent_id in blocks {
            // Check dependencies first
            let all_deps_done = if let Some(dependent) = self.tasks.get(&dependent_id) {
                dependent
                    .blocked_by
                    .iter()
                    .all(|dep_id| {
                        self.tasks
                            .get(dep_id)
                            .map(|t| t.completed)
                            .unwrap_or(true)
                    })
            } else {
                false
            };

            // Update status if needed
            if all_deps_done {
                if let Some(dependent) = self.tasks.get_mut(&dependent_id) {
                    if dependent.task.status == TaskStatus::Blocked {
                        dependent.task.status = TaskStatus::Ready;
                    }
                }
            }
        }

        Ok(())
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
    fn test_dag_decomposition() {
        let config = DagDecompositionConfig::default();
        let mut dag = DagDecomposition::new(config);

        let task = create_test_task("Research the new API endpoints and authentication");
        let subtasks = dag.decompose(&task).unwrap();

        assert!(!subtasks.is_empty());
        assert!(subtasks.len() >= 3);
    }

    #[test]
    fn test_cycle_detection() {
        let config = DagDecompositionConfig::default();
        let mut dag = DagDecomposition::new(config);

        let task1 = create_test_task("Task 1");
        let task2 = create_test_task("Task 2");
        let task3 = create_test_task("Task 3");

        dag.tasks.insert(task1.id.clone(), DagTask::new(task1.clone()));
        dag.tasks.insert(task2.id.clone(), DagTask::new(task2.clone()));
        dag.tasks.insert(task3.id.clone(), DagTask::new(task3.clone()));

        // Create chain: task1 -> task2 -> task3 (task1 blocks task2, task2 blocks task3)
        dag.tasks.get_mut(&task1.id).unwrap().blocks.push(task2.id.clone());
        dag.tasks.get_mut(&task2.id).unwrap().blocks.push(task3.id.clone());

        // Now if we wanted to add task3 -> task1, check if it would create a cycle
        // has_cycle(task3, task1) checks if there's already a path task3 -> ... -> task1
        // Since we have task1 -> task2 -> task3, there's a path task1 -> task3
        // So adding task3 -> task1 would create a cycle
        // But has_cycle checks the opposite direction, so we check has_cycle(task1, task3)
        assert!(dag.has_cycle(&task1.id, &task3.id)); // Path exists: task1 -> task2 -> task3

        // No path from task3 to task1
        assert!(!dag.has_cycle(&task3.id, &task1.id));
    }

    #[test]
    fn test_ready_tasks() {
        let config = DagDecompositionConfig::default();
        let mut dag = DagDecomposition::new(config);

        let task = create_test_task("Implement feature with multiple steps");
        let _subtasks = dag.decompose(&task).unwrap();

        let ready = dag.ready_tasks();
        assert!(!ready.is_empty());

        // First task should be ready
        if let Some(first_ready) = ready.first() {
            dag.mark_complete(first_ready.clone()).unwrap();
            let new_ready = dag.ready_tasks();
            // More tasks should become ready after completing the first one
            assert!(new_ready.len() >= ready.len() - 1);
        }
    }
}
