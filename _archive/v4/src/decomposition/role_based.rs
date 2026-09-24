use crate::core::types::{Task, TaskId, TaskStatus};
use crate::decomposition::Decomposition;
use anyhow::{anyhow, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum QueenRole {
    Research,
    Implement,
    Test,
    Debug,
    Review,
    Deploy,
}

impl QueenRole {
    pub fn default_skills(&self) -> Vec<String> {
        match self {
            Self::Research => vec![
                "research".to_string(),
                "documentation".to_string(),
                "analysis".to_string(),
                "investigation".to_string(),
                "exploration".to_string(),
            ],
            Self::Implement => vec![
                "implement".to_string(),
                "code".to_string(),
                "write".to_string(),
                "develop".to_string(),
                "build".to_string(),
                "create".to_string(),
            ],
            Self::Test => vec![
                "test".to_string(),
                "verify".to_string(),
                "validate".to_string(),
                "quality".to_string(),
                "coverage".to_string(),
            ],
            Self::Debug => vec![
                "debug".to_string(),
                "fix".to_string(),
                "troubleshoot".to_string(),
                "diagnose".to_string(),
                "resolve".to_string(),
            ],
            Self::Review => vec![
                "review".to_string(),
                "audit".to_string(),
                "inspect".to_string(),
                "evaluate".to_string(),
                "assess".to_string(),
            ],
            Self::Deploy => vec![
                "deploy".to_string(),
                "release".to_string(),
                "publish".to_string(),
                "ship".to_string(),
                "launch".to_string(),
            ],
        }
    }

    pub fn default_system_prompt(&self) -> String {
        match self {
            Self::Research => {
                "You are a research specialist. Your role is to gather information, analyze documentation, \
                and provide comprehensive research summaries. Focus on thoroughness and accuracy."
                    .to_string()
            }
            Self::Implement => {
                "You are an implementation specialist. Your role is to write production-quality code \
                following best practices. Focus on clean, maintainable, and efficient implementations."
                    .to_string()
            }
            Self::Test => {
                "You are a testing specialist. Your role is to write comprehensive test suites, \
                ensure code coverage, and validate functionality. Focus on edge cases and reliability."
                    .to_string()
            }
            Self::Debug => {
                "You are a debugging specialist. Your role is to identify root causes of issues, \
                reproduce bugs, and implement fixes. Focus on systematic problem-solving."
                    .to_string()
            }
            Self::Review => {
                "You are a code review specialist. Your role is to ensure code quality, identify \
                potential issues, and suggest improvements. Focus on maintainability and best practices."
                    .to_string()
            }
            Self::Deploy => {
                "You are a deployment specialist. Your role is to handle releases, manage deployments, \
                and ensure smooth rollouts. Focus on reliability and safety."
                    .to_string()
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleDefinition {
    pub name: String,
    pub skills: Vec<String>,
    pub system_prompt: String,
}

impl RoleDefinition {
    pub fn from_queen_role(role: QueenRole) -> Self {
        Self {
            name: format!("{:?}", role),
            skills: role.default_skills(),
            system_prompt: role.default_system_prompt(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct RoleBasedConfig {
    pub roles: HashMap<String, RoleDefinition>,
}

impl RoleBasedConfig {
    pub fn new() -> Self {
        Self {
            roles: HashMap::new(),
        }
    }

    pub fn with_default_roles() -> Self {
        let mut config = Self::new();

        for role in &[
            QueenRole::Research,
            QueenRole::Implement,
            QueenRole::Test,
            QueenRole::Debug,
            QueenRole::Review,
            QueenRole::Deploy,
        ] {
            let def = RoleDefinition::from_queen_role(*role);
            config.roles.insert(def.name.clone(), def);
        }

        config
    }

    pub fn add_role(&mut self, role: RoleDefinition) {
        self.roles.insert(role.name.clone(), role);
    }
}

impl Default for RoleBasedConfig {
    fn default() -> Self {
        Self::with_default_roles()
    }
}

pub struct RoleBasedDecomposition {
    config: RoleBasedConfig,
    task_registry: HashMap<TaskId, Task>,
    task_role_map: HashMap<TaskId, String>,
    completed_tasks: HashMap<TaskId, bool>,
}

impl RoleBasedDecomposition {
    pub fn new(config: RoleBasedConfig) -> Self {
        Self {
            config,
            task_registry: HashMap::new(),
            task_role_map: HashMap::new(),
            completed_tasks: HashMap::new(),
        }
    }

    /// Match a task to the most appropriate role
    fn match_role(&self, task: &Task) -> Option<String> {
        let description = task.description.to_lowercase();
        let mut best_match: Option<(String, f64)> = None;

        for (role_name, role_def) in &self.config.roles {
            let mut score = 0.0;
            let mut matches = 0;

            // Count skill keyword matches
            for skill in &role_def.skills {
                if description.contains(&skill.to_lowercase()) {
                    matches += 1;
                    // Weight earlier matches more heavily
                    if let Some(pos) = description.find(&skill.to_lowercase()) {
                        let position_weight = 1.0 / (pos as f64 + 1.0);
                        score += 1.0 + position_weight;
                    } else {
                        score += 1.0;
                    }
                }
            }

            // Bonus for multiple matches
            if matches > 1 {
                score *= 1.0 + (matches as f64 * 0.2);
            }

            if score > 0.0 {
                if let Some((_, best_score)) = best_match {
                    if score > best_score {
                        best_match = Some((role_name.clone(), score));
                    }
                } else {
                    best_match = Some((role_name.clone(), score));
                }
            }
        }

        best_match.map(|(name, _)| name)
    }

    /// Generate subtasks based on role assignment
    fn generate_role_based_subtasks(&self, task: &Task) -> Vec<(Task, String)> {
        let description = task.description.to_lowercase();
        let mut subtasks = Vec::new();

        // Complex tasks often need multiple roles
        if description.contains("research") && description.contains("implement") {
            // Research phase
            let research_task = self.create_subtask(
                task,
                format!("Research phase: {}", task.description),
                vec![],
            );
            subtasks.push((research_task.clone(), "Research".to_string()));

            // Implementation phase
            let impl_task = self.create_subtask(
                task,
                format!("Implementation phase: {}", task.description),
                vec![research_task.id.clone()],
            );
            subtasks.push((impl_task.clone(), "Implement".to_string()));

            // Testing phase
            let test_task = self.create_subtask(
                task,
                format!("Testing phase: {}", task.description),
                vec![impl_task.id.clone()],
            );
            subtasks.push((test_task, "Test".to_string()));
        } else if description.contains("research") {
            // Pure research tasks
            let gather = self.create_subtask(
                task,
                "Gather documentation and resources",
                vec![],
            );
            subtasks.push((gather.clone(), "Research".to_string()));

            let analyze = self.create_subtask(
                task,
                "Analyze and synthesize findings",
                vec![gather.id.clone()],
            );
            subtasks.push((analyze.clone(), "Research".to_string()));

            let document = self.create_subtask(
                task,
                "Create research summary",
                vec![analyze.id.clone()],
            );
            subtasks.push((document, "Research".to_string()));
        } else if description.contains("implement") || description.contains("code") {
            // Implementation tasks
            let design = self.create_subtask(
                task,
                "Design architecture and interfaces",
                vec![],
            );
            subtasks.push((design.clone(), "Implement".to_string()));

            let code = self.create_subtask(
                task,
                "Write core implementation",
                vec![design.id.clone()],
            );
            subtasks.push((code.clone(), "Implement".to_string()));

            let review = self.create_subtask(
                task,
                "Code review and refinement",
                vec![code.id.clone()],
            );
            subtasks.push((review, "Review".to_string()));
        } else if description.contains("test") {
            // Testing tasks
            let unit = self.create_subtask(task, "Write unit tests", vec![]);
            subtasks.push((unit.clone(), "Test".to_string()));

            let integration = self.create_subtask(task, "Write integration tests", vec![]);
            subtasks.push((integration.clone(), "Test".to_string()));

            let run = self.create_subtask(
                task,
                "Run test suite and verify",
                vec![unit.id.clone(), integration.id.clone()],
            );
            subtasks.push((run, "Test".to_string()));
        } else if description.contains("debug") || description.contains("fix") {
            // Debug tasks
            let reproduce = self.create_subtask(task, "Reproduce the issue", vec![]);
            subtasks.push((reproduce.clone(), "Debug".to_string()));

            let diagnose = self.create_subtask(
                task,
                "Diagnose root cause",
                vec![reproduce.id.clone()],
            );
            subtasks.push((diagnose.clone(), "Debug".to_string()));

            let fix = self.create_subtask(
                task,
                "Implement and verify fix",
                vec![diagnose.id.clone()],
            );
            subtasks.push((fix, "Debug".to_string()));
        } else {
            // Generic decomposition with role matching
            let role = self.match_role(task).unwrap_or_else(|| "Research".to_string());

            let plan = self.create_subtask(
                task,
                format!("Plan: {}", task.description),
                vec![],
            );
            subtasks.push((plan.clone(), role.clone()));

            let execute = self.create_subtask(
                task,
                format!("Execute: {}", task.description),
                vec![plan.id.clone()],
            );
            subtasks.push((execute.clone(), role.clone()));

            let verify = self.create_subtask(
                task,
                format!("Verify: {}", task.description),
                vec![execute.id.clone()],
            );
            subtasks.push((verify, "Review".to_string()));
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

    /// Get the role assigned to a task
    pub fn get_task_role(&self, task_id: &TaskId) -> Option<&str> {
        self.task_role_map.get(task_id).map(|s| s.as_str())
    }

    /// Get all tasks assigned to a specific role
    pub fn get_tasks_by_role(&self, role_name: &str) -> Vec<TaskId> {
        self.task_role_map
            .iter()
            .filter(|(_, r)| r.as_str() == role_name)
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Get role definition
    pub fn get_role_definition(&self, role_name: &str) -> Option<&RoleDefinition> {
        self.config.roles.get(role_name)
    }
}

impl Decomposition for RoleBasedDecomposition {
    fn decompose(&mut self, task: &Task) -> Result<Vec<Task>> {
        let subtasks_with_roles = self.generate_role_based_subtasks(task);

        let subtasks: Vec<Task> = subtasks_with_roles
            .iter()
            .map(|(t, _)| t.clone())
            .collect();

        // Register tasks with their roles
        for (subtask, role) in subtasks_with_roles {
            self.task_registry.insert(subtask.id.clone(), subtask.clone());
            self.task_role_map.insert(subtask.id.clone(), role);
            self.completed_tasks.insert(subtask.id.clone(), false);
        }

        Ok(subtasks)
    }

    fn can_decompose(&self, task: &Task) -> bool {
        !task.description.is_empty() && self.match_role(task).is_some()
    }

    fn add_dynamic_task(&mut self, _parent_id: TaskId, subtask: Task) -> Result<TaskId> {
        let role = self
            .match_role(&subtask)
            .ok_or_else(|| anyhow!("No matching role for task: {}", subtask.description))?;

        let task_id = subtask.id.clone();
        self.task_registry.insert(task_id.clone(), subtask);
        self.task_role_map.insert(task_id.clone(), role);
        self.completed_tasks.insert(task_id.clone(), false);

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
    fn test_role_matching() {
        let config = RoleBasedConfig::with_default_roles();
        let decomp = RoleBasedDecomposition::new(config);

        let research_task = create_test_task("Research the API documentation");
        assert_eq!(decomp.match_role(&research_task), Some("Research".to_string()));

        let impl_task = create_test_task("Implement the connector code");
        assert_eq!(decomp.match_role(&impl_task), Some("Implement".to_string()));

        let test_task = create_test_task("Test the implementation");
        assert_eq!(decomp.match_role(&test_task), Some("Test".to_string()));

        let debug_task = create_test_task("Debug the failing test");
        assert_eq!(decomp.match_role(&debug_task), Some("Debug".to_string()));
    }

    #[test]
    fn test_role_based_decomposition() {
        let config = RoleBasedConfig::with_default_roles();
        let mut decomp = RoleBasedDecomposition::new(config);

        let task = create_test_task("Research API and implement connector");
        let subtasks = decomp.decompose(&task).unwrap();

        assert!(!subtasks.is_empty());
        assert!(subtasks.len() >= 3); // Research, Implement, Test phases
    }

    #[test]
    fn test_get_tasks_by_role() {
        let config = RoleBasedConfig::with_default_roles();
        let mut decomp = RoleBasedDecomposition::new(config);

        let task = create_test_task("Research API documentation and implement features");
        let subtasks = decomp.decompose(&task).unwrap();
        assert!(!subtasks.is_empty()); // Verify decomposition happened

        let research_tasks = decomp.get_tasks_by_role("Research");
        assert!(!research_tasks.is_empty());
    }

    #[test]
    fn test_default_skills() {
        let research_skills = QueenRole::Research.default_skills();
        assert!(research_skills.contains(&"research".to_string()));

        let impl_skills = QueenRole::Implement.default_skills();
        assert!(impl_skills.contains(&"implement".to_string()));
    }
}
