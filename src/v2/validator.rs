use std::path::PathBuf;
use std::time::{Duration, Instant};
use anyhow::Result;

/// Validator — checks the quality of completed work.
///
/// Three modes:
/// - Command: run a shell command (cargo check, cargo test, etc.)
/// - AiReview: use an AI agent to review code (Phase 5 placeholder)
/// - Pipeline: command first, then optional AI review
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
}
