use crate::core::types::{Task, TaskId, TaskStatus};
use crate::decomposition::Decomposition;
use anyhow::{anyhow, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ConditionOp {
    Equals,
    NotEquals,
    GreaterThan,
    LessThan,
    Contains,
    Exists,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum EffectOp {
    Set,
    Add,
    Remove,
    Increment,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Precondition {
    pub variable: String,
    pub condition: ConditionOp,
    pub value: serde_json::Value,
}

impl Precondition {
    pub fn evaluate(&self, world_state: &WorldState) -> bool {
        let state_value = world_state.get(&self.variable);

        match &self.condition {
            ConditionOp::Exists => state_value.is_some(),
            ConditionOp::Equals => {
                if let Some(val) = state_value {
                    val == &self.value
                } else {
                    false
                }
            }
            ConditionOp::NotEquals => {
                if let Some(val) = state_value {
                    val != &self.value
                } else {
                    true
                }
            }
            ConditionOp::GreaterThan => {
                if let (Some(serde_json::Value::Number(a)), serde_json::Value::Number(b)) =
                    (state_value, &self.value)
                {
                    a.as_f64().unwrap_or(0.0) > b.as_f64().unwrap_or(0.0)
                } else {
                    false
                }
            }
            ConditionOp::LessThan => {
                if let (Some(serde_json::Value::Number(a)), serde_json::Value::Number(b)) =
                    (state_value, &self.value)
                {
                    a.as_f64().unwrap_or(0.0) < b.as_f64().unwrap_or(0.0)
                } else {
                    false
                }
            }
            ConditionOp::Contains => {
                if let (Some(serde_json::Value::String(a)), serde_json::Value::String(b)) =
                    (state_value, &self.value)
                {
                    a.contains(b)
                } else if let (Some(serde_json::Value::Array(a)), _) = (state_value, &self.value) {
                    a.contains(&self.value)
                } else {
                    false
                }
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Effect {
    pub variable: String,
    pub operation: EffectOp,
    pub value: serde_json::Value,
}

impl Effect {
    pub fn apply(&self, world_state: &mut WorldState) {
        match &self.operation {
            EffectOp::Set => {
                world_state.set(self.variable.clone(), self.value.clone());
            }
            EffectOp::Add => {
                if let serde_json::Value::Array(arr) = world_state
                    .get(&self.variable)
                    .cloned()
                    .unwrap_or(serde_json::Value::Array(Vec::new()))
                {
                    let mut new_arr = arr.clone();
                    new_arr.push(self.value.clone());
                    world_state.set(self.variable.clone(), serde_json::Value::Array(new_arr));
                }
            }
            EffectOp::Remove => {
                if let Some(serde_json::Value::Array(arr)) = world_state.get(&self.variable) {
                    let new_arr: Vec<serde_json::Value> = arr
                        .iter()
                        .filter(|v| *v != &self.value)
                        .cloned()
                        .collect();
                    world_state.set(self.variable.clone(), serde_json::Value::Array(new_arr));
                }
            }
            EffectOp::Increment => {
                if let Some(serde_json::Value::Number(n)) = world_state.get(&self.variable) {
                    if let Some(current) = n.as_f64() {
                        if let serde_json::Value::Number(inc) = &self.value {
                            if let Some(inc_val) = inc.as_f64() {
                                let new_val = current + inc_val;
                                world_state.set(
                                    self.variable.clone(),
                                    serde_json::json!(new_val),
                                );
                            }
                        }
                    }
                } else {
                    // Initialize to increment value if doesn't exist
                    world_state.set(self.variable.clone(), self.value.clone());
                }
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Method {
    pub name: String,
    pub task_pattern: String,
    pub preconditions: Vec<Precondition>,
    pub subtasks: Vec<String>,
}

impl Method {
    pub fn matches(&self, task: &Task, world_state: &WorldState) -> bool {
        // Check if task description matches pattern
        let pattern_match = task.description.to_lowercase().contains(&self.task_pattern.to_lowercase());

        if !pattern_match {
            return false;
        }

        // Check preconditions
        self.preconditions.iter().all(|p| p.evaluate(world_state))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Operator {
    pub name: String,
    pub preconditions: Vec<Precondition>,
    pub effects: Vec<Effect>,
}

impl Operator {
    pub fn is_applicable(&self, world_state: &WorldState) -> bool {
        self.preconditions.iter().all(|p| p.evaluate(world_state))
    }

    pub fn apply(&self, world_state: &mut WorldState) {
        for effect in &self.effects {
            effect.apply(world_state);
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct WorldState {
    state: HashMap<String, serde_json::Value>,
}

impl WorldState {
    pub fn new() -> Self {
        Self {
            state: HashMap::new(),
        }
    }

    pub fn get(&self, key: &str) -> Option<&serde_json::Value> {
        self.state.get(key)
    }

    pub fn set(&mut self, key: String, value: serde_json::Value) {
        self.state.insert(key, value);
    }

    pub fn remove(&mut self, key: &str) -> Option<serde_json::Value> {
        self.state.remove(key)
    }

    pub fn contains(&self, key: &str) -> bool {
        self.state.contains_key(key)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HtnKnowledgeBase {
    pub methods: Vec<Method>,
    pub operators: Vec<Operator>,
}

impl HtnKnowledgeBase {
    pub fn new() -> Self {
        Self {
            methods: Vec::new(),
            operators: Vec::new(),
        }
    }

    pub fn with_defaults() -> Self {
        let mut kb = Self::new();

        // Research methods
        kb.methods.push(Method {
            name: "research_api".to_string(),
            task_pattern: "research".to_string(),
            preconditions: vec![],
            subtasks: vec![
                "gather_documentation".to_string(),
                "analyze_endpoints".to_string(),
                "create_summary".to_string(),
            ],
        });

        // Implementation methods
        kb.methods.push(Method {
            name: "implement_feature".to_string(),
            task_pattern: "implement".to_string(),
            preconditions: vec![Precondition {
                variable: "research_complete".to_string(),
                condition: ConditionOp::Equals,
                value: serde_json::json!(true),
            }],
            subtasks: vec![
                "design_structure".to_string(),
                "write_code".to_string(),
                "add_error_handling".to_string(),
                "write_docs".to_string(),
            ],
        });

        // Testing methods
        kb.methods.push(Method {
            name: "test_feature".to_string(),
            task_pattern: "test".to_string(),
            preconditions: vec![Precondition {
                variable: "implementation_complete".to_string(),
                condition: ConditionOp::Equals,
                value: serde_json::json!(true),
            }],
            subtasks: vec![
                "write_unit_tests".to_string(),
                "write_integration_tests".to_string(),
                "run_tests".to_string(),
            ],
        });

        // Debug methods
        kb.methods.push(Method {
            name: "debug_issue".to_string(),
            task_pattern: "debug".to_string(),
            preconditions: vec![],
            subtasks: vec![
                "reproduce_issue".to_string(),
                "identify_root_cause".to_string(),
                "implement_fix".to_string(),
            ],
        });

        // Operators
        kb.operators.push(Operator {
            name: "gather_documentation".to_string(),
            preconditions: vec![],
            effects: vec![Effect {
                variable: "documentation_gathered".to_string(),
                operation: EffectOp::Set,
                value: serde_json::json!(true),
            }],
        });

        kb.operators.push(Operator {
            name: "analyze_endpoints".to_string(),
            preconditions: vec![Precondition {
                variable: "documentation_gathered".to_string(),
                condition: ConditionOp::Equals,
                value: serde_json::json!(true),
            }],
            effects: vec![Effect {
                variable: "endpoints_analyzed".to_string(),
                operation: EffectOp::Set,
                value: serde_json::json!(true),
            }],
        });

        kb.operators.push(Operator {
            name: "create_summary".to_string(),
            preconditions: vec![Precondition {
                variable: "endpoints_analyzed".to_string(),
                condition: ConditionOp::Equals,
                value: serde_json::json!(true),
            }],
            effects: vec![Effect {
                variable: "research_complete".to_string(),
                operation: EffectOp::Set,
                value: serde_json::json!(true),
            }],
        });

        kb.operators.push(Operator {
            name: "design_structure".to_string(),
            preconditions: vec![],
            effects: vec![Effect {
                variable: "design_complete".to_string(),
                operation: EffectOp::Set,
                value: serde_json::json!(true),
            }],
        });

        kb.operators.push(Operator {
            name: "write_code".to_string(),
            preconditions: vec![Precondition {
                variable: "design_complete".to_string(),
                condition: ConditionOp::Equals,
                value: serde_json::json!(true),
            }],
            effects: vec![Effect {
                variable: "code_written".to_string(),
                operation: EffectOp::Set,
                value: serde_json::json!(true),
            }],
        });

        kb.operators.push(Operator {
            name: "add_error_handling".to_string(),
            preconditions: vec![Precondition {
                variable: "code_written".to_string(),
                condition: ConditionOp::Equals,
                value: serde_json::json!(true),
            }],
            effects: vec![Effect {
                variable: "error_handling_complete".to_string(),
                operation: EffectOp::Set,
                value: serde_json::json!(true),
            }],
        });

        kb.operators.push(Operator {
            name: "write_docs".to_string(),
            preconditions: vec![Precondition {
                variable: "error_handling_complete".to_string(),
                condition: ConditionOp::Equals,
                value: serde_json::json!(true),
            }],
            effects: vec![Effect {
                variable: "implementation_complete".to_string(),
                operation: EffectOp::Set,
                value: serde_json::json!(true),
            }],
        });

        kb
    }
}

impl Default for HtnKnowledgeBase {
    fn default() -> Self {
        Self::with_defaults()
    }
}

#[derive(Debug, Clone)]
pub struct HtnConfig {
    pub knowledge_base: HtnKnowledgeBase,
}

impl Default for HtnConfig {
    fn default() -> Self {
        Self {
            knowledge_base: HtnKnowledgeBase::with_defaults(),
        }
    }
}

pub struct HtnDecomposition {
    config: HtnConfig,
    world_state: WorldState,
    task_registry: HashMap<TaskId, Task>,
    completed_tasks: HashMap<TaskId, bool>,
}

impl HtnDecomposition {
    pub fn new(config: HtnConfig) -> Self {
        Self {
            config,
            world_state: WorldState::new(),
            task_registry: HashMap::new(),
            completed_tasks: HashMap::new(),
        }
    }

    /// Find applicable method for a task
    fn find_applicable_method(&self, task: &Task) -> Option<&Method> {
        self.config
            .knowledge_base
            .methods
            .iter()
            .find(|m| m.matches(task, &self.world_state))
    }

    /// Check if a task is primitive (can be executed directly)
    fn is_primitive(&self, task_name: &str) -> bool {
        self.config
            .knowledge_base
            .operators
            .iter()
            .any(|op| op.name == task_name)
    }

    /// Decompose a task using HTN planning
    fn decompose_task(&self, task: &Task, depth: usize) -> Result<Vec<Task>> {
        if depth > 10 {
            return Err(anyhow!("HTN decomposition too deep"));
        }

        // Check if task is primitive
        if self.is_primitive(&task.description) {
            return Ok(vec![task.clone()]);
        }

        // Find applicable method
        let method = self
            .find_applicable_method(task)
            .ok_or_else(|| anyhow!("No applicable method for task: {}", task.description))?;

        // Decompose into subtasks
        let mut result = Vec::new();
        let mut previous_task_id: Option<TaskId> = None;

        for (idx, subtask_name) in method.subtasks.iter().enumerate() {
            let subtask_id = TaskId(format!("{}_{}", task.id.0, idx));
            let blocked_by = if let Some(ref prev_id) = previous_task_id {
                vec![prev_id.clone()]
            } else {
                vec![]
            };

            let subtask = Task {
                id: subtask_id.clone(),
                description: subtask_name.clone(),
                status: if blocked_by.is_empty() {
                    TaskStatus::Ready
                } else {
                    TaskStatus::Blocked
                },
                assigned_to: None,
                priority: task.priority,
                blocked_by,
                created_at: Utc::now(),
            };

            // Recursively decompose if not primitive
            if !self.is_primitive(subtask_name) {
                let decomposed = self.decompose_task(&subtask, depth + 1)?;
                result.extend(decomposed);
            } else {
                result.push(subtask.clone());
            }

            previous_task_id = Some(subtask_id);
        }

        Ok(result)
    }

    /// Plan a complete sequence of actions
    pub fn plan(&self, task: &Task) -> Result<Vec<Task>> {
        self.decompose_task(task, 0)
    }

    /// Apply operator effects to world state
    fn apply_operator(&mut self, operator_name: &str) -> Result<()> {
        let operator = self
            .config
            .knowledge_base
            .operators
            .iter()
            .find(|op| op.name == operator_name)
            .ok_or_else(|| anyhow!("Operator not found: {}", operator_name))?
            .clone();

        if !operator.is_applicable(&self.world_state) {
            return Err(anyhow!("Operator preconditions not met: {}", operator_name));
        }

        operator.apply(&mut self.world_state);
        Ok(())
    }

    pub fn get_world_state(&self) -> &WorldState {
        &self.world_state
    }

    pub fn set_world_state_value(&mut self, key: String, value: serde_json::Value) {
        self.world_state.set(key, value);
    }
}

impl Decomposition for HtnDecomposition {
    fn decompose(&mut self, task: &Task) -> Result<Vec<Task>> {
        let plan = self.plan(task)?;

        // Register all tasks
        for t in &plan {
            self.task_registry.insert(t.id.clone(), t.clone());
            self.completed_tasks.insert(t.id.clone(), false);
        }

        Ok(plan)
    }

    fn can_decompose(&self, task: &Task) -> bool {
        self.find_applicable_method(task).is_some()
    }

    fn add_dynamic_task(&mut self, _parent_id: TaskId, subtask: Task) -> Result<TaskId> {
        let task_id = subtask.id.clone();
        self.task_registry.insert(task_id.clone(), subtask);
        self.completed_tasks.insert(task_id.clone(), false);

        Ok(task_id)
    }

    fn ready_tasks(&self) -> Vec<TaskId> {
        let mut ready = Vec::new();

        for (task_id, task) in &self.task_registry {
            if *self.completed_tasks.get(task_id).unwrap_or(&false) {
                continue;
            }

            // Check if all dependencies are completed
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
        let task_description = self
            .task_registry
            .get(&task_id)
            .map(|t| t.description.clone())
            .ok_or_else(|| anyhow!("Task not found: {:?}", task_id))?;

        // Apply operator effects if this is a primitive task
        if self.is_primitive(&task_description) {
            self.apply_operator(&task_description)?;
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
    fn test_precondition_evaluation() {
        let mut state = WorldState::new();
        state.set("x".to_string(), serde_json::json!(10));

        let precond = Precondition {
            variable: "x".to_string(),
            condition: ConditionOp::Equals,
            value: serde_json::json!(10),
        };

        assert!(precond.evaluate(&state));
    }

    #[test]
    fn test_effect_application() {
        let mut state = WorldState::new();

        let effect = Effect {
            variable: "count".to_string(),
            operation: EffectOp::Set,
            value: serde_json::json!(5),
        };

        effect.apply(&mut state);
        assert_eq!(state.get("count"), Some(&serde_json::json!(5)));
    }

    #[test]
    fn test_htn_decomposition() {
        let config = HtnConfig::default();
        let mut htn = HtnDecomposition::new(config);

        let task = create_test_task("research API endpoints");
        let plan = htn.decompose(&task).unwrap();

        assert!(!plan.is_empty());
    }

    #[test]
    fn test_htn_planning_with_preconditions() {
        let config = HtnConfig::default();
        let mut htn = HtnDecomposition::new(config);

        // Set research as complete
        htn.set_world_state_value("research_complete".to_string(), serde_json::json!(true));

        let task = create_test_task("implement new feature");
        let plan = htn.decompose(&task).unwrap();

        assert!(!plan.is_empty());
    }
}
