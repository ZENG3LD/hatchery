//! BroodLord — Level 3 strategic orchestrator for Hatchery V2.
//!
//! BroodLord manages multiple SwarmHosts, each handling a sub-project.
//! It provides:
//! - Sub-project decomposition and SwarmHost management
//! - Cross-SwarmHost dependency monitoring
//! - Intelligent escalation handling
//! - Operator communication via OperatorChannel
//! - Dynamic control (reprioritize, cancel, scale)

use std::collections::HashMap;
use anyhow::{Result, anyhow};
use chrono::{DateTime, Utc};
use serde::{Serialize, Deserialize};

use crate::core::types::*;
use crate::swarm_host::{SwarmHost, SwarmHostConfig, SwarmProgress, TickResult};
use crate::core::operator::{OperatorChannel, OperatorEvent, OperatorCommand, LogLevel};
use crate::core::shared_memory::SharedMemory;

// ============================================================================
// Types
// ============================================================================

/// Status of a managed SwarmHost.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SwarmHostStatus {
    /// SwarmHost is initializing
    Initializing,
    /// SwarmHost is running with progress
    Running { tasks_done: usize, tasks_total: usize },
    /// SwarmHost is blocked
    Blocked { reason: String },
    /// SwarmHost completed successfully
    Completed { tasks_done: usize, tasks_total: usize, duration_secs: u64 },
    /// SwarmHost failed
    Failed { error: String },
}

/// A sub-project managed by the BroodLord.
pub struct ManagedSwarmHost {
    /// The SwarmHost instance
    pub swarm: SwarmHost,
    /// Current status
    pub status: SwarmHostStatus,
    /// When this swarm was started
    pub started_at: DateTime<Utc>,
    /// Priority (higher = more important)
    pub priority: u8,
}

/// Configuration for BroodLord.
#[derive(Debug, Clone)]
pub struct BroodLordConfig {
    /// Maximum number of SwarmHosts
    pub max_swarm_hosts: usize,
    /// Default config for spawned SwarmHosts
    pub default_swarm_config: SwarmHostConfig,
    /// Maximum total iterations across all swarms
    pub max_total_iterations: usize,
}

impl Default for BroodLordConfig {
    fn default() -> Self {
        Self {
            max_swarm_hosts: 4,
            default_swarm_config: SwarmHostConfig::default(),
            max_total_iterations: 1000,
        }
    }
}

/// Result of a BroodLord tick.
#[derive(Debug, Clone)]
pub struct BroodLordTickResult {
    pub swarm_ticks: HashMap<String, TickResult>,
    pub commands_processed: usize,
    pub escalations_forwarded: usize,
    pub swarms_completed: usize,
    pub iteration: usize,
}

/// Global progress summary across all SwarmHosts.
#[derive(Debug, Clone)]
pub struct GlobalProgress {
    pub total_swarms: usize,
    pub active_swarms: usize,
    pub completed_swarms: usize,
    pub total_tasks: usize,
    pub completed_tasks: usize,
    pub failed_tasks: usize,
    pub total_iterations: usize,
}

// ============================================================================
// BroodLord
// ============================================================================

/// BroodLord — Level 3 strategic orchestrator.
///
/// Manages multiple SwarmHosts, each handling a sub-project.
/// Reports to the Operator via OperatorChannel and handles
/// operator commands for dynamic control.
pub struct BroodLord {
    /// Managed SwarmHosts
    swarms: HashMap<String, ManagedSwarmHost>,
    /// Global knowledge store (cross-SwarmHost)
    global_memory: SharedMemory,
    /// Communication with operator
    operator: Box<dyn OperatorChannel>,
    /// Configuration
    config: BroodLordConfig,
    /// Total iteration count
    iteration: usize,
    /// When BroodLord was created
    created_at: DateTime<Utc>,
}

impl BroodLord {
    /// Create a new BroodLord.
    pub fn new(config: BroodLordConfig, operator: Box<dyn OperatorChannel>) -> Self {
        Self {
            swarms: HashMap::new(),
            global_memory: SharedMemory::new(SwarmHostId("brood-lord".to_string())),
            operator,
            config,
            iteration: 0,
            created_at: Utc::now(),
        }
    }

    /// Add a SwarmHost to manage.
    /// Returns the swarm ID.
    pub fn add_swarm(&mut self, id: &str, swarm: SwarmHost, priority: u8) -> Result<()> {
        if self.swarms.len() >= self.config.max_swarm_hosts {
            return Err(anyhow!("Max SwarmHosts ({}) reached", self.config.max_swarm_hosts));
        }
        self.swarms.insert(id.to_string(), ManagedSwarmHost {
            swarm,
            status: SwarmHostStatus::Running { tasks_done: 0, tasks_total: 0 },
            started_at: Utc::now(),
            priority,
        });
        Ok(())
    }

    /// Run one tick: process operator commands, tick all active swarms, collect results.
    pub async fn tick(&mut self) -> Result<BroodLordTickResult> {
        self.iteration += 1;
        let mut result = BroodLordTickResult {
            swarm_ticks: HashMap::new(),
            commands_processed: 0,
            escalations_forwarded: 0,
            swarms_completed: 0,
            iteration: self.iteration,
        };

        // 1. Process operator commands
        result.commands_processed = self.process_commands().await?;

        // 2. Tick all active swarms
        // Collect swarm IDs first to avoid borrow issues
        let swarm_ids: Vec<String> = self.swarms.keys().cloned().collect();

        for swarm_id in &swarm_ids {
            let managed = self.swarms.get_mut(swarm_id).unwrap();

            // Skip completed/failed swarms
            if matches!(managed.status, SwarmHostStatus::Completed { .. } | SwarmHostStatus::Failed { .. }) {
                continue;
            }

            // Tick the swarm
            match managed.swarm.tick().await {
                Ok(tick_result) => {
                    result.swarm_ticks.insert(swarm_id.clone(), tick_result);
                }
                Err(e) => {
                    managed.status = SwarmHostStatus::Failed { error: e.to_string() };
                    self.operator.emit(OperatorEvent::Error {
                        source: AgentId::SwarmHost(SwarmHostId(swarm_id.clone())),
                        error: e.to_string(),
                    }).await?;
                }
            }

            // Update status
            let progress = managed.swarm.progress();
            if managed.swarm.is_complete() {
                let elapsed = (Utc::now() - managed.started_at).num_seconds().max(0) as u64;
                managed.status = SwarmHostStatus::Completed {
                    tasks_done: progress.completed,
                    tasks_total: progress.total_tasks,
                    duration_secs: elapsed,
                };
                result.swarms_completed += 1;

                // Emit completion event
                self.operator.emit(OperatorEvent::SwarmCompleted {
                    swarm_id: SwarmHostId(swarm_id.clone()),
                    tasks_done: progress.completed,
                    tasks_total: progress.total_tasks,
                    duration_secs: elapsed,
                }).await?;
            } else {
                managed.status = SwarmHostStatus::Running {
                    tasks_done: progress.completed,
                    tasks_total: progress.total_tasks,
                };
            }

            // 3. Process outbox messages (escalations → operator)
            let outbox = managed.swarm.drain_outbox();
            for msg in outbox {
                if matches!(msg.msg_type, MessageType::Escalation) {
                    result.escalations_forwarded += 1;
                    // Forward escalation to operator
                    if let Ok(issue) = serde_json::from_value::<String>(
                        msg.payload.get("issue").cloned().unwrap_or_default()
                    ) {
                        self.operator.emit(OperatorEvent::Escalation {
                            source: msg.from.clone(),
                            issue,
                            severity: Severity::Medium,
                        }).await?;
                    }
                }
                // Store knowledge in global memory
                if matches!(msg.msg_type, MessageType::Knowledge) {
                    if let (Some(key), Some(value)) = (
                        msg.payload.get("key").and_then(|v| v.as_str()),
                        msg.payload.get("value"),
                    ) {
                        self.global_memory.insert(
                            format!("{}.{}", swarm_id, key),
                            value.clone(),
                            msg.from.clone(),
                        );
                    }
                }
            }
        }

        // 4. Emit global progress
        let global = self.global_progress();
        self.operator.emit(OperatorEvent::GlobalProgress {
            done: global.completed_tasks,
            total: global.total_tasks,
            elapsed_secs: (Utc::now() - self.created_at).num_seconds().max(0) as u64,
        }).await?;

        // 5. Check if all complete
        if self.is_complete() {
            let elapsed = (Utc::now() - self.created_at).num_seconds().max(0) as u64;
            self.operator.emit(OperatorEvent::AllComplete {
                total_tasks: global.total_tasks,
                total_duration_secs: elapsed,
            }).await?;
        }

        Ok(result)
    }

    /// Process pending operator commands.
    async fn process_commands(&mut self) -> Result<usize> {
        let mut count = 0;
        while let Some(cmd) = self.operator.try_recv().await {
            count += 1;
            match cmd {
                OperatorCommand::Cancel { swarm_id } => {
                    if let Some(managed) = self.swarms.get_mut(&swarm_id.0) {
                        managed.swarm.shutdown().await?;
                        managed.status = SwarmHostStatus::Failed {
                            error: "Cancelled by operator".to_string(),
                        };
                    }
                }
                OperatorCommand::ShutdownAll => {
                    self.shutdown().await?;
                }
                OperatorCommand::Reprioritize { swarm_id, priority } => {
                    if let Some(managed) = self.swarms.get_mut(&swarm_id.0) {
                        managed.priority = priority;
                    }
                }
                // Scale, Answer, Message — log for now
                _ => {
                    self.operator.emit(OperatorEvent::Log {
                        level: LogLevel::Info,
                        message: format!("Command received but not yet implemented: {:?}", cmd),
                    }).await?;
                }
            }
        }
        Ok(count)
    }

    /// Check if all SwarmHosts are complete.
    pub fn is_complete(&self) -> bool {
        !self.swarms.is_empty() && self.swarms.values().all(|s| {
            matches!(s.status, SwarmHostStatus::Completed { .. } | SwarmHostStatus::Failed { .. })
        })
    }

    /// Get global progress.
    pub fn global_progress(&self) -> GlobalProgress {
        let mut gp = GlobalProgress {
            total_swarms: self.swarms.len(),
            active_swarms: 0,
            completed_swarms: 0,
            total_tasks: 0,
            completed_tasks: 0,
            failed_tasks: 0,
            total_iterations: self.iteration,
        };
        for managed in self.swarms.values() {
            let progress = managed.swarm.progress();
            gp.total_tasks += progress.total_tasks;
            gp.completed_tasks += progress.completed;
            gp.failed_tasks += progress.failed;
            match &managed.status {
                SwarmHostStatus::Running { .. } => gp.active_swarms += 1,
                SwarmHostStatus::Completed { .. } => gp.completed_swarms += 1,
                _ => {}
            }
        }
        gp
    }

    /// Get a reference to global memory.
    pub fn global_memory(&self) -> &SharedMemory {
        &self.global_memory
    }

    /// Number of managed SwarmHosts.
    pub fn swarm_count(&self) -> usize {
        self.swarms.len()
    }

    /// Get SwarmHost status by ID.
    pub fn swarm_status(&self, id: &str) -> Option<&SwarmHostStatus> {
        self.swarms.get(id).map(|s| &s.status)
    }

    /// Get progress of a specific SwarmHost.
    pub fn swarm_progress(&self, id: &str) -> Option<SwarmProgress> {
        self.swarms.get(id).map(|s| s.swarm.progress())
    }

    /// Shutdown all SwarmHosts gracefully.
    pub async fn shutdown(&mut self) -> Result<()> {
        for (_, managed) in self.swarms.iter_mut() {
            managed.swarm.shutdown().await?;
            if !matches!(managed.status, SwarmHostStatus::Completed { .. }) {
                managed.status = SwarmHostStatus::Failed {
                    error: "Shutdown requested".to_string(),
                };
            }
        }
        Ok(())
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::operator::NullChannel;
    use crate::core::task_dag::{Priority, Complexity};
    use crate::queen::Queen;
    use async_trait::async_trait;

    // MockQueen for testing
    struct MockQueen {
        id: QueenId,
        status: QueenStatus,
    }

    impl MockQueen {
        fn new(id: &str) -> Self {
            Self {
                id: QueenId(id.to_string()),
                status: QueenStatus::Idle,
            }
        }
    }

    #[async_trait]
    impl Queen for MockQueen {
        fn id(&self) -> QueenId {
            self.id.clone()
        }

        fn backend(&self) -> QueenBackend {
            QueenBackend::ClaudeNative
        }

        async fn assign(&mut self, _task: Task, _ctx: TaskContext) -> Result<()> {
            self.status = QueenStatus::Working {
                task_id: TaskId::default(),
                progress: 0.0,
                sub_tasks: vec![],
            };
            Ok(())
        }

        async fn status(&self) -> QueenStatus {
            self.status.clone()
        }

        async fn result(&self) -> Option<TaskResult> {
            None
        }

        async fn send_message(&mut self, _msg: SwarmMessage) -> Result<()> {
            Ok(())
        }

        async fn drain_outbox(&mut self) -> Vec<SwarmMessage> {
            vec![]
        }

        async fn is_alive(&self) -> bool {
            true
        }

        async fn shutdown(&mut self) -> Result<()> {
            Ok(())
        }
    }

    #[test]
    fn test_create_brood_lord_with_default_config() {
        let config = BroodLordConfig::default();
        let operator = Box::new(NullChannel::new());
        let lord = BroodLord::new(config, operator);

        assert_eq!(lord.swarm_count(), 0);
        assert_eq!(lord.iteration, 0);
        assert!(!lord.is_complete());
    }

    #[test]
    fn test_add_swarm_hosts_up_to_max_limit() {
        let config = BroodLordConfig {
            max_swarm_hosts: 2,
            ..Default::default()
        };
        let operator = Box::new(NullChannel::new());
        let mut lord = BroodLord::new(config, operator);

        // Add first swarm
        let swarm1 = SwarmHost::new(
            SwarmHostId("swarm1".to_string()),
            SwarmHostConfig::default(),
        ).unwrap();
        assert!(lord.add_swarm("swarm1", swarm1, 100).is_ok());
        assert_eq!(lord.swarm_count(), 1);

        // Add second swarm
        let swarm2 = SwarmHost::new(
            SwarmHostId("swarm2".to_string()),
            SwarmHostConfig::default(),
        ).unwrap();
        assert!(lord.add_swarm("swarm2", swarm2, 100).is_ok());
        assert_eq!(lord.swarm_count(), 2);

        // Try to add third swarm (should fail)
        let swarm3 = SwarmHost::new(
            SwarmHostId("swarm3".to_string()),
            SwarmHostConfig::default(),
        ).unwrap();
        assert!(lord.add_swarm("swarm3", swarm3, 100).is_err());
        assert_eq!(lord.swarm_count(), 2);
    }

    #[test]
    fn test_is_complete_returns_false_when_swarms_running() {
        let config = BroodLordConfig::default();
        let operator = Box::new(NullChannel::new());
        let mut lord = BroodLord::new(config, operator);

        let swarm = SwarmHost::new(
            SwarmHostId("swarm1".to_string()),
            SwarmHostConfig::default(),
        ).unwrap();
        lord.add_swarm("swarm1", swarm, 100).unwrap();

        // Swarm is running (default status)
        assert!(!lord.is_complete());
    }

    #[test]
    fn test_is_complete_returns_true_when_all_swarms_done() {
        let config = BroodLordConfig::default();
        let operator = Box::new(NullChannel::new());
        let mut lord = BroodLord::new(config, operator);

        let swarm = SwarmHost::new(
            SwarmHostId("swarm1".to_string()),
            SwarmHostConfig::default(),
        ).unwrap();
        lord.add_swarm("swarm1", swarm, 100).unwrap();

        // Manually mark as completed
        let managed = lord.swarms.get_mut("swarm1").unwrap();
        managed.status = SwarmHostStatus::Completed {
            tasks_done: 5,
            tasks_total: 5,
            duration_secs: 120,
        };

        assert!(lord.is_complete());
    }

    #[test]
    fn test_global_progress_returns_correct_aggregated_counts() {
        let config = BroodLordConfig::default();
        let operator = Box::new(NullChannel::new());
        let mut lord = BroodLord::new(config, operator);

        // Add two swarms with tasks
        let mut swarm1 = SwarmHost::new(
            SwarmHostId("swarm1".to_string()),
            SwarmHostConfig::default(),
        ).unwrap();
        swarm1.add_task("t1", "Task 1", vec![], Priority::High, Complexity::Trivial);
        swarm1.add_task("t2", "Task 2", vec![], Priority::High, Complexity::Trivial);
        lord.add_swarm("swarm1", swarm1, 100).unwrap();

        let mut swarm2 = SwarmHost::new(
            SwarmHostId("swarm2".to_string()),
            SwarmHostConfig::default(),
        ).unwrap();
        swarm2.add_task("t3", "Task 3", vec![], Priority::Normal, Complexity::Medium);
        lord.add_swarm("swarm2", swarm2, 100).unwrap();

        let progress = lord.global_progress();
        assert_eq!(progress.total_swarms, 2);
        assert_eq!(progress.total_tasks, 3);
        assert_eq!(progress.completed_tasks, 0);
    }

    #[test]
    fn test_swarm_count_and_swarm_status() {
        let config = BroodLordConfig::default();
        let operator = Box::new(NullChannel::new());
        let mut lord = BroodLord::new(config, operator);

        let swarm = SwarmHost::new(
            SwarmHostId("swarm1".to_string()),
            SwarmHostConfig::default(),
        ).unwrap();
        lord.add_swarm("swarm1", swarm, 100).unwrap();

        assert_eq!(lord.swarm_count(), 1);

        let status = lord.swarm_status("swarm1");
        assert!(status.is_some());

        match status.unwrap() {
            SwarmHostStatus::Running { .. } => {},
            _ => panic!("Expected Running status"),
        }
    }

    #[tokio::test]
    async fn test_process_cancel_command() {
        let config = BroodLordConfig::default();
        let channel = NullChannel::new();

        // Pre-load cancel command
        channel.push_command(OperatorCommand::Cancel {
            swarm_id: SwarmHostId("swarm1".to_string()),
        });

        let mut lord = BroodLord::new(config, Box::new(channel));

        let swarm = SwarmHost::new(
            SwarmHostId("swarm1".to_string()),
            SwarmHostConfig::default(),
        ).unwrap();
        lord.add_swarm("swarm1", swarm, 100).unwrap();

        // Tick should process the cancel command
        let result = lord.tick().await.unwrap();
        assert_eq!(result.commands_processed, 1);

        // Swarm should be marked as failed
        let status = lord.swarm_status("swarm1").unwrap();
        match status {
            SwarmHostStatus::Failed { error } => {
                assert_eq!(error, "Cancelled by operator");
            }
            _ => panic!("Expected Failed status after cancel"),
        }
    }

    #[tokio::test]
    async fn test_process_shutdown_all_command() {
        let config = BroodLordConfig::default();
        let channel = NullChannel::new();

        // Pre-load shutdown command
        channel.push_command(OperatorCommand::ShutdownAll);

        let mut lord = BroodLord::new(config, Box::new(channel));

        let swarm = SwarmHost::new(
            SwarmHostId("swarm1".to_string()),
            SwarmHostConfig::default(),
        ).unwrap();
        lord.add_swarm("swarm1", swarm, 100).unwrap();

        // Tick should process shutdown command
        let result = lord.tick().await.unwrap();
        assert_eq!(result.commands_processed, 1);

        // Swarm should be marked as failed (shutdown requested)
        let status = lord.swarm_status("swarm1").unwrap();
        match status {
            SwarmHostStatus::Failed { error } => {
                assert_eq!(error, "Shutdown requested");
            }
            _ => panic!("Expected Failed status after shutdown"),
        }
    }

    #[tokio::test]
    async fn test_tick_with_empty_swarms() {
        let config = BroodLordConfig::default();
        let operator = Box::new(NullChannel::new());
        let mut lord = BroodLord::new(config, operator);

        // Tick with no swarms should work
        let result = lord.tick().await.unwrap();
        assert_eq!(result.iteration, 1);
        assert_eq!(result.commands_processed, 0);
        assert_eq!(result.escalations_forwarded, 0);
        assert_eq!(result.swarms_completed, 0);
    }

    #[test]
    fn test_swarm_progress_returns_none_for_nonexistent_swarm() {
        let config = BroodLordConfig::default();
        let operator = Box::new(NullChannel::new());
        let lord = BroodLord::new(config, operator);

        assert!(lord.swarm_progress("nonexistent").is_none());
    }

    #[test]
    fn test_global_memory_access() {
        let config = BroodLordConfig::default();
        let operator = Box::new(NullChannel::new());
        let lord = BroodLord::new(config, operator);

        let memory = lord.global_memory();
        assert!(memory.is_empty());
    }
}
