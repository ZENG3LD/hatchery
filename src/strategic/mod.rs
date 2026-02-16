//! StrategicAdvisor trait — abstracts strategic decision-making for escalations.
//!
//! Overmind is one implementation (LLM-powered). This trait allows swapping between:
//! - Rule-based advisor (deterministic heuristics)
//! - LLM-powered advisor (Claude for complex decisions)
//! - Hybrid advisor (rules + LLM fallback)
//! - Custom strategies

use crate::core::types::{AgentId, TaskId};
use anyhow::Result;
use async_trait::async_trait;

pub mod llm_advisor;
pub mod rule_based;

/// Event that triggers strategic advice.
#[derive(Debug, Clone)]
pub enum StrategicEvent {
    /// Task escalated after repeated failures.
    TaskEscalation {
        task_id: TaskId,
        retry_count: usize,
        reasons: Vec<String>,
    },
    /// Deadlock detected — idle agents but remaining tasks.
    DeadlockDetected {
        idle_agents: Vec<AgentId>,
        blocked_tasks: Vec<TaskId>,
        ready_tasks: Vec<TaskId>,
    },
    /// Merge conflict between agents.
    MergeConflict {
        task_id: TaskId,
        conflicting_files: Vec<String>,
    },
    /// Bottleneck task blocking many others.
    BottleneckDetected {
        task_id: TaskId,
        blocked_count: usize,
    },
    /// Agent recovery failed.
    AgentRecoveryFailed {
        agent_id: AgentId,
        task_id: TaskId,
        attempts: usize,
    },
}

/// Strategic command to execute.
#[derive(Debug, Clone)]
pub enum StrategicCommand {
    /// Retry task with modified description/approach.
    RetryTask {
        task_id: TaskId,
        modified_description: Option<String>,
    },
    /// Spawn multiple agents on a task (zerg rush).
    ZergRush {
        task_id: TaskId,
        num_agents: usize,
    },
    /// Re-decompose a task into new subtasks.
    Redecompose {
        task_id: TaskId,
        new_subtask_descriptions: Vec<String>,
    },
    /// Spawn additional agents.
    SpawnAgents { count: usize },
    /// Mark task as permanently failed.
    FailTask { task_id: TaskId, reason: String },
    /// Shut down the entire pipeline.
    Shutdown { reason: String },
    /// No action needed.
    Noop,
}

/// StrategicAdvisor provides strategic decisions for complex failure scenarios.
#[async_trait]
pub trait StrategicAdvisor: Send + Sync {
    /// Analyze an event and recommend a strategic action.
    async fn advise(&mut self, event: StrategicEvent) -> Result<StrategicCommand>;

    /// Get the name of this advisor strategy.
    fn name(&self) -> &str;

    /// Whether this advisor uses LLM (affects cost tracking).
    fn uses_llm(&self) -> bool;
}
