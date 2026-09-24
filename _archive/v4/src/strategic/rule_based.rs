//! Rule-based strategic advisor — deterministic decisions without LLM.

use super::{StrategicAdvisor, StrategicCommand, StrategicEvent};
use anyhow::Result;
use async_trait::async_trait;

/// Configuration for rule-based strategic decisions.
#[derive(Debug, Clone)]
pub struct RuleBasedConfig {
    /// Max retries before failing a task.
    pub max_retries: usize,
    /// Bottleneck threshold for triggering zerg rush.
    pub zerg_rush_threshold: usize,
    /// Number of agents to spawn in zerg rush.
    pub zerg_rush_agents: usize,
    /// Max recovery attempts before failing.
    pub max_recovery_attempts: usize,
}

impl Default for RuleBasedConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            zerg_rush_threshold: 3,
            zerg_rush_agents: 3,
            max_recovery_attempts: 2,
        }
    }
}

/// Rule-based strategic advisor using deterministic heuristics.
pub struct RuleBasedAdvisor {
    config: RuleBasedConfig,
}

impl RuleBasedAdvisor {
    pub fn new(config: RuleBasedConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl StrategicAdvisor for RuleBasedAdvisor {
    async fn advise(&mut self, event: StrategicEvent) -> Result<StrategicCommand> {
        let command = match event {
            StrategicEvent::TaskEscalation {
                task_id,
                retry_count,
                reasons,
            } => {
                if retry_count >= self.config.max_retries {
                    StrategicCommand::FailTask {
                        task_id,
                        reason: format!(
                            "Failed after {} retries. Reasons: {}",
                            retry_count,
                            reasons.join("; ")
                        ),
                    }
                } else {
                    StrategicCommand::RetryTask {
                        task_id,
                        modified_description: None,
                    }
                }
            }
            StrategicEvent::DeadlockDetected {
                blocked_tasks,
                ready_tasks,
                ..
            } => {
                if ready_tasks.is_empty() && !blocked_tasks.is_empty() {
                    // True deadlock — fail the first blocked task to break the cycle
                    StrategicCommand::FailTask {
                        task_id: blocked_tasks[0].clone(),
                        reason: "Deadlock detected, failing to break cycle".to_string(),
                    }
                } else {
                    StrategicCommand::SpawnAgents {
                        count: ready_tasks.len().min(3),
                    }
                }
            }
            StrategicEvent::MergeConflict { task_id, .. } => StrategicCommand::RetryTask {
                task_id,
                modified_description: Some("Resolve merge conflicts before proceeding".to_string()),
            },
            StrategicEvent::BottleneckDetected {
                task_id,
                blocked_count,
            } => {
                if blocked_count >= self.config.zerg_rush_threshold {
                    StrategicCommand::ZergRush {
                        task_id,
                        num_agents: self.config.zerg_rush_agents,
                    }
                } else {
                    StrategicCommand::Noop
                }
            }
            StrategicEvent::AgentRecoveryFailed {
                task_id, attempts, ..
            } => {
                if attempts >= self.config.max_recovery_attempts {
                    StrategicCommand::FailTask {
                        task_id,
                        reason: format!("Agent recovery failed after {} attempts", attempts),
                    }
                } else {
                    StrategicCommand::RetryTask {
                        task_id,
                        modified_description: None,
                    }
                }
            }
        };

        Ok(command)
    }

    fn name(&self) -> &str {
        "rule-based"
    }

    fn uses_llm(&self) -> bool {
        false
    }
}
