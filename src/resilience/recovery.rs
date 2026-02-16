//! Session-based recovery with health monitoring.

use super::{Resilience, ResilienceAction, HealthStatus, RecoveryPlan, RecoveryAction, agent_key, task_key};
use crate::core::types::{TaskId, AgentId};
use anyhow::Result;
use std::collections::HashMap;
use std::time::{Duration, Instant};

// ============================================================================
// Configuration
// ============================================================================

/// Configuration for recovery behavior.
#[derive(Debug, Clone)]
pub struct RecoveryConfig {
    /// Maximum number of recovery attempts per agent
    pub max_recoveries: u32,
    /// Time without output before considering an agent stalled
    pub stall_timeout: Duration,
    /// Minimum time between recovery attempts (cooldown)
    pub recovery_cooldown: Duration,
}

impl Default for RecoveryConfig {
    fn default() -> Self {
        RecoveryConfig {
            max_recoveries: 3,
            stall_timeout: Duration::from_secs(300), // 5 minutes
            recovery_cooldown: Duration::from_secs(60), // 1 minute
        }
    }
}

// ============================================================================
// State Tracking
// ============================================================================

/// Health record for an agent.
#[derive(Debug, Clone)]
struct AgentHealthRecord {
    agent_key: String,
    status: HealthStatus,
    last_heartbeat: Instant,
    recovery_count: u32,
    last_recovery: Option<Instant>,
    consecutive_failures: u32,
    session_id: Option<String>,
}

impl AgentHealthRecord {
    fn new(agent_key: String) -> Self {
        AgentHealthRecord {
            agent_key,
            status: HealthStatus::Healthy,
            last_heartbeat: Instant::now(),
            recovery_count: 0,
            last_recovery: None,
            consecutive_failures: 0,
            session_id: None,
        }
    }
}

/// Record of a recovery event.
#[derive(Debug, Clone)]
pub struct RecoveryEvent {
    pub agent_key: String,
    pub timestamp: Instant,
    pub action: RecoveryAction,
    pub reason: String,
}

// ============================================================================
// Implementation
// ============================================================================

/// Recovery resilience handler with session management and health monitoring.
pub struct RecoveryResilience {
    config: RecoveryConfig,
    agent_health: HashMap<String, AgentHealthRecord>,
    recovery_history: Vec<RecoveryEvent>,
    task_assignments: HashMap<String, String>, // task_id -> agent_key
}

impl RecoveryResilience {
    /// Create a new recovery resilience handler with the given configuration.
    pub fn new(config: RecoveryConfig) -> Self {
        RecoveryResilience {
            config,
            agent_health: HashMap::new(),
            recovery_history: Vec::new(),
            task_assignments: HashMap::new(),
        }
    }

    /// Create with default configuration.
    pub fn default() -> Self {
        Self::new(RecoveryConfig::default())
    }

    /// Get or create health record for an agent.
    fn get_or_create_health_record(&mut self, agent_key: String) -> &mut AgentHealthRecord {
        self.agent_health
            .entry(agent_key.clone())
            .or_insert_with(|| AgentHealthRecord::new(agent_key))
    }

    /// Check if an agent can be recovered based on recovery limits and cooldown.
    fn can_recover(&self, record: &AgentHealthRecord) -> bool {
        if record.recovery_count >= self.config.max_recoveries {
            return false;
        }

        if let Some(last_recovery) = record.last_recovery {
            let elapsed = Instant::now().duration_since(last_recovery);
            if elapsed < self.config.recovery_cooldown {
                return false;
            }
        }

        true
    }

    /// Record a heartbeat from an agent.
    pub fn record_heartbeat(&mut self, agent_id: &AgentId) {
        let key = agent_key(agent_id);
        let record = self.get_or_create_health_record(key);
        record.last_heartbeat = Instant::now();
        record.consecutive_failures = 0;

        // Update status based on heartbeat
        if record.status == HealthStatus::Dead || record.status == HealthStatus::Critical {
            record.status = HealthStatus::Healthy;
        }
    }

    /// Detect stalled agents (no output in stall_timeout).
    pub fn detect_stalled_agents(&mut self) -> Vec<AgentId> {
        let now = Instant::now();
        let mut stalled = Vec::new();

        for (key, record) in self.agent_health.iter_mut() {
            let elapsed = now.duration_since(record.last_heartbeat);
            if elapsed > self.config.stall_timeout {
                record.status = HealthStatus::Degraded;
                // Parse agent key back to AgentId (simplified)
                if key.starts_with("queen:") {
                    stalled.push(AgentId::Queen(crate::core::types::QueenId(
                        key.strip_prefix("queen:").unwrap().to_string(),
                    )));
                } else if key == "validator" {
                    stalled.push(AgentId::Validator);
                }
            }
        }

        stalled
    }

    /// Get the complete recovery history.
    pub fn get_recovery_history(&self) -> &[RecoveryEvent] {
        &self.recovery_history
    }

    /// Set session ID for an agent.
    pub fn set_session_id(&mut self, agent_id: &AgentId, session_id: String) {
        let key = agent_key(agent_id);
        let record = self.get_or_create_health_record(key);
        record.session_id = Some(session_id);
    }

    /// Get session ID for an agent.
    pub fn get_session_id(&self, agent_id: &AgentId) -> Option<String> {
        let key = agent_key(agent_id);
        self.agent_health.get(&key)?.session_id.clone()
    }

    /// Record task assignment to an agent.
    pub fn assign_task(&mut self, task_id: &TaskId, agent_id: &AgentId) {
        let task_key_str = task_key(task_id);
        let agent_key_str = agent_key(agent_id);
        self.task_assignments.insert(task_key_str, agent_key_str);
    }

    /// Get the number of recoveries for an agent.
    pub fn get_recovery_count(&self, agent_id: &AgentId) -> u32 {
        let key = agent_key(agent_id);
        self.agent_health
            .get(&key)
            .map(|r| r.recovery_count)
            .unwrap_or(0)
    }
}

impl Resilience for RecoveryResilience {
    fn handle_failure(&mut self, _task_id: TaskId, agent_id: AgentId, _error: String) -> Result<ResilienceAction> {
        let agent_key_str = agent_key(&agent_id);

        // Extract config values we need
        let max_recoveries = self.config.max_recoveries;
        let recovery_cooldown = self.config.recovery_cooldown;

        // Get or create the record and check if we can recover
        let record = self.get_or_create_health_record(agent_key_str.clone());

        // Inline can_recover logic to avoid double borrow
        let can_recover = if record.recovery_count >= max_recoveries {
            false
        } else if let Some(last_recovery) = record.last_recovery {
            let elapsed = Instant::now().duration_since(last_recovery);
            elapsed >= recovery_cooldown
        } else {
            true
        };

        if !can_recover {
            // Max recoveries exceeded, escalate to operator
            return Ok(ResilienceAction::Escalate {
                to: AgentId::Operator,
            });
        }

        // Update failure count
        record.consecutive_failures += 1;

        // Determine action based on failure count and agent state
        let action = if record.consecutive_failures == 1 {
            // First failure - simple retry
            ResilienceAction::Retry {
                delay: Duration::from_secs(5),
            }
        } else if record.consecutive_failures < 3 {
            // Multiple failures - consider recovery
            if record.session_id.is_some() {
                // Try to resume from session
                ResilienceAction::Retry {
                    delay: Duration::from_secs(10),
                }
            } else {
                ResilienceAction::Retry {
                    delay: Duration::from_secs(15),
                }
            }
        } else {
            // Too many failures - abandon and escalate
            record.status = HealthStatus::Critical;
            ResilienceAction::Abandon
        };

        Ok(action)
    }

    fn check_health(&self, agent_id: AgentId) -> Result<HealthStatus> {
        let key = agent_key(&agent_id);

        if let Some(record) = self.agent_health.get(&key) {
            let elapsed = Instant::now().duration_since(record.last_heartbeat);

            let status = if elapsed < Duration::from_secs(60) {
                HealthStatus::Healthy
            } else if elapsed < Duration::from_secs(180) {
                HealthStatus::Degraded
            } else if elapsed < self.config.stall_timeout {
                HealthStatus::Critical
            } else {
                HealthStatus::Dead
            };

            Ok(status)
        } else {
            // No health record - assume healthy (new agent)
            Ok(HealthStatus::Healthy)
        }
    }

    fn plan_recovery(&mut self, agent_id: AgentId) -> Result<RecoveryPlan> {
        let agent_key_str = agent_key(&agent_id);

        // Extract values we need before the mutable borrow
        let max_recoveries = self.config.max_recoveries;

        // Build the recovery event inside a scope to end the mutable borrow
        let (action, event) = {
            let record = self.get_or_create_health_record(agent_key_str.clone());

            let action = if record.recovery_count >= max_recoveries {
                // Max recoveries exceeded, spawn replacement
                let replacement_id = match &agent_id {
                    AgentId::Queen(id) => {
                        let new_id = format!("{}_replacement_{}", id.0, record.recovery_count);
                        AgentId::Queen(crate::core::types::QueenId(new_id))
                    }
                    AgentId::Nydus(id) => {
                        let new_id = format!("{}_replacement_{}", id.0, record.recovery_count);
                        AgentId::Nydus(crate::core::types::NydusId(new_id))
                    }
                    _ => agent_id.clone(),
                };

                RecoveryAction::Spawn {
                    replacement_id,
                }
            } else if let Some(session_id) = &record.session_id {
                // Resume from session
                RecoveryAction::Resume {
                    session_id: session_id.clone(),
                }
            } else {
                // Restart agent
                RecoveryAction::Restart
            };

            // Update recovery tracking
            let recovery_count = record.recovery_count + 1;
            record.recovery_count = recovery_count;
            record.last_recovery = Some(Instant::now());

            // Create the event
            let event = RecoveryEvent {
                agent_key: agent_key_str,
                timestamp: Instant::now(),
                action: action.clone(),
                reason: format!("Recovery attempt {}/{}", recovery_count, max_recoveries),
            };

            (action, event)
        }; // Mutable borrow ends here

        // Now push the event
        self.recovery_history.push(event);

        Ok(RecoveryPlan {
            agent_id: agent_id.clone(),
            action,
        })
    }

    fn record_failure(&mut self, task_id: TaskId) -> Result<()> {
        // Find which agent was assigned this task
        let task_key_str = task_key(&task_id);
        if let Some(agent_key_str) = self.task_assignments.get(&task_key_str) {
            if let Some(record) = self.agent_health.get_mut(agent_key_str) {
                record.consecutive_failures += 1;
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_recovery_cooldown() {
        let config = RecoveryConfig {
            max_recoveries: 3,
            stall_timeout: Duration::from_secs(300),
            recovery_cooldown: Duration::from_secs(60),
        };

        let mut recovery = RecoveryResilience::new(config);
        let agent_id = AgentId::Validator;

        // First recovery should succeed
        let plan = recovery.plan_recovery(agent_id.clone()).unwrap();
        assert!(matches!(plan.action, RecoveryAction::Restart));

        // Second recovery immediately after should still work (cooldown checked in can_recover)
        let plan = recovery.plan_recovery(agent_id.clone()).unwrap();
        assert!(matches!(plan.action, RecoveryAction::Restart));
    }

    #[test]
    fn test_health_status_progression() {
        let mut recovery = RecoveryResilience::default();
        let agent_id = AgentId::Validator;

        // Record heartbeat
        recovery.record_heartbeat(&agent_id);

        // Check health immediately - should be healthy
        let status = recovery.check_health(agent_id.clone()).unwrap();
        assert_eq!(status, HealthStatus::Healthy);
    }
}
