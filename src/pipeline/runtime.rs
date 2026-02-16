//! Pipeline runtime execution engine.

use super::Pipeline;
use crate::core::types::{AgentId, QueenId, Task, TaskId, TaskStatus};
use crate::scaling::{ScaleDecision, ScaleMetrics};
use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Runtime status of the pipeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeStatus {
    /// Pipeline created but not started
    Created,
    /// Pipeline is actively running
    Running,
    /// Pipeline execution is paused
    Paused,
    /// Pipeline completed successfully
    Completed,
    /// Pipeline failed with an error
    Failed { error: String },
}

/// Runtime statistics collected during execution.
#[derive(Debug, Clone)]
pub struct RuntimeStats {
    /// Total tasks submitted to the pipeline
    pub tasks_submitted: usize,
    /// Tasks successfully completed
    pub tasks_completed: usize,
    /// Tasks that failed
    pub tasks_failed: usize,
    /// Number of agents spawned during execution
    pub agents_spawned: usize,
    /// Number of agents terminated
    pub agents_terminated: usize,
    /// Total messages exchanged between agents
    pub total_messages: usize,
    /// Total runtime duration
    pub uptime: Duration,
}

impl RuntimeStats {
    fn new() -> Self {
        RuntimeStats {
            tasks_submitted: 0,
            tasks_completed: 0,
            tasks_failed: 0,
            agents_spawned: 0,
            agents_terminated: 0,
            total_messages: 0,
            uptime: Duration::from_secs(0),
        }
    }
}

/// Runtime execution engine for a pipeline.
pub struct PipelineRuntime {
    pipeline: Pipeline,
    status: RuntimeStatus,
    started_at: Option<Instant>,
    stats: RuntimeStats,
    /// Agent registry: agent_id -> is_active
    agents: HashMap<String, bool>,
    /// Task tracking: task_id -> (assigned_agent, start_time)
    task_assignments: HashMap<String, (String, Instant)>,
    /// Iteration counter for main loop
    iteration: u64,
}

impl PipelineRuntime {
    /// Create a new runtime with the given pipeline.
    pub fn new(pipeline: Pipeline) -> Self {
        PipelineRuntime {
            pipeline,
            status: RuntimeStatus::Created,
            started_at: None,
            stats: RuntimeStats::new(),
            agents: HashMap::new(),
            task_assignments: HashMap::new(),
            iteration: 0,
        }
    }

    /// Get the current runtime status.
    pub fn status(&self) -> &RuntimeStatus {
        &self.status
    }

    /// Get the current runtime statistics.
    pub fn stats(&self) -> &RuntimeStats {
        &self.stats
    }

    /// Pause execution (can be resumed later).
    pub fn pause(&mut self) {
        if self.status == RuntimeStatus::Running {
            self.status = RuntimeStatus::Paused;
            eprintln!("[PipelineRuntime] Pipeline paused");
        }
    }

    /// Resume execution after a pause.
    pub fn resume(&mut self) {
        if self.status == RuntimeStatus::Paused {
            self.status = RuntimeStatus::Running;
            eprintln!("[PipelineRuntime] Pipeline resumed");
        }
    }

    /// Hot-swap the topology component while running.
    pub fn swap_topology(&mut self, t: Box<dyn crate::topology::Topology>) {
        let old = self.pipeline.swap_topology(t);
        eprintln!(
            "[PipelineRuntime] Hot-swapped topology: {} -> {}",
            std::any::type_name_of_val(&*old)
                .split("::")
                .last()
                .unwrap_or("Unknown"),
            std::any::type_name_of_val(&*self.pipeline.topology)
                .split("::")
                .last()
                .unwrap_or("Unknown")
        );
    }

    /// Submit a new task at runtime.
    pub fn submit_task(&mut self, task: Task) -> Result<TaskId> {
        let task_id = task.id.clone();

        // Decompose the task
        let subtasks = self.pipeline.decomposition.decompose(&task)?;

        self.stats.tasks_submitted += 1 + subtasks.len();

        eprintln!(
            "[PipelineRuntime] Submitted task {} -> {} subtasks",
            task_id.0,
            subtasks.len()
        );

        Ok(task_id)
    }

    /// Main execution loop.
    ///
    /// Orchestrates the pipeline components in a continuous cycle:
    /// 1. Check scaling needs and adjust agent pool
    /// 2. Get ready tasks from decomposition
    /// 3. Assign tasks via topology
    /// 4. Handle task completions
    /// 5. Apply resilience strategies on failures
    /// 6. Store results in memory
    /// 7. Check termination condition from scheduler
    pub async fn run(&mut self, tasks: Vec<Task>) -> Result<RuntimeStats> {
        self.status = RuntimeStatus::Running;
        self.started_at = Some(Instant::now());

        eprintln!(
            "[PipelineRuntime] Starting pipeline '{}' with {} initial tasks",
            self.pipeline.name,
            tasks.len()
        );

        // Submit all initial tasks to decomposition
        for task in tasks {
            let subtasks = self.pipeline.decomposition.decompose(&task)?;
            self.stats.tasks_submitted += 1 + subtasks.len();
        }

        // Initialize agent pool
        self.spawn_initial_agents()?;

        // Main orchestration loop
        loop {
            self.iteration += 1;

            // Check if paused
            if self.status == RuntimeStatus::Paused {
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            }

            // 1. Check scaling needs
            let metrics = self.collect_metrics();
            let scale_decision = self.pipeline.scaling.should_scale(&metrics)?;

            if !matches!(scale_decision, ScaleDecision::NoAction) {
                self.execute_scaling(scale_decision)?;
            }

            // 2. Get ready tasks from decomposition
            let ready_tasks = self.pipeline.decomposition.ready_tasks();

            // 3. Assign tasks via topology
            for task_id in ready_tasks {
                let assigned_agents = self.pipeline.topology.assign_task(&task_id)?;

                for agent_id in assigned_agents {
                    let agent_key = agent_id_to_key(&agent_id);
                    self.task_assignments
                        .insert(task_id.0.clone(), (agent_key.clone(), Instant::now()));
                    eprintln!(
                        "[PipelineRuntime] Assigned task {} to agent {}",
                        task_id.0, agent_key
                    );
                }
            }

            // 4. Simulate task execution (in real system, agents would report back)
            // For this simplified version, we complete tasks after a delay
            self.process_task_completions().await?;

            // 5. Check termination condition
            if self.pipeline.scheduling.should_terminate() {
                eprintln!("[PipelineRuntime] Scheduler signaled termination");
                break;
            }

            // Wait for next scheduling interval
            let interval = self.pipeline.scheduling.next_interval();
            if interval > Duration::from_millis(0) {
                tokio::time::sleep(interval).await;
            }

            // Safety: prevent infinite loops during development
            if self.iteration > 10000 {
                eprintln!("[PipelineRuntime] WARNING: Max iterations reached, terminating");
                break;
            }
        }

        self.status = RuntimeStatus::Completed;

        if let Some(start) = self.started_at {
            self.stats.uptime = start.elapsed();
        }

        eprintln!(
            "[PipelineRuntime] Pipeline completed. Stats: {} submitted, {} completed, {} failed",
            self.stats.tasks_submitted, self.stats.tasks_completed, self.stats.tasks_failed
        );

        Ok(self.stats.clone())
    }

    /// Spawn initial agents for the pipeline.
    fn spawn_initial_agents(&mut self) -> Result<()> {
        // Start with 3 agents
        for i in 0..3 {
            let agent_id = AgentId::Queen(QueenId(format!("Q{}", i)));
            let agent_key = agent_id_to_key(&agent_id);

            self.pipeline.topology.add_agent(agent_id)?;
            self.agents.insert(agent_key.clone(), true);
            self.stats.agents_spawned += 1;

            eprintln!("[PipelineRuntime] Spawned agent {}", agent_key);
        }

        Ok(())
    }

    /// Collect current metrics for scaling decisions.
    fn collect_metrics(&self) -> ScaleMetrics {
        let total_agents = self.agents.len();
        let active_agents = self.task_assignments.len();
        let idle_agents = total_agents.saturating_sub(active_agents);
        let ready_tasks = self.pipeline.decomposition.ready_tasks().len();

        ScaleMetrics {
            active_agents,
            idle_agents,
            ready_tasks,
            queue_depth: ready_tasks,
            avg_task_duration: Duration::from_secs(10), // Placeholder
            error_rate: if self.stats.tasks_submitted > 0 {
                self.stats.tasks_failed as f64 / self.stats.tasks_submitted as f64
            } else {
                0.0
            },
        }
    }

    /// Execute a scaling decision.
    fn execute_scaling(&mut self, decision: ScaleDecision) -> Result<()> {
        match decision {
            ScaleDecision::ScaleUp { count } => {
                eprintln!("[PipelineRuntime] Scaling up by {} agents", count);
                for _ in 0..count {
                    let agent_id = AgentId::Queen(QueenId(format!(
                        "Q{}",
                        self.stats.agents_spawned
                    )));
                    let agent_key = agent_id_to_key(&agent_id);

                    self.pipeline.topology.add_agent(agent_id)?;
                    self.agents.insert(agent_key, true);
                    self.stats.agents_spawned += 1;
                }
            }
            ScaleDecision::ScaleDown { count } => {
                eprintln!("[PipelineRuntime] Scaling down by {} agents", count);
                let idle_agents: Vec<String> = self
                    .agents
                    .iter()
                    .filter(|(key, _)| {
                        !self
                            .task_assignments
                            .values()
                            .any(|(agent_key, _)| agent_key == *key)
                    })
                    .take(count)
                    .map(|(key, _)| key.clone())
                    .collect();

                for agent_key in idle_agents {
                    if let Ok(agent_id) = key_to_agent_id(&agent_key) {
                        self.pipeline.topology.remove_agent(agent_id)?;
                        self.agents.remove(&agent_key);
                        self.stats.agents_terminated += 1;
                    }
                }
            }
            ScaleDecision::NoAction => {}
        }

        Ok(())
    }

    /// Process task completions (simplified simulation).
    async fn process_task_completions(&mut self) -> Result<()> {
        let now = Instant::now();
        let completed: Vec<(String, String)> = self
            .task_assignments
            .iter()
            .filter(|(_, (_, start_time))| now.duration_since(*start_time) > Duration::from_secs(1))
            .take(2) // Complete up to 2 tasks per iteration
            .map(|(task_id, (agent_key, _))| (task_id.clone(), agent_key.clone()))
            .collect();

        for (task_id_str, agent_key) in completed {
            let task_id = TaskId(task_id_str.clone());
            let agent_id = key_to_agent_id(&agent_key)?;

            // Mark task as complete in decomposition
            self.pipeline.decomposition.mark_complete(task_id.clone())?;

            // Notify scheduler
            self.pipeline
                .scheduling
                .notify_completion(task_id.clone(), agent_id.clone())?;

            // Notify topology
            self.pipeline.topology.handle_completion(
                agent_id,
                task_id,
                crate::core::types::TaskResult {
                    status: TaskStatus::Completed,
                    output: "Task completed successfully".to_string(),
                    artifacts: vec![],
                    duration: Duration::from_secs(1),
                    git_sha: None,
                },
            );

            self.task_assignments.remove(&task_id_str);
            self.stats.tasks_completed += 1;
        }

        Ok(())
    }
}

/// Convert AgentId to string key.
fn agent_id_to_key(agent_id: &AgentId) -> String {
    match agent_id {
        AgentId::Nydus(id) => format!("nydus:{}", id.0),
        AgentId::Queen(id) => format!("queen:{}", id.0),
        AgentId::Overlord(id) => format!("overlord:{}", id),
        AgentId::Overmind(id) => format!("overmind:{}", id),
        AgentId::Validator => "validator".to_string(),
        AgentId::Operator => "operator".to_string(),
    }
}

/// Convert string key back to AgentId.
fn key_to_agent_id(key: &str) -> Result<AgentId> {
    if let Some(id) = key.strip_prefix("queen:") {
        Ok(AgentId::Queen(QueenId(id.to_string())))
    } else if let Some(id) = key.strip_prefix("nydus:") {
        Ok(AgentId::Nydus(crate::core::types::NydusId(id.to_string())))
    } else if let Some(id) = key.strip_prefix("overlord:") {
        Ok(AgentId::Overlord(crate::core::types::OverlordId(
            id.to_string(),
        )))
    } else if let Some(id) = key.strip_prefix("overmind:") {
        Ok(AgentId::Overmind(crate::core::types::OvermindId(
            id.to_string(),
        )))
    } else if key == "validator" {
        Ok(AgentId::Validator)
    } else if key == "operator" {
        Ok(AgentId::Operator)
    } else {
        Err(anyhow!("Invalid agent key: {}", key))
    }
}
