//! Git worktree isolation backend — wraps the existing safety::worktree::WorktreeManager.

use super::{IsolationBackend, MergeOutcome, WorkspaceInfo};
use crate::core::types::{AgentId, QueenId};
use anyhow::{anyhow, Result};
use std::path::PathBuf;

/// Git worktree isolation backend.
///
/// Each agent gets its own git worktree on a separate branch.
/// Wraps `safety::worktree::WorktreeManager`.
pub struct GitWorktreeIsolation {
    manager: crate::safety::worktree::WorktreeManager,
}

impl GitWorktreeIsolation {
    pub fn new(repo_dir: PathBuf, base_branch: Option<&str>) -> Result<Self> {
        let manager = crate::safety::worktree::WorktreeManager::new(&repo_dir, base_branch)?;
        Ok(Self { manager })
    }
}

impl IsolationBackend for GitWorktreeIsolation {
    fn create_workspace(&mut self, agent_id: &AgentId) -> Result<WorkspaceInfo> {
        let queen_id = extract_queen_id(agent_id)?;
        let path = self.manager.create(&queen_id)?;
        let info = self
            .manager
            .get_info(&queen_id)
            .ok_or_else(|| anyhow!("Failed to get worktree info after creation"))?;
        Ok(WorkspaceInfo {
            agent_id: agent_id.clone(),
            path,
            branch: Some(info.branch.clone()),
            created_at: info.created_at,
        })
    }

    fn workspace_path(&self, agent_id: &AgentId) -> Option<PathBuf> {
        let queen_id = extract_queen_id(agent_id).ok()?;
        Some(self.manager.worktree_path(&queen_id))
    }

    fn merge(&mut self, agent_id: &AgentId) -> Result<MergeOutcome> {
        let queen_id = extract_queen_id(agent_id)?;
        let result = self.manager.merge(&queen_id)?;
        Ok(match result {
            crate::safety::worktree::MergeResult::Success { commit_sha } => {
                MergeOutcome::Success { commit_sha }
            }
            crate::safety::worktree::MergeResult::Conflict { files } => {
                MergeOutcome::Conflict { files }
            }
            crate::safety::worktree::MergeResult::NoChanges => MergeOutcome::NoChanges,
        })
    }

    fn cleanup(&mut self, agent_id: &AgentId) -> Result<()> {
        let queen_id = extract_queen_id(agent_id)?;
        self.manager.cleanup(&queen_id)
    }

    fn sync(&mut self, agent_id: &AgentId) -> Result<()> {
        let queen_id = extract_queen_id(agent_id)?;
        let results = self.manager.sync_all_with_base(Some(&queen_id));
        for (id, result) in results {
            match result {
                crate::safety::worktree::SyncResult::Synced => {
                    eprintln!("[IsolationBackend] Synced worktree for {}", id.0);
                }
                crate::safety::worktree::SyncResult::Skipped(reason) => {
                    eprintln!(
                        "[IsolationBackend] Skipped sync for {}: {}",
                        id.0, reason
                    );
                }
                crate::safety::worktree::SyncResult::ConflictAborted => {
                    eprintln!(
                        "[IsolationBackend] Conflict during sync for {}, aborted",
                        id.0
                    );
                }
                crate::safety::worktree::SyncResult::Error(e) => {
                    eprintln!("[IsolationBackend] Error syncing {}: {}", id.0, e);
                }
            }
        }
        Ok(())
    }

    fn active_workspaces(&self) -> Vec<WorkspaceInfo> {
        self.manager
            .list()
            .into_iter()
            .map(|info| WorkspaceInfo {
                agent_id: AgentId::Queen(info.queen_id.clone()),
                path: info.path.clone(),
                branch: Some(info.branch.clone()),
                created_at: info.created_at,
            })
            .collect()
    }

    fn prune(&mut self) -> Result<usize> {
        self.manager.prune()?;
        Ok(0) // WorktreeManager::prune doesn't return count
    }
}

/// No-op isolation backend for single-directory mode (no isolation).
pub struct NoIsolation {
    working_dir: PathBuf,
}

impl NoIsolation {
    pub fn new(working_dir: PathBuf) -> Self {
        Self { working_dir }
    }
}

impl IsolationBackend for NoIsolation {
    fn create_workspace(&mut self, _agent_id: &AgentId) -> Result<WorkspaceInfo> {
        Ok(WorkspaceInfo {
            agent_id: AgentId::Validator, // placeholder
            path: self.working_dir.clone(),
            branch: None,
            created_at: chrono::Utc::now(),
        })
    }

    fn workspace_path(&self, _agent_id: &AgentId) -> Option<PathBuf> {
        Some(self.working_dir.clone())
    }

    fn merge(&mut self, _agent_id: &AgentId) -> Result<MergeOutcome> {
        Ok(MergeOutcome::NoChanges) // No isolation = no merge needed
    }

    fn cleanup(&mut self, _agent_id: &AgentId) -> Result<()> {
        Ok(())
    }

    fn sync(&mut self, _agent_id: &AgentId) -> Result<()> {
        Ok(())
    }

    fn active_workspaces(&self) -> Vec<WorkspaceInfo> {
        vec![]
    }

    fn prune(&mut self) -> Result<usize> {
        Ok(0)
    }
}

fn extract_queen_id(agent_id: &AgentId) -> Result<QueenId> {
    match agent_id {
        AgentId::Queen(id) => Ok(id.clone()),
        other => Err(anyhow!("Expected Queen agent, got {:?}", other)),
    }
}
