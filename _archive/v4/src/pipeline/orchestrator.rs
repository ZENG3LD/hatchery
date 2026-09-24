//! Pipeline orchestrator — unified event-driven orchestration engine.
//!
//! This replaces the hardcoded Nydus event loop with a composable architecture
//! that integrates 7 pipeline traits + 5 domain traits into a single coordinated system.

use crate::agent_backend::{AgentBackend, AgentEvent, AgentSpawnConfig};
use crate::communication::Communication;
use crate::core::types::{AgentId, Task, TaskContext, TaskId, TaskStatus};
use crate::decomposition::Decomposition;
use crate::isolation::IsolationBackend;
use crate::memory::Memory;
use crate::resilience::Resilience;
use crate::scaling::{Scaling, ScaleDecision, ScaleMetrics};
use crate::scheduling::Scheduling;
use crate::session_parser::SessionParser;
use crate::strategic::{StrategicAdvisor, StrategicEvent};
use crate::topology::Topology;
use crate::validation::{Validation, ValidationRequest};
use anyhow::Result;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Orchestrator configuration.
#[derive(Debug, Clone)]
pub struct OrchestratorConfig {
    /// Working directory for the orchestrator
    pub working_dir: PathBuf,
    /// PRD file path (optional, used for PRD-based decomposition)
    pub prd_path: Option<PathBuf>,
    /// Maximum number of agents to spawn
    pub max_agents: usize,
    /// Minimum number of agents to maintain
    pub min_agents: usize,
    /// Git base branch for merging
    pub base_branch: String,
    /// Whether to use git isolation
    pub git_isolation: bool,
    /// Verification command
    pub verify_cmd: Option<String>,
}

impl Default for OrchestratorConfig {
    fn default() -> Self {
        Self {
            working_dir: std::env::current_dir().unwrap_or_default(),
            prd_path: None,
            max_agents: 8,
            min_agents: 1,
            base_branch: "main".to_string(),
            git_isolation: false,
            verify_cmd: None,
        }
    }
}

/// Statistics from an orchestrator run.
#[derive(Debug, Clone)]
pub struct OrchestratorStats {
    pub tasks_completed: usize,
    pub tasks_failed: usize,
    pub total_tasks: usize,
    pub agents_spawned: usize,
    pub total_cost_usd: f64,
    pub duration: Duration,
}

/// Tracks state for an assigned task.
struct TaskAssignment {
    task_id: TaskId,
    agent_id: AgentId,
    started_at: Instant,
}

/// PipelineOrchestrator composes all pipeline components + domain traits
/// into a unified event-driven orchestration engine.
///
/// This replaces the hardcoded Nydus event loop with a composable architecture.
pub struct PipelineOrchestrator {
    // Pipeline components (7 traits)
    topology: Box<dyn Topology>,
    decomposition: Box<dyn Decomposition>,
    communication: Box<dyn Communication>,
    memory: Box<dyn Memory>,
    scheduling: Box<dyn Scheduling>,
    resilience: Box<dyn Resilience>,
    scaling: Box<dyn Scaling>,

    // Domain components (5 traits, optional)
    agent_backend: Option<Box<dyn AgentBackend>>,
    validation: Option<Box<dyn Validation>>,
    isolation: Option<Box<dyn IsolationBackend>>,
    strategic: Option<Box<dyn StrategicAdvisor>>,
    session_parser: Option<Box<dyn SessionParser>>,

    // Config
    config: OrchestratorConfig,

    // Runtime state
    active_assignments: HashMap<String, TaskAssignment>,
    tasks_completed: usize,
    tasks_failed: usize,
    total_cost_usd: f64,
    agents_spawned: usize,
}

impl PipelineOrchestrator {
    /// Create from a Pipeline and config
    pub fn from_pipeline(pipeline: super::Pipeline, config: OrchestratorConfig) -> Self {
        Self {
            topology: pipeline.topology,
            decomposition: pipeline.decomposition,
            communication: pipeline.communication,
            memory: pipeline.memory,
            scheduling: pipeline.scheduling,
            resilience: pipeline.resilience,
            scaling: pipeline.scaling,
            agent_backend: None,
            validation: None,
            isolation: None,
            strategic: None,
            session_parser: None,
            config,
            active_assignments: HashMap::new(),
            tasks_completed: 0,
            tasks_failed: 0,
            total_cost_usd: 0.0,
            agents_spawned: 0,
        }
    }

    /// Set the agent backend.
    pub fn with_agent_backend(mut self, backend: impl AgentBackend + 'static) -> Self {
        self.agent_backend = Some(Box::new(backend));
        self
    }

    /// Set the validation strategy.
    pub fn with_validation(mut self, validation: impl Validation + 'static) -> Self {
        self.validation = Some(Box::new(validation));
        self
    }

    /// Set the isolation backend.
    pub fn with_isolation(mut self, isolation: impl IsolationBackend + 'static) -> Self {
        self.isolation = Some(Box::new(isolation));
        self
    }

    /// Set the strategic advisor.
    pub fn with_strategic(mut self, strategic: impl StrategicAdvisor + 'static) -> Self {
        self.strategic = Some(Box::new(strategic));
        self
    }

    /// Set the session parser.
    pub fn with_session_parser(mut self, parser: impl SessionParser + 'static) -> Self {
        self.session_parser = Some(Box::new(parser));
        self
    }

    /// Get orchestrator statistics.
    pub fn stats(&self) -> OrchestratorStats {
        OrchestratorStats {
            tasks_completed: self.tasks_completed,
            tasks_failed: self.tasks_failed,
            total_tasks: self.tasks_completed + self.tasks_failed + self.active_assignments.len(),
            agents_spawned: self.agents_spawned,
            total_cost_usd: self.total_cost_usd,
            duration: Duration::from_secs(0), // filled by run()
        }
    }

    /// Run the orchestration loop.
    ///
    /// This is the main event loop that:
    /// 1. Gets ready tasks from decomposition
    /// 2. Assigns tasks to agents via topology + agent_backend
    /// 3. Waits for events from agents
    /// 4. On completion: validate → merge → update decomposition
    /// 5. On failure: resilience → strategic advisor
    /// 6. Check scaling metrics periodically
    /// 7. Repeat until scheduling says terminate
    pub async fn run(&mut self) -> Result<OrchestratorStats> {
        let start = Instant::now();

        // Check backend exists before entering loop
        if self.agent_backend.is_none() {
            return Err(anyhow::anyhow!("AgentBackend is required to run the orchestrator"));
        }

        // Spawn initial agents
        let initial_count = self.config.min_agents;
        for _ in 0..initial_count {
            let spawn_config = AgentSpawnConfig {
                working_dir: self.config.working_dir.clone(),
                ..AgentSpawnConfig::default()
            };
            let agent_id = self.agent_backend.as_mut().unwrap().spawn(spawn_config).await?;
            self.topology.add_agent(agent_id)?;
            self.agents_spawned += 1;
        }

        eprintln!(
            "[ORCHESTRATOR] Spawned {} initial agents",
            initial_count
        );

        // Main event loop
        loop {
            // Check if we should terminate
            if self.scheduling.should_terminate() && self.active_assignments.is_empty() {
                eprintln!("[ORCHESTRATOR] All tasks complete, terminating");
                break;
            }

            // Get ready tasks and assign them
            let ready = self.decomposition.ready_tasks();
            for task_id in ready {
                // Skip if already assigned
                if self.active_assignments.contains_key(&task_id.0) {
                    continue;
                }

                // Ask topology for assignment
                match self.topology.assign_task(&task_id) {
                    Ok(agents) if !agents.is_empty() => {
                        let agent_id = agents[0].clone();

                        // Create workspace if isolation is configured
                        let _workspace_path = if let Some(ref mut isolation) = self.isolation {
                            match isolation.create_workspace(&agent_id) {
                                Ok(info) => Some(info.path),
                                Err(e) => {
                                    eprintln!(
                                        "[ORCHESTRATOR] Warning: workspace creation failed: {}",
                                        e
                                    );
                                    None
                                }
                            }
                        } else {
                            None
                        };

                        // Build task context
                        let context = TaskContext {
                            knowledge: Default::default(),
                            recent_messages: vec![],
                            shared_state: Default::default(),
                            skill_hint: None,
                            knowledge_entries: vec![],
                            other_tasks_summary: None,
                            rejection_feedback: None,
                        };

                        // Create the task object
                        let task = Task {
                            id: task_id.clone(),
                            description: format!("Task {}", task_id.0),
                            status: TaskStatus::Assigned,
                            assigned_to: None,
                            priority: 128,
                            blocked_by: vec![],
                            created_at: chrono::Utc::now(),
                        };

                        // Assign task via backend
                        if let Err(e) = self.agent_backend.as_ref().unwrap().assign_task(&agent_id, task, context).await {
                            eprintln!(
                                "[ORCHESTRATOR] Failed to assign task {}: {}",
                                task_id.0, e
                            );
                            continue;
                        }

                        // Track assignment
                        self.active_assignments.insert(
                            task_id.0.clone(),
                            TaskAssignment {
                                task_id: task_id.clone(),
                                agent_id: agent_id.clone(),
                                started_at: Instant::now(),
                            },
                        );

                        // Notify scheduling
                        let _ = self.scheduling.next_task(agent_id.clone());

                        // Store in memory
                        let _ = self.memory.insert(
                            format!("assignment:{}", task_id.0),
                            serde_json::json!({"status": "assigned"}),
                            AgentId::Validator, // system source
                        );

                        eprintln!("[ORCHESTRATOR] Assigned task {} to agent", task_id.0);
                    }
                    Ok(_) => {
                        // No agents available, try scaling
                        let metrics = ScaleMetrics {
                            active_agents: self.active_assignments.len(),
                            idle_agents: self.agent_backend.as_ref().unwrap()
                                .active_agents()
                                .len()
                                .saturating_sub(self.active_assignments.len()),
                            ready_tasks: self.decomposition.ready_tasks().len(),
                            queue_depth: self.decomposition.ready_tasks().len(),
                            avg_task_duration: Duration::from_secs(60),
                            error_rate: 0.0,
                        };

                        if let Ok(decision) = self.scaling.should_scale(&metrics) {
                            if !matches!(decision, ScaleDecision::NoAction) {
                                // In a real implementation, scaling.execute_scale would
                                // need to spawn agents via the backend. For now we skip.
                                eprintln!("[ORCHESTRATOR] Scale decision: {:?}", decision);
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("[ORCHESTRATOR] Assignment error: {}", e);
                    }
                }
            }

            // Poll for agent events
            let next_event_fut = self.agent_backend.as_mut().unwrap().next_event();
            match tokio::time::timeout(self.scheduling.next_interval(), next_event_fut).await {
                Ok(Some(event)) => {
                    self.handle_event(event).await?;
                }
                Ok(None) => {
                    // No more events — backend channel closed
                    eprintln!("[ORCHESTRATOR] Agent backend channel closed");
                    break;
                }
                Err(_) => {
                    // Timeout — continue loop
                }
            }
        }

        // Shutdown all agents
        let active = self.agent_backend.as_ref().unwrap().active_agents();
        for agent_id in active {
            let _ = self.agent_backend.as_ref().unwrap().shutdown(&agent_id).await;
            if let Some(ref mut isolation) = self.isolation {
                let _ = isolation.cleanup(&agent_id);
            }
        }

        Ok(OrchestratorStats {
            tasks_completed: self.tasks_completed,
            tasks_failed: self.tasks_failed,
            total_tasks: self.tasks_completed + self.tasks_failed,
            agents_spawned: self.agents_spawned,
            total_cost_usd: self.total_cost_usd,
            duration: start.elapsed(),
        })
    }

    /// Handle a single agent event.
    async fn handle_event(
        &mut self,
        event: AgentEvent,
    ) -> Result<()> {
        match event {
            AgentEvent::TaskCompleted {
                agent_id,
                task_id,
                result_text,
                cost_usd,
                duration_ms,
            } => {
                self.total_cost_usd += cost_usd;

                // Validate if validation is configured
                let approved = if let Some(ref mut validation) = self.validation {
                    let request = ValidationRequest {
                        task_id: task_id.clone(),
                        agent_id: agent_id.clone(),
                        worktree_path: self
                            .isolation
                            .as_ref()
                            .and_then(|i| i.workspace_path(&agent_id))
                            .unwrap_or_else(|| self.config.working_dir.clone()),
                        base_branch: self.config.base_branch.clone(),
                        task_description: result_text.clone(),
                        verify_cmd: self.config.verify_cmd.clone(),
                        duration_secs: duration_ms as f64 / 1000.0,
                        cost_usd,
                    };

                    match validation.validate(request).await {
                        Ok(report) => {
                            match report.verdict {
                                crate::validation::ValidationVerdict::Approve => true,
                                crate::validation::ValidationVerdict::Reject { reason } => {
                                    eprintln!(
                                        "[ORCHESTRATOR] Task {} rejected: {}",
                                        task_id.0, reason
                                    );
                                    false
                                }
                                crate::validation::ValidationVerdict::NeedsReview { details } => {
                                    eprintln!(
                                        "[ORCHESTRATOR] Task {} needs review: {}",
                                        task_id.0, details
                                    );
                                    false
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!(
                                "[ORCHESTRATOR] Validation error for {}: {}",
                                task_id.0, e
                            );
                            true // Default approve on validation error
                        }
                    }
                } else {
                    true // No validation = auto-approve
                };

                if approved {
                    // Merge if isolation is configured
                    if let Some(ref mut isolation) = self.isolation {
                        match isolation.merge(&agent_id) {
                            Ok(crate::isolation::MergeOutcome::Success { commit_sha }) => {
                                eprintln!(
                                    "[ORCHESTRATOR] Merged task {} ({})",
                                    task_id.0, commit_sha
                                );
                            }
                            Ok(crate::isolation::MergeOutcome::Conflict { files }) => {
                                eprintln!(
                                    "[ORCHESTRATOR] Merge conflict for task {}: {:?}",
                                    task_id.0, files
                                );
                                // Could escalate to strategic advisor here
                            }
                            Ok(crate::isolation::MergeOutcome::NoChanges) => {}
                            Err(e) => {
                                eprintln!("[ORCHESTRATOR] Merge error: {}", e);
                            }
                        }
                    }

                    // Mark complete in decomposition
                    if let Err(e) = self.decomposition.mark_complete(task_id.clone()) {
                        eprintln!("[ORCHESTRATOR] Error marking complete: {}", e);
                    }

                    // Notify topology
                    let result = crate::core::types::TaskResult {
                        status: TaskStatus::Completed,
                        output: result_text,
                        artifacts: vec![],
                        duration: Duration::from_millis(duration_ms),
                        git_sha: None,
                    };
                    self.topology
                        .handle_completion(agent_id.clone(), task_id.clone(), result);

                    // Notify scheduling
                    let _ = self
                        .scheduling
                        .notify_completion(task_id.clone(), agent_id);

                    // Update memory
                    let _ = self.memory.insert(
                        format!("completed:{}", task_id.0),
                        serde_json::json!({"status": "completed", "cost_usd": cost_usd}),
                        AgentId::Validator,
                    );

                    self.tasks_completed += 1;
                    self.active_assignments.remove(&task_id.0);
                    eprintln!(
                        "[ORCHESTRATOR] Task {} completed ({}/{})",
                        task_id.0,
                        self.tasks_completed,
                        self.tasks_completed + self.tasks_failed + self.active_assignments.len()
                    );
                } else {
                    // Rejected — handle as failure for retry
                    self.active_assignments.remove(&task_id.0);
                    let _ = self.resilience.handle_failure(
                        task_id,
                        agent_id,
                        "Validation rejected".to_string(),
                    );
                }
            }

            AgentEvent::TaskFailed {
                agent_id,
                task_id,
                error,
            } => {
                eprintln!("[ORCHESTRATOR] Task {} failed: {}", task_id.0, error);
                self.active_assignments.remove(&task_id.0);

                // Handle failure via resilience
                match self
                    .resilience
                    .handle_failure(task_id.clone(), agent_id.clone(), error.clone())
                {
                    Ok(action) => {
                        match action {
                            crate::resilience::ResilienceAction::Retry { delay } => {
                                eprintln!(
                                    "[ORCHESTRATOR] Retrying task {} after {:?}",
                                    task_id.0, delay
                                );
                                // Task stays in decomposition, will be re-assigned
                            }
                            crate::resilience::ResilienceAction::Escalate { .. } => {
                                // Escalate to strategic advisor
                                if let Some(ref mut advisor) = self.strategic {
                                    let event = StrategicEvent::TaskEscalation {
                                        task_id: task_id.clone(),
                                        retry_count: 1,
                                        reasons: vec![error],
                                    };
                                    match advisor.advise(event).await {
                                        Ok(cmd) => {
                                            eprintln!(
                                                "[ORCHESTRATOR] Strategic advice: {:?}",
                                                cmd
                                            );
                                            // Handle strategic command
                                        }
                                        Err(e) => {
                                            eprintln!(
                                                "[ORCHESTRATOR] Strategic advisor error: {}",
                                                e
                                            );
                                        }
                                    }
                                }
                            }
                            crate::resilience::ResilienceAction::Abandon => {
                                self.tasks_failed += 1;
                                eprintln!("[ORCHESTRATOR] Task {} abandoned", task_id.0);
                            }
                            _ => {}
                        }
                    }
                    Err(e) => {
                        eprintln!("[ORCHESTRATOR] Resilience error: {}", e);
                        self.tasks_failed += 1;
                    }
                }

                // Cleanup workspace
                if let Some(ref mut isolation) = self.isolation {
                    let _ = isolation.cleanup(&agent_id);
                }
            }

            AgentEvent::Progress {
                agent_id,
                task_id,
                turns_completed,
                cost_usd,
            } => {
                self.total_cost_usd = self.total_cost_usd.max(cost_usd);
                // Store progress in memory
                let _ = self.memory.insert(
                    format!("progress:{}", task_id.0),
                    serde_json::json!({
                        "turns": turns_completed,
                        "cost_usd": cost_usd,
                    }),
                    agent_id,
                );
            }

            AgentEvent::ProcessDied { agent_id, exit_code } => {
                eprintln!(
                    "[ORCHESTRATOR] Agent process died (exit: {:?})",
                    exit_code
                );

                // Find the task this agent was working on
                let task_id = self
                    .active_assignments
                    .iter()
                    .find(|(_, a)| {
                        matches!(&a.agent_id, id if format!("{:?}", id) == format!("{:?}", agent_id))
                    })
                    .map(|(k, _)| k.clone());

                if let Some(tid) = task_id {
                    self.active_assignments.remove(&tid);
                    // Resilience handles recovery
                    let _ = self.resilience.handle_failure(
                        TaskId(tid),
                        agent_id.clone(),
                        format!("Process died with exit code {:?}", exit_code),
                    );
                }

                // Remove from topology
                let _ = self.topology.remove_agent(agent_id);
            }
        }

        Ok(())
    }
}
