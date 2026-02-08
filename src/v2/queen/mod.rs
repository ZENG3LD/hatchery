//! Queen module — core AI agent abstraction for Hatchery V2.
//!
//! A Queen is an autonomous AI "sergeant" that receives tasks from SwarmHost,
//! decomposes them into sub-tasks, manages workers, and reports results.
//!
//! Two implementations:
//! - NativeQueen: wraps Claude Code CLI with PipeProcess (Phase 1.3)
//! - CustomQueen: manages workers via API calls (Phase 1.4)

pub mod native;
pub mod custom;

// Re-export Queen trait and key types
pub use native::NativeQueen;
pub use custom::CustomQueen;

use async_trait::async_trait;
use anyhow::Result;
use crate::v2::types::*;

/// QueenConfig — common configuration for all Queen implementations.
#[derive(Debug, Clone)]
pub struct QueenConfig {
    pub timeout: std::time::Duration,
    pub max_workers: usize,
    pub model: String,
}

/// The Queen trait — core abstraction for AI agent managers.
///
/// A Queen is the basic autonomous unit in Hatchery: an AI "sergeant"
/// that receives tasks from SwarmHost, decomposes them into sub-tasks,
/// manages workers, and reports results back up.
///
/// Two implementations:
/// - NativeQueen: wraps Claude Code CLI with PipeProcess
/// - CustomQueen: manages workers via API calls with full control stack
#[async_trait]
pub trait Queen: Send + Sync {
    /// Unique identifier for this Queen
    fn id(&self) -> QueenId;

    /// Backend type (for routing and logging)
    fn backend(&self) -> QueenBackend;

    /// Assign a task from SwarmHost
    async fn assign(&mut self, task: Task, context: TaskContext) -> Result<()>;

    /// Get current status
    async fn status(&self) -> QueenStatus;

    /// Get completed result (None if still working)
    async fn result(&self) -> Option<TaskResult>;

    /// Receive a message from SwarmHost or another Queen
    async fn send_message(&mut self, msg: SwarmMessage) -> Result<()>;

    /// Drain outgoing messages (status reports, escalations, knowledge)
    async fn drain_outbox(&mut self) -> Vec<SwarmMessage>;

    /// Check if Queen is still alive/responsive
    async fn is_alive(&self) -> bool;

    /// Graceful shutdown
    async fn shutdown(&mut self) -> Result<()>;
}
