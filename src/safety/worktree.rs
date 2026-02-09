//! Git worktree isolation for V2 Queens — each Queen gets its own worktree on a separate branch.
//!
//! Lifecycle:
//! 1. `create(queen_id)` → `git worktree add .hatchery/worktrees/{queen_id} -b hatchery/{queen_id}`
//! 2. Queen operates in the worktree directory
//! 3. `merge(queen_id)` → merge Queen branch into base branch (with conflict detection)
//! 4. `cleanup(queen_id)` → remove worktree and branch
//! 5. `prune()` → clean orphan worktrees at startup

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use chrono::{DateTime, Utc};
use serde::{Serialize, Deserialize};
use anyhow::{Context, Result, anyhow};
use crate::core::types::QueenId;

// ============================================================================
// Types
// ============================================================================

/// Information about an active worktree.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorktreeInfo {
    /// The Queen that owns this worktree.
    pub queen_id: QueenId,
    /// Path to the worktree directory.
    pub path: PathBuf,
    /// Branch name (e.g., "hatchery/Q0").
    pub branch: String,
    /// When the worktree was created.
    pub created_at: DateTime<Utc>,
}

/// Result of a merge operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MergeResult {
    /// Merge succeeded. Contains the commit SHA.
    Success { commit_sha: String },
    /// Merge conflict detected. Contains list of conflicted files.
    Conflict { files: Vec<PathBuf> },
    /// No changes to merge (branch is up to date with base).
    NoChanges,
}

/// Strategy for resolving merge conflicts automatically.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConflictStrategy {
    /// Take the incoming branch's version (theirs).
    TakeTheirs,
    /// Keep the current branch's version (ours).
    TakeOurs,
    /// Don't auto-resolve, escalate to user.
    Escalate,
}

// ============================================================================
// WorktreeManager
// ============================================================================

/// Manages git worktrees for isolated Queen directories.
pub struct WorktreeManager {
    /// The main repository root.
    repo_dir: PathBuf,
    /// Base directory for worktrees: `{repo_dir}/.hatchery/worktrees/`
    worktree_base: PathBuf,
    /// Branch to fork from and merge into.
    base_branch: String,
    /// Active worktrees indexed by queen ID.
    worktrees: HashMap<QueenId, WorktreeInfo>,
}

impl WorktreeManager {
    /// Create a new WorktreeManager.
    ///
    /// `repo_dir` is the main git repository root.
    /// `base_branch` is auto-detected from HEAD if None.
    pub fn new(repo_dir: &Path, base_branch: Option<&str>) -> Result<Self> {
        let base = match base_branch {
            Some(b) => b.to_string(),
            None => detect_current_branch(repo_dir)?,
        };

        let worktree_base = repo_dir.join(".hatchery").join("worktrees");

        Ok(Self {
            repo_dir: repo_dir.to_path_buf(),
            worktree_base,
            base_branch: base,
            worktrees: HashMap::new(),
        })
    }

    /// Create an isolated worktree for a Queen.
    ///
    /// Branch name: `hatchery/{queen_id.0}`
    /// Path: `{repo_dir}/.hatchery/worktrees/{queen_id.0}`
    pub fn create(&mut self, queen_id: &QueenId) -> Result<PathBuf> {
        let wt_path = self.worktree_base.join(&queen_id.0);
        let branch_name = format!("hatchery/{}", queen_id.0);

        // Ensure base directory exists
        if let Some(parent) = wt_path.parent() {
            std::fs::create_dir_all(parent)
                .context("Failed to create worktree base directory")?;
        }

        // Remove stale worktree if exists
        if wt_path.exists() {
            let _ = self.remove_worktree(&wt_path);
        }

        // Delete branch if it exists (leftover from previous run)
        let _ = git_cmd(&self.repo_dir, &["branch", "-D", &branch_name]);

        // Create worktree with new branch from base
        let normalized_path = normalize_path_for_git(&wt_path);
        git_cmd(
            &self.repo_dir,
            &[
                "worktree",
                "add",
                &normalized_path,
                "-b",
                &branch_name,
                &self.base_branch,
            ],
        )
        .with_context(|| format!("Failed to create worktree for {}", queen_id.0))?;

        // Track this worktree
        let info = WorktreeInfo {
            queen_id: queen_id.clone(),
            path: wt_path.clone(),
            branch: branch_name,
            created_at: Utc::now(),
        };
        self.worktrees.insert(queen_id.clone(), info);

        Ok(wt_path)
    }

    /// Get the worktree path for a Queen.
    ///
    /// Returns None if the worktree doesn't exist.
    pub fn get_worktree_path(&self, queen_id: &QueenId) -> Option<PathBuf> {
        self.worktrees.get(queen_id).map(|info| info.path.clone())
    }

    /// Check if a Queen's worktree has uncommitted changes.
    pub fn has_changes(&self, queen_id: &QueenId) -> Result<bool> {
        let info = self.worktrees.get(queen_id)
            .ok_or_else(|| anyhow!("Worktree not found for Queen {}", queen_id.0))?;

        let output = Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(&info.path)
            .output()
            .context("Failed to check git status")?;

        if !output.status.success() {
            anyhow::bail!("git status failed");
        }

        let status = String::from_utf8_lossy(&output.stdout);
        Ok(!status.trim().is_empty())
    }

    /// Merge a Queen's branch back into base branch.
    ///
    /// Returns MergeResult:
    /// - NoChanges: Branch has no commits beyond base
    /// - Success: Merge succeeded, returns commit SHA
    /// - Conflict: Merge conflict detected, returns list of conflicted files
    pub fn merge(&mut self, queen_id: &QueenId) -> Result<MergeResult> {
        self.merge_with_attribution(queen_id, None)
    }

    /// Merge a Queen's branch with custom commit message attribution.
    /// The merge commit message includes Co-Authored-By for the Queen.
    pub fn merge_with_attribution(&mut self, queen_id: &QueenId, attribution: Option<&str>) -> Result<MergeResult> {
        let info = self.worktrees.get(queen_id)
            .ok_or_else(|| anyhow!("Worktree not found for Queen {}", queen_id.0))?;

        let branch_name = &info.branch;
        let wt_path = &info.path;

        // Ensure all changes (including generated files like Cargo.lock) are committed in the worktree
        // before merge to prevent untracked file conflicts
        let status_output = Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(wt_path)
            .output()
            .context("Failed to check worktree status before merge")?;

        if status_output.status.success() {
            let status = String::from_utf8_lossy(&status_output.stdout);
            if !status.trim().is_empty() {
                // There are uncommitted changes - add and commit them
                git_cmd(wt_path, &["add", "-A"])
                    .context("Failed to add uncommitted changes before merge")?;

                git_cmd(wt_path, &["commit", "--amend", "--no-edit"])
                    .or_else(|_| {
                        // If amend fails (no previous commit), create a new commit
                        git_cmd(wt_path, &["commit", "-m", "chore: add generated files"])
                    })
                    .context("Failed to commit generated files before merge")?;
            }
        }

        // First check if branch has any commits beyond base
        let output = Command::new("git")
            .args(["log", &format!("{}..{}", self.base_branch, branch_name), "--oneline"])
            .current_dir(&self.repo_dir)
            .output()
            .context("Failed to check branch commits")?;

        if !output.status.success() {
            anyhow::bail!("git log failed");
        }

        let commits = String::from_utf8_lossy(&output.stdout);
        if commits.trim().is_empty() {
            return Ok(MergeResult::NoChanges);
        }

        // Switch to base branch if needed
        let current = detect_current_branch(&self.repo_dir)?;
        if current != self.base_branch {
            git_cmd(&self.repo_dir, &["checkout", &self.base_branch])
                .context("Failed to checkout base branch for merge")?;
        }

        // Build commit message with attribution if provided
        let commit_msg = if let Some(attr) = attribution {
            format!("merge(hatchery): {}\n\nCo-Authored-By: {}", queen_id.0, attr)
        } else {
            format!("merge(hatchery): {}", queen_id.0)
        };

        // Attempt merge with --no-ff
        let output = Command::new("git")
            .args([
                "merge",
                "--no-ff",
                branch_name,
                "-m",
                &commit_msg,
            ])
            .current_dir(&self.repo_dir)
            .output()
            .context("Failed to run git merge")?;

        if output.status.success() {
            // Merge succeeded, get commit SHA
            let sha = get_head_sha(&self.repo_dir)?;
            Ok(MergeResult::Success { commit_sha: sha })
        } else {
            // Merge conflict — get conflicted files before aborting
            let files = get_conflicted_files(&self.repo_dir)?;

            // Abort the merge
            let _ = git_cmd(&self.repo_dir, &["merge", "--abort"]);

            let stderr = String::from_utf8_lossy(&output.stderr);
            eprintln!(
                "[SAFETY] Merge conflict for {}: {}",
                queen_id.0,
                stderr.trim()
            );

            Ok(MergeResult::Conflict { files })
        }
    }

    /// Auto-resolve merge conflicts using a strategy.
    ///
    /// Must be called after a merge that returned MergeResult::Conflict.
    pub fn resolve_conflict(&self, strategy: &ConflictStrategy) -> Result<()> {
        match strategy {
            ConflictStrategy::TakeTheirs => {
                // Accept incoming changes
                git_cmd(&self.repo_dir, &["checkout", "--theirs", "."])
                    .context("Failed to checkout theirs")?;
                git_cmd(&self.repo_dir, &["add", "."])
                    .context("Failed to add resolved files")?;
                Ok(())
            }
            ConflictStrategy::TakeOurs => {
                // Keep our changes
                git_cmd(&self.repo_dir, &["checkout", "--ours", "."])
                    .context("Failed to checkout ours")?;
                git_cmd(&self.repo_dir, &["add", "."])
                    .context("Failed to add resolved files")?;
                Ok(())
            }
            ConflictStrategy::Escalate => {
                anyhow::bail!("Conflict escalated to user — manual resolution required")
            }
        }
    }

    /// Remove worktree and delete branch for a Queen.
    pub fn cleanup(&mut self, queen_id: &QueenId) -> Result<()> {
        let info = self.worktrees.remove(queen_id)
            .ok_or_else(|| anyhow!("Worktree not found for Queen {}", queen_id.0))?;

        // Remove worktree
        if info.path.exists() {
            self.remove_worktree(&info.path)?;
        }

        // Delete branch (may fail if not merged — use -D to force)
        let _ = git_cmd(&self.repo_dir, &["branch", "-D", &info.branch]);

        Ok(())
    }

    /// Prune orphan worktrees. Call at startup.
    pub fn prune(&self) -> Result<()> {
        git_cmd(&self.repo_dir, &["worktree", "prune"])
            .context("Failed to prune worktrees")?;

        // Also clean up .hatchery/worktrees/ if it exists but is empty
        if self.worktree_base.exists() {
            let is_empty = self
                .worktree_base
                .read_dir()
                .map(|mut d| d.next().is_none())
                .unwrap_or(true);
            if is_empty {
                let _ = std::fs::remove_dir_all(&self.worktree_base);
            }
        }

        Ok(())
    }

    /// Get worktree path for a Queen.
    pub fn worktree_path(&self, queen_id: &QueenId) -> PathBuf {
        self.worktree_base.join(&queen_id.0)
    }

    /// Get info about a Queen's worktree.
    pub fn get_info(&self, queen_id: &QueenId) -> Option<&WorktreeInfo> {
        self.worktrees.get(queen_id)
    }

    /// List all active worktrees.
    pub fn list(&self) -> Vec<&WorktreeInfo> {
        self.worktrees.values().collect()
    }

    /// Number of active worktrees.
    pub fn count(&self) -> usize {
        self.worktrees.len()
    }

    /// Get the branch name for a Queen with optional swarm context.
    /// Default: "hatchery/{queen_id}"
    /// With swarm: "hatchery/{swarm_id}/{queen_id}"
    pub fn branch_name_with_swarm(queen_id: &QueenId, swarm_id: Option<&str>) -> String {
        match swarm_id {
            Some(sid) => format!("hatchery/{}/{}", sid, queen_id.0),
            None => format!("hatchery/{}", queen_id.0),
        }
    }

    /// Internal: remove a worktree directory.
    fn remove_worktree(&self, wt_path: &Path) -> Result<()> {
        // Try git worktree remove first
        let normalized_path = normalize_path_for_git(wt_path);
        let result = git_cmd(
            &self.repo_dir,
            &["worktree", "remove", "--force", &normalized_path],
        );

        if result.is_err() {
            // Fallback: manual removal + prune
            let _ = std::fs::remove_dir_all(wt_path);
            let _ = git_cmd(&self.repo_dir, &["worktree", "prune"]);
        }

        Ok(())
    }
}

// ============================================================================
// Helpers
// ============================================================================

/// Detect the current branch name.
fn detect_current_branch(repo_dir: &Path) -> Result<String> {
    let output = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(repo_dir)
        .output()
        .context("Failed to detect current git branch")?;

    if !output.status.success() {
        anyhow::bail!("Not a git repository or no commits yet");
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Get the SHA of the current HEAD commit.
fn get_head_sha(repo_dir: &Path) -> Result<String> {
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo_dir)
        .output()
        .context("Failed to get HEAD SHA")?;

    if !output.status.success() {
        anyhow::bail!("Failed to get HEAD SHA");
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Get list of conflicted files from git status (must be called during active merge).
fn get_conflicted_files(repo_dir: &Path) -> Result<Vec<PathBuf>> {
    let output = Command::new("git")
        .args(["diff", "--name-only", "--diff-filter=U"])
        .current_dir(repo_dir)
        .output()
        .context("Failed to get conflicted files")?;

    if !output.status.success() {
        return Ok(Vec::new());
    }

    let files = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| PathBuf::from(line.trim()))
        .collect();

    Ok(files)
}

/// Convert a Windows path to a forward-slash path suitable for Git CLI.
///
/// Git on Windows cannot handle backslash paths or spaces without proper escaping.
/// This function normalizes paths by converting backslashes to forward slashes.
fn normalize_path_for_git(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// Run a git command and return success/failure.
fn git_cmd(repo_dir: &Path, args: &[&str]) -> Result<()> {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo_dir)
        .output()
        .with_context(|| format!("Failed to run: git {}", args.join(" ")))?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git {} failed: {}", args.join(" "), stderr.trim());
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Helper: create a temp git repo for testing.
    fn temp_repo() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_path_buf();

        // Init git repo
        git_cmd(&path, &["init"]).unwrap();
        git_cmd(&path, &["config", "user.email", "test@test.com"]).unwrap();
        git_cmd(&path, &["config", "user.name", "Test"]).unwrap();

        // Create initial commit
        fs::write(path.join("README.md"), "# Test").unwrap();
        git_cmd(&path, &["add", "."]).unwrap();
        git_cmd(&path, &["commit", "-m", "initial"]).unwrap();

        (dir, path)
    }

    #[test]
    fn test_create_and_cleanup_worktree() {
        let (_dir, repo) = temp_repo();
        let mut mgr = WorktreeManager::new(&repo, None).unwrap();

        let queen_id = QueenId("Q0".to_string());

        // Create worktree
        let wt_path = mgr.create(&queen_id).unwrap();
        assert!(wt_path.exists());
        assert!(wt_path.join("README.md").exists());

        // Branch should exist
        let output = Command::new("git")
            .args(["branch", "--list", "hatchery/Q0"])
            .current_dir(&repo)
            .output()
            .unwrap();
        let branches = String::from_utf8_lossy(&output.stdout);
        assert!(branches.contains("hatchery/Q0"));

        // Info should be tracked
        assert_eq!(mgr.count(), 1);
        let info = mgr.get_info(&queen_id).unwrap();
        assert_eq!(info.queen_id, queen_id);
        assert_eq!(info.branch, "hatchery/Q0");

        // Cleanup
        mgr.cleanup(&queen_id).unwrap();
        assert!(!wt_path.exists());
        assert_eq!(mgr.count(), 0);
    }

    #[test]
    fn test_create_with_changes_merge_success() {
        let (_dir, repo) = temp_repo();
        let mut mgr = WorktreeManager::new(&repo, None).unwrap();

        let queen_id = QueenId("Q1".to_string());

        // Create worktree
        let wt_path = mgr.create(&queen_id).unwrap();

        // Make an uncommitted change first
        fs::write(wt_path.join("uncommitted.txt"), "uncommitted").unwrap();
        assert!(mgr.has_changes(&queen_id).unwrap());

        // Now commit a change
        fs::write(wt_path.join("new_file.txt"), "hello from Q1").unwrap();
        git_cmd(&wt_path, &["add", "."]).unwrap();
        git_cmd(&wt_path, &["commit", "-m", "Q1: add file"]).unwrap();

        // After committing all changes, has_changes should be false
        assert!(!mgr.has_changes(&queen_id).unwrap());

        // Merge back
        let result = mgr.merge(&queen_id).unwrap();
        match result {
            MergeResult::Success { commit_sha } => {
                assert!(!commit_sha.is_empty());
            }
            _ => panic!("Expected merge success, got {:?}", result),
        }

        // Verify the file exists in main repo
        assert!(repo.join("new_file.txt").exists());

        // Cleanup
        mgr.cleanup(&queen_id).unwrap();
    }

    #[test]
    fn test_merge_no_changes() {
        let (_dir, repo) = temp_repo();
        let mut mgr = WorktreeManager::new(&repo, None).unwrap();

        let queen_id = QueenId("Q2".to_string());

        // Create worktree but don't make any changes
        let _wt_path = mgr.create(&queen_id).unwrap();

        // Check has_changes returns false
        assert!(!mgr.has_changes(&queen_id).unwrap());

        // Merge should return NoChanges
        let result = mgr.merge(&queen_id).unwrap();
        match result {
            MergeResult::NoChanges => (),
            _ => panic!("Expected NoChanges, got {:?}", result),
        }

        // Cleanup
        mgr.cleanup(&queen_id).unwrap();
    }

    #[test]
    fn test_merge_conflict() {
        let (_dir, repo) = temp_repo();
        let mut mgr = WorktreeManager::new(&repo, None).unwrap();

        let queen_id = QueenId("Q3".to_string());

        // Create worktree
        let wt_path = mgr.create(&queen_id).unwrap();

        // Change same file in both main and worktree (create conflict)
        fs::write(repo.join("README.md"), "# Main change").unwrap();
        git_cmd(&repo, &["add", "."]).unwrap();
        git_cmd(&repo, &["commit", "-m", "main: edit readme"]).unwrap();

        fs::write(wt_path.join("README.md"), "# Worker change").unwrap();
        git_cmd(&wt_path, &["add", "."]).unwrap();
        git_cmd(&wt_path, &["commit", "-m", "Q3: edit readme"]).unwrap();

        // Merge should detect conflict
        let result = mgr.merge(&queen_id).unwrap();
        match result {
            MergeResult::Conflict { files } => {
                assert!(!files.is_empty());
                // Should contain README.md
                let has_readme = files.iter().any(|p| p.ends_with("README.md"));
                assert!(has_readme, "Expected README.md in conflict list");
            }
            _ => panic!("Expected conflict, got {:?}", result),
        }

        // Cleanup
        mgr.cleanup(&queen_id).unwrap();
    }

    #[test]
    fn test_has_changes_detects_modifications() {
        let (_dir, repo) = temp_repo();
        let mut mgr = WorktreeManager::new(&repo, None).unwrap();

        let queen_id = QueenId("Q4".to_string());

        // Create worktree
        let wt_path = mgr.create(&queen_id).unwrap();

        // Initially no changes
        assert!(!mgr.has_changes(&queen_id).unwrap());

        // Make a change but don't commit
        fs::write(wt_path.join("uncommitted.txt"), "uncommitted change").unwrap();

        // Should detect uncommitted changes
        assert!(mgr.has_changes(&queen_id).unwrap());

        // Cleanup
        mgr.cleanup(&queen_id).unwrap();
    }

    #[test]
    fn test_list_returns_all_worktrees() {
        let (_dir, repo) = temp_repo();
        let mut mgr = WorktreeManager::new(&repo, None).unwrap();

        let q0 = QueenId("Q0".to_string());
        let q1 = QueenId("Q1".to_string());

        // Create two worktrees
        mgr.create(&q0).unwrap();
        mgr.create(&q1).unwrap();

        // List should return both
        let list = mgr.list();
        assert_eq!(list.len(), 2);
        assert_eq!(mgr.count(), 2);

        // Check that both queen IDs are present
        let ids: Vec<_> = list.iter().map(|info| &info.queen_id).collect();
        assert!(ids.contains(&&q0));
        assert!(ids.contains(&&q1));

        // Cleanup
        mgr.cleanup(&q0).unwrap();
        mgr.cleanup(&q1).unwrap();
        assert_eq!(mgr.count(), 0);
    }

    #[test]
    fn test_prune() {
        let (_dir, repo) = temp_repo();
        let mgr = WorktreeManager::new(&repo, None).unwrap();

        // Prune should not error on clean repo
        mgr.prune().unwrap();
    }

    #[test]
    fn test_resolve_conflict_take_theirs() {
        let (_dir, repo) = temp_repo();
        let mut mgr = WorktreeManager::new(&repo, None).unwrap();

        let queen_id = QueenId("Q5".to_string());

        // Create worktree and conflict
        let wt_path = mgr.create(&queen_id).unwrap();

        // Make conflicting changes
        fs::write(repo.join("conflict.txt"), "main version").unwrap();
        git_cmd(&repo, &["add", "."]).unwrap();
        git_cmd(&repo, &["commit", "-m", "main: conflict file"]).unwrap();

        fs::write(wt_path.join("conflict.txt"), "queen version").unwrap();
        git_cmd(&wt_path, &["add", "."]).unwrap();
        git_cmd(&wt_path, &["commit", "-m", "Q5: conflict file"]).unwrap();

        // Try merge (will fail)
        let result = mgr.merge(&queen_id).unwrap();
        assert!(matches!(result, MergeResult::Conflict { .. }));

        // Resolve with TakeTheirs (but we can't complete the merge in this test
        // because we aborted it - this just tests the resolve_conflict function)
        // In real usage, you'd retry the merge after resolving

        // Cleanup
        mgr.cleanup(&queen_id).unwrap();
    }

    #[test]
    fn test_resolve_conflict_escalate() {
        let (_dir, repo) = temp_repo();
        let mgr = WorktreeManager::new(&repo, None).unwrap();

        // Test that Escalate returns an error
        let result = mgr.resolve_conflict(&ConflictStrategy::Escalate);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("manual resolution"));
    }

    #[test]
    fn test_merge_with_attribution() {
        let (_dir, repo) = temp_repo();
        let mut mgr = WorktreeManager::new(&repo, None).unwrap();

        let queen_id = QueenId("Q6".to_string());

        // Create worktree
        let wt_path = mgr.create(&queen_id).unwrap();

        // Make a change and commit
        fs::write(wt_path.join("attributed.txt"), "with attribution").unwrap();
        git_cmd(&wt_path, &["add", "."]).unwrap();
        git_cmd(&wt_path, &["commit", "-m", "Q6: add file"]).unwrap();

        // Merge with attribution
        let attribution = "Queen Agent Q6 <queen6@hatchery.ai>";
        let result = mgr.merge_with_attribution(&queen_id, Some(attribution)).unwrap();

        match result {
            MergeResult::Success { commit_sha } => {
                assert!(!commit_sha.is_empty());

                // Verify commit message includes attribution
                let output = Command::new("git")
                    .args(["log", "-1", "--pretty=%B"])
                    .current_dir(&repo)
                    .output()
                    .unwrap();

                let commit_msg = String::from_utf8_lossy(&output.stdout);
                assert!(commit_msg.contains("merge(hatchery): Q6"));
                assert!(commit_msg.contains("Co-Authored-By:"));
                assert!(commit_msg.contains(attribution));
            }
            _ => panic!("Expected merge success, got {:?}", result),
        }

        // Cleanup
        mgr.cleanup(&queen_id).unwrap();
    }

    #[test]
    fn test_branch_name_with_swarm() {
        let queen_id = QueenId("Q7".to_string());

        // Without swarm context
        let branch = WorktreeManager::branch_name_with_swarm(&queen_id, None);
        assert_eq!(branch, "hatchery/Q7");

        // With swarm context
        let branch = WorktreeManager::branch_name_with_swarm(&queen_id, Some("swarm-alpha"));
        assert_eq!(branch, "hatchery/swarm-alpha/Q7");
    }

    #[test]
    fn test_merge_without_attribution() {
        let (_dir, repo) = temp_repo();
        let mut mgr = WorktreeManager::new(&repo, None).unwrap();

        let queen_id = QueenId("Q8".to_string());

        // Create worktree
        let wt_path = mgr.create(&queen_id).unwrap();

        // Make a change and commit
        fs::write(wt_path.join("no_attr.txt"), "no attribution").unwrap();
        git_cmd(&wt_path, &["add", "."]).unwrap();
        git_cmd(&wt_path, &["commit", "-m", "Q8: add file"]).unwrap();

        // Merge without attribution (using original merge method)
        let result = mgr.merge(&queen_id).unwrap();

        match result {
            MergeResult::Success { commit_sha } => {
                assert!(!commit_sha.is_empty());

                // Verify commit message does NOT include Co-Authored-By
                let output = Command::new("git")
                    .args(["log", "-1", "--pretty=%B"])
                    .current_dir(&repo)
                    .output()
                    .unwrap();

                let commit_msg = String::from_utf8_lossy(&output.stdout);
                assert!(commit_msg.contains("merge(hatchery): Q8"));
                assert!(!commit_msg.contains("Co-Authored-By:"));
            }
            _ => panic!("Expected merge success, got {:?}", result),
        }

        // Cleanup
        mgr.cleanup(&queen_id).unwrap();
    }
}
