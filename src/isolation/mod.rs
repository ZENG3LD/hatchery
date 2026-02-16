//! IsolationBackend trait — abstracts how agents get isolated workspaces.
//!
//! Git worktrees is one implementation. This trait allows swapping between:
//! - Git worktrees (each agent gets its own branch)
//! - No isolation (all agents share the same directory)
//! - Docker containers (future)
//! - Virtual filesystems (future)

use crate::core::types::AgentId;
use anyhow::Result;
use std::path::PathBuf;

pub mod git_worktree;

/// Result of merging isolated work back.
#[derive(Debug, Clone)]
pub enum MergeOutcome {
    /// Merge succeeded with commit SHA.
    Success { commit_sha: String },
    /// Merge conflict on specified files.
    Conflict { files: Vec<PathBuf> },
    /// No changes to merge.
    NoChanges,
}

/// Information about an isolated workspace.
#[derive(Debug, Clone)]
pub struct WorkspaceInfo {
    pub agent_id: AgentId,
    pub path: PathBuf,
    pub branch: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// IsolationBackend defines how agents get isolated workspaces.
pub trait IsolationBackend: Send + Sync {
    /// Create an isolated workspace for an agent.
    fn create_workspace(&mut self, agent_id: &AgentId) -> Result<WorkspaceInfo>;

    /// Get the workspace path for an agent.
    fn workspace_path(&self, agent_id: &AgentId) -> Option<PathBuf>;

    /// Merge an agent's isolated work back to the base.
    fn merge(&mut self, agent_id: &AgentId) -> Result<MergeOutcome>;

    /// Clean up an agent's workspace.
    fn cleanup(&mut self, agent_id: &AgentId) -> Result<()>;

    /// Sync a workspace with the latest base changes.
    fn sync(&mut self, agent_id: &AgentId) -> Result<()>;

    /// List all active workspaces.
    fn active_workspaces(&self) -> Vec<WorkspaceInfo>;

    /// Prune orphaned workspaces.
    fn prune(&mut self) -> Result<usize>;
}
