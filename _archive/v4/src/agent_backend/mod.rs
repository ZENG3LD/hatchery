//! AgentBackend trait — abstracts how agents are spawned, communicated with, and managed.
//!
//! The Queen implementation is one backend. This trait allows swapping between:
//! - Claude Code subprocesses (via Queen)
//! - Claude API direct calls
//! - Mock agents for testing
//! - Future: other LLM providers

use crate::core::types::{AgentId, Task, TaskContext, TaskId};
use anyhow::Result;
use async_trait::async_trait;
use std::path::PathBuf;

pub mod claude_code;

/// Event from an agent backend.
#[derive(Debug, Clone)]
pub enum AgentEvent {
    /// Task completed successfully.
    TaskCompleted {
        agent_id: AgentId,
        task_id: TaskId,
        result_text: String,
        cost_usd: f64,
        duration_ms: u64,
    },
    /// Task failed with error.
    TaskFailed {
        agent_id: AgentId,
        task_id: TaskId,
        error: String,
    },
    /// Progress update during task execution.
    Progress {
        agent_id: AgentId,
        task_id: TaskId,
        turns_completed: u32,
        cost_usd: f64,
    },
    /// Agent process died unexpectedly.
    ProcessDied {
        agent_id: AgentId,
        exit_code: Option<i32>,
    },
}

/// Configuration for spawning a new agent.
#[derive(Debug, Clone)]
pub struct AgentSpawnConfig {
    /// Working directory for the agent.
    pub working_dir: PathBuf,
    /// System prompt to initialize the agent with.
    pub system_prompt: Option<String>,
    /// Maximum turns before auto-termination.
    pub max_turns: Option<u32>,
    /// Model to use (e.g., "claude-sonnet-4-5-20250929").
    pub model: Option<String>,
    /// Allowed tools (if None, all tools are allowed).
    pub allowed_tools: Option<Vec<String>>,
}

impl Default for AgentSpawnConfig {
    fn default() -> Self {
        Self {
            working_dir: std::env::current_dir().unwrap_or_default(),
            system_prompt: None,
            max_turns: None,
            model: None,
            allowed_tools: None,
        }
    }
}

/// AgentBackend defines how agents are spawned and communicated with.
#[async_trait]
pub trait AgentBackend: Send + Sync {
    /// Spawn a new agent, returning its ID.
    async fn spawn(&mut self, config: AgentSpawnConfig) -> Result<AgentId>;

    /// Assign a task to an agent.
    async fn assign_task(
        &self,
        agent_id: &AgentId,
        task: Task,
        context: TaskContext,
    ) -> Result<()>;

    /// Abort a task on an agent.
    async fn abort_task(&self, agent_id: &AgentId, task_id: &TaskId, reason: &str)
        -> Result<()>;

    /// Shutdown an agent gracefully.
    async fn shutdown(&self, agent_id: &AgentId) -> Result<()>;

    /// Poll for the next event from any managed agent.
    async fn next_event(&mut self) -> Option<AgentEvent>;

    /// Get list of currently active agent IDs.
    fn active_agents(&self) -> Vec<AgentId>;

    /// Check if an agent is alive.
    fn is_alive(&self, agent_id: &AgentId) -> bool;
}
