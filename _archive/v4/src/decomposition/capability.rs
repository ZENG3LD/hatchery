use crate::core::types::{Task, TaskId, TaskStatus};
use crate::decomposition::Decomposition;
use anyhow::{anyhow, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Capability {
    pub name: String,
    pub proficiency: f64, // 0.0 to 1.0
    pub tools: Vec<String>,
}

impl Capability {
    pub fn new(name: impl Into<String>, proficiency: f64) -> Self {
        Self {
            name: name.into(),
            proficiency: proficiency.clamp(0.0, 1.0),
            tools: Vec::new(),
        }
    }

    pub fn with_tools(mut self, tools: Vec<String>) -> Self {
        self.tools = tools;
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConstraints {
    pub max_concurrent_tasks: usize,
    pub max_cost_per_task: f64,
    pub supported_languages: Vec<String>,
}

impl Default for AgentConstraints {
    fn default() -> Self {
        Self {
            max_concurrent_tasks: 3,
            max_cost_per_task: 1.0,
            supported_languages: vec!["rust".to_string(), "python".to_string()],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentPerformance {
    pub avg_completion_time: Duration,
    pub success_rate: f64, // 0.0 to 1.0
    pub cost_per_token: f64,
}

impl Default for AgentPerformance {
    fn default() -> Self {
        Self {
            avg_completion_time: Duration::from_secs(300), // 5 minutes
            success_rate: 0.95,
            cost_per_token: 0.000003, // Sonnet pricing
        }
    }
}

impl AgentPerformance {
    pub fn estimated_cost(&self, tokens: usize) -> f64 {
        tokens as f64 * self.cost_per_token
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentCard {
    pub agent_id: String,
    pub capabilities: Vec<Capability>,
    pub constraints: AgentConstraints,
    pub performance: AgentPerformance,
}

impl AgentCard {
    pub fn new(agent_id: impl Into<String>) -> Self {
        Self {
            agent_id: agent_id.into(),
            capabilities: Vec::new(),
            constraints: AgentConstraints::default(),
            performance: AgentPerformance::default(),
        }
    }

    pub fn with_capabilities(mut self, capabilities: Vec<Capability>) -> Self {
        self.capabilities = capabilities;
        self
    }

    pub fn with_constraints(mut self, constraints: AgentConstraints) -> Self {
        self.constraints = constraints;
        self
    }

    pub fn with_performance(mut self, performance: AgentPerformance) -> Self {
        self.performance = performance;
        self
    }

    /// Get capability by name
    pub fn get_capability(&self, name: &str) -> Option<&Capability> {
        self.capabilities
            .iter()
            .find(|c| c.name.to_lowercase() == name.to_lowercase())
    }

    /// Check if agent supports a language
    pub fn supports_language(&self, language: &str) -> bool {
        self.constraints
            .supported_languages
            .iter()
            .any(|l| l.to_lowercase() == language.to_lowercase())
    }
}

#[derive(Debug, Clone)]
pub struct CapabilityScore {
    pub agent_id: String,
    pub score: f64,
    pub matched_capabilities: Vec<String>,
    pub unmatched: Vec<String>,
}

impl CapabilityScore {
    pub fn match_percentage(&self) -> f64 {
        let total = self.matched_capabilities.len() + self.unmatched.len();
        if total > 0 {
            self.matched_capabilities.len() as f64 / total as f64
        } else {
            0.0
        }
    }
}

#[derive(Debug, Clone)]
pub struct CapabilityConfig {
    pub cards: Vec<AgentCard>,
}

impl CapabilityConfig {
    pub fn new() -> Self {
        Self { cards: Vec::new() }
    }

    pub fn with_default_cards() -> Self {
        let mut config = Self::new();

        // Research agent
        config.cards.push(
            AgentCard::new("research-agent")
                .with_capabilities(vec![
                    Capability::new("research", 0.95).with_tools(vec!["web_search".to_string(), "read".to_string()]),
                    Capability::new("documentation", 0.90).with_tools(vec!["read".to_string(), "grep".to_string()]),
                    Capability::new("analysis", 0.85),
                ])
                .with_performance(AgentPerformance {
                    avg_completion_time: Duration::from_secs(600),
                    success_rate: 0.92,
                    cost_per_token: 0.000003,
                }),
        );

        // Rust implementer
        config.cards.push(
            AgentCard::new("rust-implementer")
                .with_capabilities(vec![
                    Capability::new("rust", 0.95).with_tools(vec!["write".to_string(), "edit".to_string()]),
                    Capability::new("implementation", 0.90).with_tools(vec!["cargo".to_string()]),
                    Capability::new("error_handling", 0.85),
                ])
                .with_constraints(AgentConstraints {
                    max_concurrent_tasks: 5,
                    max_cost_per_task: 2.0,
                    supported_languages: vec!["rust".to_string()],
                })
                .with_performance(AgentPerformance {
                    avg_completion_time: Duration::from_secs(900),
                    success_rate: 0.93,
                    cost_per_token: 0.000003,
                }),
        );

        // Test specialist
        config.cards.push(
            AgentCard::new("test-specialist")
                .with_capabilities(vec![
                    Capability::new("testing", 0.95).with_tools(vec!["cargo_test".to_string()]),
                    Capability::new("quality_assurance", 0.90),
                    Capability::new("coverage", 0.85),
                ])
                .with_performance(AgentPerformance {
                    avg_completion_time: Duration::from_secs(450),
                    success_rate: 0.94,
                    cost_per_token: 0.000003,
                }),
        );

        // Debug specialist
        config.cards.push(
            AgentCard::new("debug-specialist")
                .with_capabilities(vec![
                    Capability::new("debugging", 0.95).with_tools(vec!["cargo".to_string(), "grep".to_string()]),
                    Capability::new("troubleshooting", 0.92),
                    Capability::new("root_cause_analysis", 0.88),
                ])
                .with_performance(AgentPerformance {
                    avg_completion_time: Duration::from_secs(800),
                    success_rate: 0.89,
                    cost_per_token: 0.000003,
                }),
        );

        // General implementer
        config.cards.push(
            AgentCard::new("implementer")
                .with_capabilities(vec![
                    Capability::new("typescript", 0.90).with_tools(vec!["npm".to_string()]),
                    Capability::new("python", 0.85).with_tools(vec!["pip".to_string()]),
                    Capability::new("implementation", 0.88),
                ])
                .with_constraints(AgentConstraints {
                    max_concurrent_tasks: 4,
                    max_cost_per_task: 1.5,
                    supported_languages: vec![
                        "typescript".to_string(),
                        "python".to_string(),
                        "javascript".to_string(),
                    ],
                })
                .with_performance(AgentPerformance {
                    avg_completion_time: Duration::from_secs(700),
                    success_rate: 0.91,
                    cost_per_token: 0.000003,
                }),
        );

        config
    }

    pub fn add_card(&mut self, card: AgentCard) {
        self.cards.push(card);
    }

    pub fn get_card(&self, agent_id: &str) -> Option<&AgentCard> {
        self.cards.iter().find(|c| c.agent_id == agent_id)
    }
}

impl Default for CapabilityConfig {
    fn default() -> Self {
        Self::with_default_cards()
    }
}

pub struct CapabilityDecomposition {
    config: CapabilityConfig,
    task_registry: HashMap<TaskId, Task>,
    task_agent_map: HashMap<TaskId, String>,
    agent_task_count: HashMap<String, usize>,
    completed_tasks: HashMap<TaskId, bool>,
}

impl CapabilityDecomposition {
    pub fn new(config: CapabilityConfig) -> Self {
        Self {
            config,
            task_registry: HashMap::new(),
            task_agent_map: HashMap::new(),
            agent_task_count: HashMap::new(),
            completed_tasks: HashMap::new(),
        }
    }

    /// Extract required capabilities from task description
    fn extract_required_capabilities(&self, task: &Task) -> Vec<String> {
        let description = task.description.to_lowercase();
        let mut capabilities = Vec::new();

        // Check for capability keywords
        let capability_keywords = [
            ("research", vec!["research", "investigate", "explore", "analyze"]),
            ("rust", vec!["rust", "cargo"]),
            ("implementation", vec!["implement", "code", "write", "develop", "build"]),
            ("testing", vec!["test", "verify", "validate"]),
            ("debugging", vec!["debug", "fix", "troubleshoot"]),
            ("documentation", vec!["document", "docs", "readme"]),
            ("typescript", vec!["typescript", "ts", "node"]),
            ("python", vec!["python", "py"]),
        ];

        for (capability, keywords) in &capability_keywords {
            for keyword in keywords {
                if description.contains(keyword) {
                    if !capabilities.contains(&capability.to_string()) {
                        capabilities.push(capability.to_string());
                    }
                }
            }
        }

        capabilities
    }

    /// Match capabilities and score agents
    fn match_capabilities(&self, task: &Task) -> Vec<CapabilityScore> {
        let required_caps = self.extract_required_capabilities(task);
        let mut scores = Vec::new();

        for card in &self.config.cards {
            let mut matched = Vec::new();
            let mut total_proficiency = 0.0;
            let mut match_count = 0;

            for req_cap in &required_caps {
                if let Some(capability) = card.get_capability(req_cap) {
                    matched.push(req_cap.clone());
                    total_proficiency += capability.proficiency;
                    match_count += 1;
                }
            }

            let unmatched: Vec<String> = required_caps
                .iter()
                .filter(|c| !matched.contains(c))
                .cloned()
                .collect();

            // Calculate weighted score
            let proficiency_score = if match_count > 0 {
                total_proficiency / match_count as f64
            } else {
                0.0
            };

            let success_rate = card.performance.success_rate;
            let cost_factor = 1.0 / (1.0 + card.performance.cost_per_token * 1_000_000.0);

            // Check if agent is at capacity
            let current_tasks = self.agent_task_count.get(&card.agent_id).copied().unwrap_or(0);
            let capacity_factor = if current_tasks >= card.constraints.max_concurrent_tasks {
                0.5 // Penalize agents at capacity
            } else {
                1.0
            };

            let score = proficiency_score * success_rate * cost_factor * capacity_factor;

            scores.push(CapabilityScore {
                agent_id: card.agent_id.clone(),
                score,
                matched_capabilities: matched,
                unmatched,
            });
        }

        // Sort by score descending
        scores.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));

        scores
    }

    /// Generate capability-aligned subtasks
    fn generate_capability_subtasks(&self, task: &Task) -> Vec<(Task, String)> {
        let description = task.description.to_lowercase();
        let mut subtasks = Vec::new();

        // Complex multi-capability tasks
        if description.contains("research") && description.contains("implement") {
            let research = self.create_subtask(
                task,
                format!("Research phase: {}", task.description),
                vec![],
            );
            subtasks.push((research.clone(), "research-agent".to_string()));

            let implement = self.create_subtask(
                task,
                format!("Implementation phase: {}", task.description),
                vec![research.id.clone()],
            );

            // Pick appropriate implementer based on language
            let agent = if description.contains("rust") {
                "rust-implementer"
            } else {
                "implementer"
            };
            subtasks.push((implement.clone(), agent.to_string()));

            let test = self.create_subtask(
                task,
                format!("Testing phase: {}", task.description),
                vec![implement.id.clone()],
            );
            subtasks.push((test, "test-specialist".to_string()));
        } else {
            // Match to best agent
            let scores = self.match_capabilities(task);
            if let Some(best) = scores.first() {
                // Single task assigned to best agent
                subtasks.push((task.clone(), best.agent_id.clone()));
            } else {
                // Fallback to general implementer
                subtasks.push((task.clone(), "implementer".to_string()));
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

    /// Get agent assigned to a task
    pub fn get_task_agent(&self, task_id: &TaskId) -> Option<&str> {
        self.task_agent_map.get(task_id).map(|s| s.as_str())
    }

    /// Get all tasks assigned to an agent
    pub fn get_agent_tasks(&self, agent_id: &str) -> Vec<TaskId> {
        self.task_agent_map
            .iter()
            .filter(|(_, a)| a.as_str() == agent_id)
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Get agent card
    pub fn get_agent_card(&self, agent_id: &str) -> Option<&AgentCard> {
        self.config.get_card(agent_id)
    }

    /// Get agent workload
    pub fn get_agent_workload(&self, agent_id: &str) -> usize {
        self.agent_task_count.get(agent_id).copied().unwrap_or(0)
    }

    /// Calculate best agent for a task
    pub fn best_agent_for_task(&self, task: &Task) -> Option<String> {
        let scores = self.match_capabilities(task);
        scores.first().map(|s| s.agent_id.clone())
    }
}

impl Decomposition for CapabilityDecomposition {
    fn decompose(&mut self, task: &Task) -> Result<Vec<Task>> {
        let subtasks_with_agents = self.generate_capability_subtasks(task);

        let subtasks: Vec<Task> = subtasks_with_agents
            .iter()
            .map(|(t, _)| t.clone())
            .collect();

        // Register tasks with agents
        for (subtask, agent_id) in subtasks_with_agents {
            self.task_registry.insert(subtask.id.clone(), subtask.clone());
            self.task_agent_map.insert(subtask.id.clone(), agent_id.clone());
            self.completed_tasks.insert(subtask.id.clone(), false);

            // Update agent task count
            *self.agent_task_count.entry(agent_id).or_insert(0) += 1;
        }

        Ok(subtasks)
    }

    fn can_decompose(&self, task: &Task) -> bool {
        !task.description.is_empty() && !self.match_capabilities(task).is_empty()
    }

    fn add_dynamic_task(&mut self, _parent_id: TaskId, subtask: Task) -> Result<TaskId> {
        let agent = self
            .best_agent_for_task(&subtask)
            .ok_or_else(|| anyhow!("No suitable agent for task: {}", subtask.description))?;

        let task_id = subtask.id.clone();
        self.task_registry.insert(task_id.clone(), subtask);
        self.task_agent_map.insert(task_id.clone(), agent.clone());
        self.completed_tasks.insert(task_id.clone(), false);

        *self.agent_task_count.entry(agent).or_insert(0) += 1;

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

        // Decrease agent task count
        if let Some(agent_id) = self.task_agent_map.get(&task_id) {
            if let Some(count) = self.agent_task_count.get_mut(agent_id) {
                *count = count.saturating_sub(1);
            }
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
    fn test_capability_extraction() {
        let config = CapabilityConfig::with_default_cards();
        let decomp = CapabilityDecomposition::new(config);

        let task = create_test_task("Research Rust API and implement connector");
        let caps = decomp.extract_required_capabilities(&task);

        assert!(caps.contains(&"research".to_string()));
        assert!(caps.contains(&"rust".to_string()));
        assert!(caps.contains(&"implementation".to_string()));
    }

    #[test]
    fn test_capability_matching() {
        let config = CapabilityConfig::with_default_cards();
        let decomp = CapabilityDecomposition::new(config);

        let task = create_test_task("Implement Rust connector");
        let scores = decomp.match_capabilities(&task);

        assert!(!scores.is_empty());
        let best = &scores[0];
        assert_eq!(best.agent_id, "rust-implementer");
    }

    #[test]
    fn test_capability_decomposition() {
        let config = CapabilityConfig::with_default_cards();
        let mut decomp = CapabilityDecomposition::new(config);

        let task = create_test_task("Research API and implement Rust connector with tests");
        let subtasks = decomp.decompose(&task).unwrap();

        assert!(!subtasks.is_empty());
    }

    #[test]
    fn test_agent_workload_tracking() {
        let config = CapabilityConfig::with_default_cards();
        let mut decomp = CapabilityDecomposition::new(config);

        let task = create_test_task("Research and implement feature");
        let subtasks = decomp.decompose(&task).unwrap();

        // Check that workload is tracked
        for subtask in &subtasks {
            if let Some(agent) = decomp.get_task_agent(&subtask.id) {
                assert!(decomp.get_agent_workload(agent) > 0);
            }
        }

        // Complete a task and check workload decreases
        if let Some(first_task) = subtasks.first() {
            let agent = decomp.get_task_agent(&first_task.id).unwrap().to_string();
            let before = decomp.get_agent_workload(&agent);
            decomp.mark_complete(first_task.id.clone()).unwrap();
            let after = decomp.get_agent_workload(&agent);
            assert_eq!(after, before - 1);
        }
    }
}
