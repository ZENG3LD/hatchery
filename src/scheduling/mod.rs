//! Scheduling module for Hatchery orchestration system.
//!
//! Provides various scheduling strategies for task execution:
//! - Event-driven: immediate task dispatch on events
//! - Timer-based: fixed-interval heartbeat scheduling
//! - Hybrid: combines event-driven + timer-based
//! - Load balancing: distributes tasks evenly across agents
//! - Priority: priority queue with bottleneck detection
//! - LLM realtime: priority-based request routing for LLM inference
//! - Work stealing: pull-based shared queue with work stealing

use crate::core::types::{AgentId, TaskId};
use anyhow::Result;
use std::time::Duration;

/// Scheduling trait defines when and how tasks are executed.
pub trait Scheduling: Send + Sync {
    /// Get the next task for the requesting agent.
    fn next_task(&mut self, agent_id: AgentId) -> Result<Option<TaskId>>;

    /// Notify the scheduler that a task has been completed by an agent.
    fn notify_completion(&mut self, task_id: TaskId, agent_id: AgentId) -> Result<()>;

    /// Get the time interval until the next scheduling check.
    fn next_interval(&self) -> Duration;

    /// Check if the scheduler should terminate (all tasks complete).
    fn should_terminate(&self) -> bool;
}

// Module declarations
pub mod event_driven;
pub mod timer_based;
pub mod hybrid;
pub mod load_balancing;
pub mod priority;
pub mod llm_realtime;
pub mod work_stealing;

// Re-exports
pub use event_driven::{EventDrivenConfig, EventDrivenScheduling};
pub use hybrid::{HybridScheduling, HybridSchedulingConfig};
pub use llm_realtime::{InferenceRequest, LlmRealtimeConfig, LlmRealtimeScheduling, PreemptionEvent};
pub use load_balancing::{AgentLoad, LoadBalancingScheduling, LoadBalancingConfig, LoadStrategy};
pub use priority::{Priority, PriorityConfig, PriorityScheduling, PriorityTask};
pub use timer_based::{TimerBasedConfig, TimerBasedScheduling};
pub use work_stealing::{WorkStealingConfig, WorkStealingScheduling};
