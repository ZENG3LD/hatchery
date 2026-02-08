//! Git worktree isolation — each worker gets its own worktree on a separate branch.
//!
//! Lifecycle:
//! 1. `create(worker_id)` → `git worktree add .hatchery/worktrees/W{id} -b hatchery/W{id}`
//! 2. Worker operates in the worktree directory
//! 3. `merge(worker_id)` → merge worker branch into base branch
//! 4. `cleanup(worker_id)` → remove worktree and branch
//! 5. `prune()` → clean orphan worktrees at startup

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Manages git worktrees for isolated worker directories.
pub struct WorktreeManager {
    /// The main repository working directory.
    repo_dir: PathBuf,
    /// Base directory for all worktrees: `{repo_dir}/.hatchery/worktrees/`
    worktree_base: PathBuf,
    /// The branch to fork from and merge back into.
    base_branch: String,
}

impl WorktreeManager {
    /// Create a new WorktreeManager.
    ///
    /// `repo_dir` is the main git repository root.
    /// `base_branch` is determined automatically from HEAD if not specified.
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
        })
    }

    /// Create a worktree for a worker. Returns the worktree path.
    ///
    /// Creates branch `hatchery/{worker_tag}` from the base branch.
    /// `worker_tag` is e.g. "W0", "W1", "L2.0.W0".
    pub fn create(&self, worker_tag: &str) -> Result<PathBuf> {
        let wt_path = self.worktree_base.join(worker_tag);
        let branch_name = format!("hatchery/{}", worker_tag);

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
        git_cmd(
            &self.repo_dir,
            &[
                "worktree",
                "add",
                &wt_path.to_string_lossy(),
                "-b",
                &branch_name,
                &self.base_branch,
            ],
        )
        .with_context(|| format!("Failed to create worktree for {}", worker_tag))?;

        Ok(wt_path)
    }

    /// Merge a worker's branch back into the base branch.
    ///
    /// Returns `Ok(true)` if merge succeeded, `Ok(false)` if there was a conflict.
    pub fn merge(&self, worker_tag: &str) -> Result<bool> {
        let branch_name = format!("hatchery/{}", worker_tag);

        // Switch to base branch (we're in the main repo dir)
        // First, check we're on the right branch
        let current = detect_current_branch(&self.repo_dir)?;
        if current != self.base_branch {
            git_cmd(&self.repo_dir, &["checkout", &self.base_branch])
                .context("Failed to checkout base branch for merge")?;
        }

        // Attempt merge with --no-ff for clear history
        let output = Command::new("git")
            .args([
                "merge",
                "--no-ff",
                &branch_name,
                "-m",
                &format!("merge(hatchery): {}", worker_tag),
            ])
            .current_dir(&self.repo_dir)
            .output()
            .context("Failed to run git merge")?;

        if output.status.success() {
            Ok(true)
        } else {
            // Merge conflict — abort and report
            let _ = git_cmd(&self.repo_dir, &["merge", "--abort"]);
            let stderr = String::from_utf8_lossy(&output.stderr);
            eprintln!(
                "[SAFETY] Merge conflict for {}: {}",
                worker_tag,
                stderr.trim()
            );
            Ok(false)
        }
    }

    /// Remove a worker's worktree and delete its branch.
    pub fn cleanup(&self, worker_tag: &str) -> Result<()> {
        let wt_path = self.worktree_base.join(worker_tag);
        let branch_name = format!("hatchery/{}", worker_tag);

        // Remove worktree
        if wt_path.exists() {
            self.remove_worktree(&wt_path)?;
        }

        // Delete branch (may fail if not merged — use -D to force)
        let _ = git_cmd(&self.repo_dir, &["branch", "-D", &branch_name]);

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

    /// Get the worktree path for a worker.
    pub fn worktree_path(&self, worker_tag: &str) -> PathBuf {
        self.worktree_base.join(worker_tag)
    }

    /// Internal: remove a worktree directory.
    fn remove_worktree(&self, wt_path: &Path) -> Result<()> {
        // Try git worktree remove first
        let result = git_cmd(
            &self.repo_dir,
            &["worktree", "remove", "--force", &wt_path.to_string_lossy()],
        );

        if result.is_err() {
            // Fallback: manual removal + prune
            let _ = std::fs::remove_dir_all(wt_path);
            let _ = git_cmd(&self.repo_dir, &["worktree", "prune"]);
        }

        Ok(())
    }
}

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
        let mgr = WorktreeManager::new(&repo, None).unwrap();

        // Create worktree
        let wt_path = mgr.create("W0").unwrap();
        assert!(wt_path.exists());
        assert!(wt_path.join("README.md").exists());

        // Branch should exist
        let output = Command::new("git")
            .args(["branch", "--list", "hatchery/W0"])
            .current_dir(&repo)
            .output()
            .unwrap();
        let branches = String::from_utf8_lossy(&output.stdout);
        assert!(branches.contains("hatchery/W0"));

        // Cleanup
        mgr.cleanup("W0").unwrap();
        assert!(!wt_path.exists());
    }

    #[test]
    fn test_create_merge_cleanup() {
        let (_dir, repo) = temp_repo();
        let mgr = WorktreeManager::new(&repo, None).unwrap();

        // Create worktree
        let wt_path = mgr.create("W1").unwrap();

        // Make a change in the worktree
        fs::write(wt_path.join("new_file.txt"), "hello from W1").unwrap();
        git_cmd(&wt_path, &["add", "."]).unwrap();
        git_cmd(&wt_path, &["commit", "-m", "W1: add file"]).unwrap();

        // Merge back
        let merged = mgr.merge("W1").unwrap();
        assert!(merged);

        // Verify the file exists in main repo
        assert!(repo.join("new_file.txt").exists());

        // Cleanup
        mgr.cleanup("W1").unwrap();
    }

    #[test]
    fn test_prune() {
        let (_dir, repo) = temp_repo();
        let mgr = WorktreeManager::new(&repo, None).unwrap();

        // Prune should not error on clean repo
        mgr.prune().unwrap();
    }

    #[test]
    fn test_merge_conflict_returns_false() {
        let (_dir, repo) = temp_repo();
        let mgr = WorktreeManager::new(&repo, None).unwrap();

        // Create worktree
        let wt_path = mgr.create("W2").unwrap();

        // Change same file in both main and worktree
        fs::write(repo.join("README.md"), "# Main change").unwrap();
        git_cmd(&repo, &["add", "."]).unwrap();
        git_cmd(&repo, &["commit", "-m", "main: edit readme"]).unwrap();

        fs::write(wt_path.join("README.md"), "# Worker change").unwrap();
        git_cmd(&wt_path, &["add", "."]).unwrap();
        git_cmd(&wt_path, &["commit", "-m", "W2: edit readme"]).unwrap();

        // Merge should detect conflict
        let merged = mgr.merge("W2").unwrap();
        assert!(!merged);

        // Cleanup
        mgr.cleanup("W2").unwrap();
    }
}
