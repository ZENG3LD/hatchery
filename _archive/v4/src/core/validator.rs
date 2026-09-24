use std::path::PathBuf;
use std::time::{Duration, Instant};
use anyhow::Result;

/// Rules for git diff validation.
#[derive(Debug, Clone)]
pub enum DiffRule {
    /// Forbid files matching a pattern (e.g., ".env")
    ForbiddenPattern { pattern: String, message: String },
    /// Max file size in bytes
    MaxFileSize { bytes: u64 },
    /// Require test files matching a glob
    RequireTests { test_glob: String },
    /// No binary files
    NoBinaryFiles,
}

impl DiffRule {
    /// Default rules: no .env files, no binary files
    pub fn defaults() -> Vec<DiffRule> {
        vec![
            DiffRule::ForbiddenPattern {
                pattern: ".env".to_string(),
                message: "Environment files must not be committed".to_string(),
            },
            DiffRule::NoBinaryFiles,
        ]
    }
}

/// Validator — checks the quality of completed work.
///
/// Four modes:
/// - Command: run a shell command (cargo check, cargo test, etc.)
/// - AiReview: use an AI agent to review code (Phase 5 placeholder)
/// - Pipeline: command first, then optional AI review
/// - GitDiff: analyze staged changes against rules
pub enum Validator {
    /// Simple command validator
    Command {
        cmd: String,
        working_dir: PathBuf,
    },
    /// AI-powered validator (placeholder for Phase 5)
    AiReview {
        model: String,
        prompt_template: String,
    },
    /// Multi-stage pipeline
    Pipeline(Vec<ValidationStage>),
    /// Git diff analyzer — checks staged changes against rules
    GitDiff {
        working_dir: PathBuf,
        rules: Vec<DiffRule>,
    },
}

pub struct ValidationStage {
    pub name: String,
    pub validator: Validator,
    pub required: bool,
}

#[derive(Debug, Clone)]
pub struct ValidationResult {
    pub passed: bool,
    pub stage_results: Vec<StageResult>,
    pub feedback: Option<String>,
    pub failed_rules: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct StageResult {
    pub stage_name: String,
    pub passed: bool,
    pub output: String,
    pub duration: Duration,
}

impl Validator {
    /// Create a command validator.
    pub fn command(cmd: &str, working_dir: PathBuf) -> Self {
        Validator::Command { cmd: cmd.to_string(), working_dir }
    }

    /// Create a pipeline of validators.
    pub fn pipeline(stages: Vec<ValidationStage>) -> Self {
        Validator::Pipeline(stages)
    }

    /// Run validation.
    pub async fn validate(&self, _files: &[PathBuf]) -> Result<ValidationResult> {
        match self {
            Validator::Command { cmd, working_dir } => {
                let start = Instant::now();

                // Use platform-appropriate shell
                let output = if cfg!(windows) {
                    tokio::process::Command::new("cmd")
                        .args(["/C", cmd])
                        .current_dir(working_dir)
                        .output()
                        .await?
                } else {
                    tokio::process::Command::new("sh")
                        .arg("-c")
                        .arg(cmd)
                        .current_dir(working_dir)
                        .output()
                        .await?
                };

                let passed = output.status.success();
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                let output_str = if stderr.is_empty() { stdout.clone() } else { format!("{}\n{}", stdout, stderr) };

                Ok(ValidationResult {
                    passed,
                    stage_results: vec![StageResult {
                        stage_name: "command".to_string(),
                        passed,
                        output: output_str.clone(),
                        duration: start.elapsed(),
                    }],
                    feedback: if !passed { Some(output_str) } else { None },
                    failed_rules: vec![],
                })
            }
            Validator::AiReview { .. } => {
                // Phase 5: AI-powered validation
                Ok(ValidationResult {
                    passed: true,
                    stage_results: vec![StageResult {
                        stage_name: "ai_review".to_string(),
                        passed: true,
                        output: "AI review not yet implemented".to_string(),
                        duration: Duration::from_secs(0),
                    }],
                    feedback: None,
                    failed_rules: vec![],
                })
            }
            Validator::Pipeline(stages) => {
                let mut stage_results = Vec::new();
                let mut all_passed = true;

                for stage in stages {
                    let result = Box::pin(stage.validator.validate(_files)).await?;
                    let stage_passed = result.passed;
                    stage_results.extend(result.stage_results);

                    if !stage_passed && stage.required {
                        all_passed = false;
                        break;
                    }
                }

                Ok(ValidationResult {
                    passed: all_passed,
                    stage_results,
                    feedback: None,
                    failed_rules: vec![],
                })
            }
            Validator::GitDiff { working_dir, rules } => {
                let start = Instant::now();
                let mut failed_rules = Vec::new();

                // Get git diff stat
                let diff_stat = if cfg!(windows) {
                    tokio::process::Command::new("git")
                        .args(["diff", "--cached", "--stat"])
                        .current_dir(working_dir)
                        .output()
                        .await?
                } else {
                    tokio::process::Command::new("git")
                        .args(["diff", "--cached", "--stat"])
                        .current_dir(working_dir)
                        .output()
                        .await?
                };

                let diff_stat_str = String::from_utf8_lossy(&diff_stat.stdout).to_string();

                // Get full diff for pattern matching
                let diff_full = tokio::process::Command::new("git")
                    .args(["diff", "--cached", "--name-only"])
                    .current_dir(working_dir)
                    .output()
                    .await?;

                let changed_files: Vec<String> = String::from_utf8_lossy(&diff_full.stdout)
                    .lines()
                    .map(|l| l.to_string())
                    .filter(|l| !l.is_empty())
                    .collect();

                // Apply rules
                for rule in rules {
                    match rule {
                        DiffRule::ForbiddenPattern { pattern, message } => {
                            for file in &changed_files {
                                if file.contains(pattern) {
                                    failed_rules.push(format!("ForbiddenPattern: {} — {}", file, message));
                                }
                            }
                        }
                        DiffRule::MaxFileSize { bytes } => {
                            for file in &changed_files {
                                let file_path = working_dir.join(file);
                                if let Ok(metadata) = std::fs::metadata(&file_path) {
                                    if metadata.len() > *bytes {
                                        failed_rules.push(format!(
                                            "MaxFileSize: {} is {} bytes (limit: {})",
                                            file, metadata.len(), bytes
                                        ));
                                    }
                                }
                            }
                        }
                        DiffRule::RequireTests { test_glob } => {
                            let has_tests = changed_files.iter().any(|f| f.contains(test_glob));
                            if !has_tests && !changed_files.is_empty() {
                                failed_rules.push(format!(
                                    "RequireTests: no files matching '{}' in changed files",
                                    test_glob
                                ));
                            }
                        }
                        DiffRule::NoBinaryFiles => {
                            // Check for binary files using git diff --numstat
                            let numstat = tokio::process::Command::new("git")
                                .args(["diff", "--cached", "--numstat"])
                                .current_dir(working_dir)
                                .output()
                                .await?;
                            let numstat_str = String::from_utf8_lossy(&numstat.stdout);
                            for line in numstat_str.lines() {
                                if line.starts_with('-') {
                                    // Binary files show as "- - filename"
                                    let parts: Vec<&str> = line.split('\t').collect();
                                    if parts.len() >= 3 {
                                        failed_rules.push(format!("NoBinaryFiles: {} is binary", parts[2]));
                                    }
                                }
                            }
                        }
                    }
                }

                let passed = failed_rules.is_empty();
                let output = if passed {
                    format!("Git diff clean: {} files changed\n{}", changed_files.len(), diff_stat_str)
                } else {
                    format!("Git diff violations:\n{}", failed_rules.join("\n"))
                };

                Ok(ValidationResult {
                    passed,
                    stage_results: vec![StageResult {
                        stage_name: "git_diff".to_string(),
                        passed,
                        output: output.clone(),
                        duration: start.elapsed(),
                    }],
                    feedback: if !passed { Some(output) } else { None },
                    failed_rules,
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_command_validator_success() {
        let v = Validator::command(
            if cfg!(windows) { "echo ok" } else { "echo ok" },
            std::env::current_dir().unwrap(),
        );
        let result = v.validate(&[]).await.unwrap();
        assert!(result.passed);
        assert_eq!(result.stage_results.len(), 1);
        assert!(result.feedback.is_none());
    }

    #[tokio::test]
    async fn test_command_validator_failure() {
        let v = Validator::command(
            if cfg!(windows) { "cmd /c exit 1" } else { "false" },
            std::env::current_dir().unwrap(),
        );
        let result = v.validate(&[]).await.unwrap();
        assert!(!result.passed);
        assert!(result.feedback.is_some());
    }

    #[tokio::test]
    async fn test_ai_review_placeholder() {
        let v = Validator::AiReview {
            model: "sonnet".to_string(),
            prompt_template: "Review this code".to_string(),
        };
        let result = v.validate(&[]).await.unwrap();
        assert!(result.passed); // placeholder always passes
    }

    #[tokio::test]
    async fn test_pipeline_all_pass() {
        let cwd = std::env::current_dir().unwrap();
        let v = Validator::pipeline(vec![
            ValidationStage {
                name: "check1".to_string(),
                validator: Validator::command(
                    if cfg!(windows) { "echo step1" } else { "echo step1" },
                    cwd.clone(),
                ),
                required: true,
            },
            ValidationStage {
                name: "check2".to_string(),
                validator: Validator::command(
                    if cfg!(windows) { "echo step2" } else { "echo step2" },
                    cwd,
                ),
                required: true,
            },
        ]);
        let result = v.validate(&[]).await.unwrap();
        assert!(result.passed);
        assert_eq!(result.stage_results.len(), 2);
    }

    #[tokio::test]
    async fn test_pipeline_stops_on_required_failure() {
        let cwd = std::env::current_dir().unwrap();
        let v = Validator::pipeline(vec![
            ValidationStage {
                name: "fail".to_string(),
                validator: Validator::command(
                    if cfg!(windows) { "cmd /c exit 1" } else { "false" },
                    cwd.clone(),
                ),
                required: true,
            },
            ValidationStage {
                name: "never_reached".to_string(),
                validator: Validator::command("echo ok", cwd),
                required: true,
            },
        ]);
        let result = v.validate(&[]).await.unwrap();
        assert!(!result.passed);
        assert_eq!(result.stage_results.len(), 1); // stopped at first failure
    }

    #[tokio::test]
    async fn test_git_diff_validator_clean() {
        use std::fs;
        use std::process::Command;
        use tempfile::tempdir;

        let temp_dir = tempdir().unwrap();
        let repo_path = temp_dir.path();

        // Initialize git repo
        Command::new("git")
            .args(["init"])
            .current_dir(repo_path)
            .output()
            .expect("Failed to init git repo");

        // Configure git for test
        Command::new("git")
            .args(["config", "user.email", "test@example.com"])
            .current_dir(repo_path)
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.name", "Test User"])
            .current_dir(repo_path)
            .output()
            .unwrap();

        // Create and stage a clean file
        fs::write(repo_path.join("test.rs"), "fn main() {}").unwrap();
        Command::new("git")
            .args(["add", "test.rs"])
            .current_dir(repo_path)
            .output()
            .unwrap();

        // Run validator
        let v = Validator::GitDiff {
            working_dir: repo_path.to_path_buf(),
            rules: DiffRule::defaults(),
        };
        let result = v.validate(&[]).await.unwrap();

        assert!(result.passed);
        assert!(result.failed_rules.is_empty());
        assert_eq!(result.stage_results.len(), 1);
        assert_eq!(result.stage_results[0].stage_name, "git_diff");
    }

    #[tokio::test]
    async fn test_git_diff_validator_forbidden_pattern() {
        use std::fs;
        use std::process::Command;
        use tempfile::tempdir;

        let temp_dir = tempdir().unwrap();
        let repo_path = temp_dir.path();

        // Initialize git repo
        Command::new("git")
            .args(["init"])
            .current_dir(repo_path)
            .output()
            .expect("Failed to init git repo");

        // Configure git for test
        Command::new("git")
            .args(["config", "user.email", "test@example.com"])
            .current_dir(repo_path)
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.name", "Test User"])
            .current_dir(repo_path)
            .output()
            .unwrap();

        // Create and stage .env file (forbidden)
        fs::write(repo_path.join(".env"), "SECRET=123").unwrap();
        Command::new("git")
            .args(["add", ".env"])
            .current_dir(repo_path)
            .output()
            .unwrap();

        // Run validator with forbidden pattern rule
        let v = Validator::GitDiff {
            working_dir: repo_path.to_path_buf(),
            rules: vec![DiffRule::ForbiddenPattern {
                pattern: ".env".to_string(),
                message: "Environment files must not be committed".to_string(),
            }],
        };
        let result = v.validate(&[]).await.unwrap();

        assert!(!result.passed);
        assert!(!result.failed_rules.is_empty());
        assert!(result.failed_rules[0].contains("ForbiddenPattern"));
        assert!(result.failed_rules[0].contains(".env"));
        assert!(result.feedback.is_some());
    }
}
