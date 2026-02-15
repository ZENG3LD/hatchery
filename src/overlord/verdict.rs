//! Verdict orchestrator for hybrid Overlord review pipeline.
//!
//! Phase 3 of the Overlord pipeline: orchestrate parsing and code checks,
//! then decide whether to auto-approve, auto-reject, or send to LLM review.

use std::path::Path;
use anyhow::{Result, Context};
use tokio::process::Command;

use super::parsers::{
    parse_diff_summary, parse_test_results, scan_code_quality, build_session_summary,
    DiffSummary, TestResults, QualityScan, SessionSummary,
};
use super::code_checks::{run_code_checks, CodeCheckVerdict};

/// Final verdict from the hybrid review pipeline
#[derive(Debug, Clone, PartialEq)]
pub enum OverlordVerdict {
    /// Auto-approve without LLM review
    Approve,
    /// Auto-reject with reason
    Reject { reason: String },
}

/// Full review result with all parsed data
#[derive(Debug)]
pub struct HybridReviewResult {
    pub verdict: OverlordVerdict,
    pub diff: DiffSummary,
    pub tests: Option<TestResults>,
    pub quality: QualityScan,
    pub session: SessionSummary,
    pub code_check_verdict: CodeCheckVerdict,
}

/// Run the hybrid review pipeline.
///
/// Steps:
/// 1. Run `git diff --numstat <base>` → parse into DiffSummary
/// 2. If verify_cmd provided, run it → parse into TestResults
/// 3. Run `git diff <base>` → scan for quality issues
/// 4. Build SessionSummary from params
/// 5. Run deterministic code checks
/// 6. Map verdict:
///    - AllClear → Approve
///    - HardReject → Reject
///    - NeedsReview → Reject (for now; Phase 3.2 will add LLM forwarding)
///
/// # Arguments
/// - `worktree_path`: Path to the git worktree to review
/// - `base_branch`: Base branch to compare against (e.g., "main")
/// - `verify_cmd`: Optional verification command to run (e.g., "cargo test")
/// - `task_description`: Description of the task for context
/// - `duration_secs`: How long the Queen worked on this task
/// - `cost_usd`: Cost in USD for this task
/// - `turns`: Number of turns the Queen took
///
/// # Errors
/// Returns an error if git commands fail or parsing fails.
pub async fn run_hybrid_review(
    worktree_path: &Path,
    base_branch: &str,
    verify_cmd: Option<&str>,
    task_description: &str,
    duration_secs: f64,
    cost_usd: f64,
    turns: usize,
) -> Result<HybridReviewResult> {
    // Step 1: Get diff summary (numstat for line counts)
    let diff = {
        let output = Command::new("git")
            .arg("diff")
            .arg("--numstat")
            .arg(base_branch)
            .current_dir(worktree_path)
            .output()
            .await
            .context("Failed to run git diff --numstat")?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        parse_diff_summary(&stdout)
    };

    // Step 2: Run verify_cmd if provided
    let tests = if let Some(cmd) = verify_cmd {
        let output = if cfg!(windows) {
            Command::new("cmd")
                .arg("/C")
                .arg(cmd)
                .current_dir(worktree_path)
                .output()
                .await
        } else {
            Command::new("sh")
                .arg("-c")
                .arg(cmd)
                .current_dir(worktree_path)
                .output()
                .await
        };

        match output {
            Ok(out) => {
                let combined = format!(
                    "{}\n{}",
                    String::from_utf8_lossy(&out.stdout),
                    String::from_utf8_lossy(&out.stderr)
                );
                Some(parse_test_results(&combined))
            }
            Err(e) => {
                eprintln!("[HybridReview] WARNING: Failed to run verify_cmd: {}", e);
                None
            }
        }
    } else {
        None
    };

    // Step 3: Scan code quality (full diff)
    let quality = {
        let output = Command::new("git")
            .arg("diff")
            .arg(base_branch)
            .current_dir(worktree_path)
            .output()
            .await
            .context("Failed to run git diff")?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        scan_code_quality(&stdout)
    };

    // Step 4: Build session summary
    let session = build_session_summary(
        duration_secs,
        cost_usd,
        turns,
        vec![], // Tools used - not available here, would need to pass from Queen event
        diff.total_files,
    );

    // Step 5: Run deterministic code checks
    let code_check_verdict = run_code_checks(
        &diff,
        tests.as_ref(),
        &quality,
        &session,
        task_description,
    );

    // Step 6: Map verdict
    let verdict = match &code_check_verdict {
        CodeCheckVerdict::AllClear => OverlordVerdict::Approve,
        CodeCheckVerdict::HardReject { reason } => OverlordVerdict::Reject {
            reason: reason.clone(),
        },
        CodeCheckVerdict::NeedsReview { report } => {
            // Default: reject with report summary. Queen reads the reason and fixes.
            // LLM Overlord review is NOT triggered automatically —
            // only Overmind can explicitly request LLM review via escalation.
            OverlordVerdict::Reject {
                reason: format!("needs review: {}", report.quality_summary),
            }
        }
    };

    Ok(HybridReviewResult {
        verdict,
        diff,
        tests,
        quality,
        session,
        code_check_verdict,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use tempfile::TempDir;

    // Helper to create a test git repo with a branch
    async fn setup_test_repo() -> Result<(TempDir, PathBuf)> {
        let temp = TempDir::new()?;
        let repo_path = temp.path().to_path_buf();

        // Initialize git repo
        Command::new("git")
            .arg("init")
            .current_dir(&repo_path)
            .output()
            .await?;

        // Configure git
        Command::new("git")
            .args(&["config", "user.email", "test@example.com"])
            .current_dir(&repo_path)
            .output()
            .await?;

        Command::new("git")
            .args(&["config", "user.name", "Test User"])
            .current_dir(&repo_path)
            .output()
            .await?;

        // Create main branch with initial commit
        fs::write(repo_path.join("README.md"), "Initial")?;
        Command::new("git")
            .args(&["add", "README.md"])
            .current_dir(&repo_path)
            .output()
            .await?;

        Command::new("git")
            .args(&["commit", "-m", "Initial commit"])
            .current_dir(&repo_path)
            .output()
            .await?;

        Ok((temp, repo_path))
    }

    #[tokio::test]
    async fn test_hybrid_review_empty_diff() -> Result<()> {
        let (_temp, repo_path) = setup_test_repo().await?;

        // No changes - should get HardReject for empty work
        let result = run_hybrid_review(
            &repo_path,
            "HEAD",
            None,
            "Test task",
            10.0,
            0.01,
            5,
        ).await?;

        match result.verdict {
            OverlordVerdict::Reject { reason } => {
                assert!(reason.contains("empty work"));
            }
            _ => panic!("Expected Reject for empty diff"),
        }

        Ok(())
    }

    #[tokio::test]
    async fn test_hybrid_review_clean_changes() -> Result<()> {
        let (_temp, repo_path) = setup_test_repo().await?;

        // Add clean changes
        fs::write(repo_path.join("src.rs"), "pub fn add(a: u32, b: u32) -> u32 { a + b }")?;
        Command::new("git")
            .args(&["add", "src.rs"])
            .current_dir(&repo_path)
            .output()
            .await?;

        // Should auto-approve (no tests, no quality issues)
        let result = run_hybrid_review(
            &repo_path,
            "HEAD",
            None,
            "Add function",
            10.0,
            0.01,
            5,
        ).await?;

        match result.verdict {
            OverlordVerdict::Approve => {
                assert!(result.diff.total_added > 0);
            }
            _ => panic!("Expected Approve for clean changes"),
        }

        Ok(())
    }

    #[tokio::test]
    async fn test_hybrid_review_with_stubs() -> Result<()> {
        let (_temp, repo_path) = setup_test_repo().await?;

        // Add changes with stubs
        let stub_code = r#"
// TODO: implement this
fn placeholder() {}
fn stub_function() { unimplemented!() }
"#;
        fs::write(repo_path.join("src.rs"), stub_code)?;
        Command::new("git")
            .args(&["add", "src.rs"])
            .current_dir(&repo_path)
            .output()
            .await?;

        // Should reject due to high stub ratio
        let result = run_hybrid_review(
            &repo_path,
            "HEAD",
            None,
            "Implement feature",
            10.0,
            0.01,
            5,
        ).await?;

        match result.verdict {
            OverlordVerdict::Reject { reason } => {
                assert!(reason.contains("stub") || reason.contains("ambiguous"));
            }
            _ => panic!("Expected Reject for stubby code"),
        }

        Ok(())
    }
}
