use crate::core::types::{Task, TaskId};
use anyhow::Result;

/// Decomposition defines how tasks are broken down into subtasks.
pub trait Decomposition: Send + Sync {
    /// Decompose a task into subtasks
    fn decompose(&mut self, task: &Task) -> Result<Vec<Task>>;

    /// Check if this decomposition strategy can handle the given task
    fn can_decompose(&self, task: &Task) -> bool;

    /// Add a dynamic task discovered at runtime
    fn add_dynamic_task(&mut self, parent_id: TaskId, subtask: Task) -> Result<TaskId>;

    /// Get all tasks that are ready to execute (no blocking dependencies)
    fn ready_tasks(&self) -> Vec<TaskId>;

    /// Mark a task as complete and update dependency graph
    fn mark_complete(&mut self, task_id: TaskId) -> Result<()>;
}

pub mod dag;
pub mod htn;
pub mod tdag;
pub mod emergent;
pub mod role_based;
pub mod capability;
pub mod prd;

pub use dag::{DagDecomposition, DagDecompositionConfig};
pub use htn::{HtnDecomposition, HtnConfig, HtnKnowledgeBase, Method, Operator};
pub use tdag::{TdagDecomposition, TdagConfig};
pub use emergent::{EmergentDecomposition, EmergentConfig, EmergentStrategy};
pub use role_based::{RoleBasedDecomposition, RoleBasedConfig, RoleDefinition, QueenRole};
pub use capability::{CapabilityDecomposition, CapabilityConfig, AgentCard, Capability};
pub use prd::{PrdDecomposition, PrdDecompositionConfig};
