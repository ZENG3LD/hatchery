//! Elastic pool with dynamic spawn/teardown and zerg rush mode.
//!
//! Features:
//! - Dynamic agent lifecycle management (spawn, drain, terminate)
//! - Zerg rush: instant scale to max agents when queue explodes
//! - Graceful draining: let agents finish current work before removal
//! - Agent efficiency tracking

use super::{generate_queen_id, ScaleDecision, ScaleMetrics, Scaling};
use crate::core::types::AgentId;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant};

// ============================================================================
// Configuration
// ============================================================================

/// Configuration for elastic pool scaling.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElasticPoolConfig {
    /// Minimum number of agents to maintain
    pub min_agents: usize,
    /// Maximum number of agents to spawn
    pub max_agents: usize,
    /// Enable zerg rush mode
    pub zerg_rush_enabled: bool,
    /// Ready tasks threshold to trigger zerg rush
    pub zerg_rush_threshold: usize,
    /// Percentage to scale up by (0.0 to 1.0)
    pub scale_up_percent: f64,
    /// Cooldown period after scale down
    pub scale_down_cooldown: Duration,
}

impl Default for ElasticPoolConfig {
    fn default() -> Self {
        ElasticPoolConfig {
            min_agents: 3,
            max_agents: 50,
            zerg_rush_enabled: true,
            zerg_rush_threshold: 20,
            scale_up_percent: 0.5, // 50% increment
            scale_down_cooldown: Duration::from_secs(60),
        }
    }
}

impl ElasticPoolConfig {
    /// Create a new config.
    pub fn new(min_agents: usize, max_agents: usize) -> Self {
        ElasticPoolConfig {
            min_agents,
            max_agents,
            ..Default::default()
        }
    }

    /// Validate configuration.
    pub fn validate(&self) -> Result<()> {
        if self.min_agents == 0 {
            anyhow::bail!("min_agents must be at least 1");
        }
        if self.max_agents < self.min_agents {
            anyhow::bail!("max_agents must be >= min_agents");
        }
        if self.zerg_rush_threshold == 0 {
            anyhow::bail!("zerg_rush_threshold must be at least 1");
        }
        if self.scale_up_percent <= 0.0 || self.scale_up_percent > 2.0 {
            anyhow::bail!("scale_up_percent must be between 0.0 and 2.0");
        }
        Ok(())
    }
}

// ============================================================================
// PoolAgent
// ============================================================================

/// Status of an agent in the pool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PoolAgentStatus {
    /// Active and accepting tasks
    Active,
    /// Idle and waiting for tasks
    Idle,
    /// Draining (finishing current work, no new tasks)
    Draining,
    /// Terminated and removed from pool
    Terminated,
}

/// An agent in the elastic pool.
#[derive(Debug, Clone)]
pub struct PoolAgent {
    /// Agent ID
    pub id: AgentId,
    /// Current status
    pub status: PoolAgentStatus,
    /// When this agent was spawned
    pub spawned_at: Instant,
    /// Number of tasks completed
    pub tasks_completed: usize,
}

impl PoolAgent {
    /// Create a new pool agent.
    pub fn new(id: AgentId) -> Self {
        PoolAgent {
            id,
            status: PoolAgentStatus::Active,
            spawned_at: Instant::now(),
            tasks_completed: 0,
        }
    }

    /// Get agent uptime.
    pub fn uptime(&self) -> Duration {
        self.spawned_at.elapsed()
    }

    /// Calculate efficiency (tasks per minute).
    pub fn efficiency(&self) -> f64 {
        let uptime_mins = self.uptime().as_secs_f64() / 60.0;
        if uptime_mins == 0.0 {
            return 0.0;
        }
        self.tasks_completed as f64 / uptime_mins
    }

    /// Mark agent as draining.
    pub fn drain(&mut self) {
        self.status = PoolAgentStatus::Draining;
    }

    /// Mark agent as terminated.
    pub fn terminate(&mut self) {
        self.status = PoolAgentStatus::Terminated;
    }
}

// ============================================================================
// PoolState
// ============================================================================

/// State of the elastic pool.
#[derive(Debug, Clone)]
pub struct PoolState {
    /// All agents in the pool (by string key)
    pub agents: HashMap<String, PoolAgent>,
    /// When scale down last occurred
    pub last_scale_down: Option<Instant>,
    /// Whether zerg rush is currently active
    pub zerg_rush_active: bool,
}

impl PoolState {
    /// Create a new pool state.
    pub fn new() -> Self {
        PoolState {
            agents: HashMap::new(),
            last_scale_down: None,
            zerg_rush_active: false,
        }
    }

    /// Get active agent count.
    pub fn active_count(&self) -> usize {
        self.agents.values()
            .filter(|a| a.status == PoolAgentStatus::Active)
            .count()
    }

    /// Get idle agent count.
    pub fn idle_count(&self) -> usize {
        self.agents.values()
            .filter(|a| a.status == PoolAgentStatus::Idle)
            .count()
    }

    /// Get draining agent count.
    pub fn draining_count(&self) -> usize {
        self.agents.values()
            .filter(|a| a.status == PoolAgentStatus::Draining)
            .count()
    }

    /// Get total agent count (excluding terminated).
    pub fn total_count(&self) -> usize {
        self.agents.values()
            .filter(|a| a.status != PoolAgentStatus::Terminated)
            .count()
    }

    /// Calculate pool utilization (active / total).
    pub fn utilization(&self) -> f64 {
        let total = self.total_count();
        if total == 0 {
            return 0.0;
        }
        self.active_count() as f64 / total as f64
    }

    /// Calculate average agent efficiency.
    pub fn average_efficiency(&self) -> f64 {
        let efficiencies: Vec<f64> = self.agents.values()
            .filter(|a| a.status != PoolAgentStatus::Terminated)
            .map(|a| a.efficiency())
            .collect();

        if efficiencies.is_empty() {
            return 0.0;
        }

        efficiencies.iter().sum::<f64>() / efficiencies.len() as f64
    }
}

impl Default for PoolState {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// ElasticPoolScaling
// ============================================================================

/// Elastic pool scaling with zerg rush mode.
pub struct ElasticPoolScaling {
    config: ElasticPoolConfig,
    state: PoolState,
    next_agent_number: usize,
}

impl ElasticPoolScaling {
    /// Create a new elastic pool scaling instance.
    pub fn new(config: ElasticPoolConfig) -> Result<Self> {
        config.validate()?;
        Ok(ElasticPoolScaling {
            config,
            state: PoolState::new(),
            next_agent_number: 0,
        })
    }

    /// Create with default configuration.
    pub fn default() -> Result<Self> {
        Self::new(ElasticPoolConfig::default())
    }

    /// Check if in scale down cooldown.
    fn in_cooldown(&self) -> bool {
        if let Some(last) = self.state.last_scale_down {
            last.elapsed() < self.config.scale_down_cooldown
        } else {
            false
        }
    }

    /// Trigger zerg rush (instant scale to max).
    pub fn trigger_zerg_rush(&mut self) -> Result<Vec<AgentId>> {
        let current = self.state.total_count();
        let to_spawn = self.config.max_agents.saturating_sub(current);

        eprintln!("[ElasticPoolScaling] ZERG RUSH! Spawning {} agents to reach max capacity",
            to_spawn);

        self.state.zerg_rush_active = true;
        self.spawn_agents(to_spawn)
    }

    /// Spawn N new agents.
    fn spawn_agents(&mut self, count: usize) -> Result<Vec<AgentId>> {
        let mut spawned = Vec::with_capacity(count);

        for _ in 0..count {
            let agent_id = generate_queen_id(&format!("pool-Q{}", self.next_agent_number));
            self.next_agent_number += 1;

            let pool_agent = PoolAgent::new(agent_id.clone());
            let key = super::agent_id_to_key(&agent_id);
            self.state.agents.insert(key, pool_agent);

            spawned.push(agent_id);
        }

        Ok(spawned)
    }

    /// Drain N agents (graceful removal).
    pub fn drain_agents(&mut self, count: usize) -> Result<Vec<AgentId>> {
        let mut drained = Vec::with_capacity(count);

        // Find idle agents to drain first
        let idle_keys: Vec<String> = self.state.agents.iter()
            .filter(|(_, a)| a.status == PoolAgentStatus::Idle)
            .map(|(k, _)| k.clone())
            .take(count)
            .collect();

        for key in idle_keys {
            if let Some(agent) = self.state.agents.get_mut(&key) {
                agent.drain();
                drained.push(agent.id.clone());
            }
        }

        // If not enough idle agents, drain active ones
        if drained.len() < count {
            let remaining = count - drained.len();
            let active_keys: Vec<String> = self.state.agents.iter()
                .filter(|(_, a)| a.status == PoolAgentStatus::Active)
                .map(|(k, _)| k.clone())
                .take(remaining)
                .collect();

            for key in active_keys {
                if let Some(agent) = self.state.agents.get_mut(&key) {
                    agent.drain();
                    drained.push(agent.id.clone());
                }
            }
        }

        self.state.last_scale_down = Some(Instant::now());
        Ok(drained)
    }

    /// Remove terminated agents from the pool.
    pub fn cleanup_terminated(&mut self) {
        self.state.agents.retain(|_, agent| {
            agent.status != PoolAgentStatus::Terminated
        });
    }

    /// Calculate scale up count.
    fn calculate_scale_up(&self, metrics: &ScaleMetrics) -> usize {
        let current = self.state.total_count();
        if current >= self.config.max_agents {
            return 0;
        }

        // Check for zerg rush
        if self.config.zerg_rush_enabled && metrics.ready_tasks >= self.config.zerg_rush_threshold {
            return self.config.max_agents.saturating_sub(current);
        }

        // Normal scale up: queue depth exceeds idle agents
        if metrics.queue_depth > self.state.idle_count() {
            let increment = ((current as f64 * self.config.scale_up_percent).ceil() as usize)
                .max(1);
            increment.min(self.config.max_agents - current)
        } else {
            0
        }
    }

    /// Calculate scale down count.
    fn calculate_scale_down(&self, _metrics: &ScaleMetrics) -> usize {
        let current = self.state.total_count();
        if current <= self.config.min_agents || self.in_cooldown() {
            return 0;
        }

        // Scale down if idle count exceeds threshold
        if self.state.idle_count() > current / 2 {
            let to_remove = self.state.idle_count() / 2;
            to_remove.min(current - self.config.min_agents)
        } else {
            0
        }
    }
}

impl Scaling for ElasticPoolScaling {
    fn should_scale(&self, metrics: &ScaleMetrics) -> Result<ScaleDecision> {
        // Check scale up first (includes zerg rush detection)
        let scale_up_count = self.calculate_scale_up(metrics);
        if scale_up_count > 0 {
            return Ok(ScaleDecision::ScaleUp { count: scale_up_count });
        }

        // Then check scale down
        let scale_down_count = self.calculate_scale_down(metrics);
        if scale_down_count > 0 {
            return Ok(ScaleDecision::ScaleDown { count: scale_down_count });
        }

        Ok(ScaleDecision::NoAction)
    }

    fn execute_scale(&mut self, decision: ScaleDecision) -> Result<Vec<AgentId>> {
        match decision {
            ScaleDecision::ScaleUp { count } => {
                let spawned = self.spawn_agents(count)?;

                let is_zerg_rush = count >= (self.config.max_agents - self.state.total_count());
                if is_zerg_rush {
                    self.state.zerg_rush_active = true;
                    eprintln!("[ElasticPoolScaling] ZERG RUSH activated! Spawned {} agents (total: {}/{})",
                        spawned.len(), self.state.total_count(), self.config.max_agents);
                } else {
                    eprintln!("[ElasticPoolScaling] Scaled up by {} agents (total: {}, utilization: {:.1}%)",
                        spawned.len(), self.state.total_count(), self.state.utilization() * 100.0);
                }

                Ok(spawned)
            }
            ScaleDecision::ScaleDown { count } => {
                let drained = self.drain_agents(count)?;

                // Deactivate zerg rush if scaling down
                if self.state.zerg_rush_active {
                    self.state.zerg_rush_active = false;
                }

                eprintln!("[ElasticPoolScaling] Scaled down by {} agents (draining: {}, total: {})",
                    drained.len(), self.state.draining_count(), self.state.total_count());
                Ok(drained)
            }
            ScaleDecision::NoAction => Ok(Vec::new()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_metrics(active: usize, idle: usize, ready: usize) -> ScaleMetrics {
        ScaleMetrics {
            active_agents: active,
            idle_agents: idle,
            ready_tasks: ready,
            queue_depth: ready,
            avg_task_duration: Duration::from_secs(60),
            error_rate: 0.0,
        }
    }

    #[test]
    fn test_pool_agent() {
        let agent_id = generate_queen_id("test");
        let mut agent = PoolAgent::new(agent_id);

        assert_eq!(agent.status, PoolAgentStatus::Active);
        assert_eq!(agent.tasks_completed, 0);

        agent.drain();
        assert_eq!(agent.status, PoolAgentStatus::Draining);
    }

    #[test]
    fn test_pool_state() {
        let mut state = PoolState::new();

        let agent1 = PoolAgent::new(generate_queen_id("agent1"));
        let agent2 = PoolAgent::new(generate_queen_id("agent2"));

        state.agents.insert("key1".to_string(), agent1);
        state.agents.insert("key2".to_string(), agent2);

        assert_eq!(state.total_count(), 2);
        assert_eq!(state.active_count(), 2);
    }

    #[test]
    fn test_zerg_rush_trigger() {
        let mut config = ElasticPoolConfig::default();
        config.zerg_rush_threshold = 10;
        config.max_agents = 20;

        let mut scaling = ElasticPoolScaling::new(config).unwrap();

        // Normal load: 5 ready tasks
        let metrics = make_metrics(3, 0, 5);
        let decision = scaling.should_scale(&metrics).unwrap();
        assert!(decision.is_scale_up());
        assert!(decision.count() < 20); // Normal scale up

        // Zerg rush: 15 ready tasks (> threshold)
        let metrics2 = make_metrics(3, 0, 15);
        let decision2 = scaling.should_scale(&metrics2).unwrap();
        assert!(decision2.is_scale_up());
        // Should scale to max
    }

    #[test]
    fn test_drain_agents() {
        let mut scaling = ElasticPoolScaling::default().unwrap();

        // Spawn some agents
        scaling.spawn_agents(5).unwrap();

        // Mark some as idle
        for (i, (_key, agent)) in scaling.state.agents.iter_mut().enumerate() {
            if i < 2 {
                agent.status = PoolAgentStatus::Idle;
            }
        }

        // Drain 3 agents
        let drained = scaling.drain_agents(3).unwrap();
        assert_eq!(drained.len(), 3);
        assert_eq!(scaling.state.draining_count(), 3);
    }

    #[test]
    fn test_scale_down_cooldown() {
        let mut config = ElasticPoolConfig::default();
        config.scale_down_cooldown = Duration::from_millis(100);
        let mut scaling = ElasticPoolScaling::new(config).unwrap();

        // Spawn agents
        scaling.spawn_agents(10).unwrap();

        // First scale down
        let metrics = make_metrics(2, 8, 0);
        let decision = scaling.should_scale(&metrics).unwrap();
        assert!(decision.is_scale_down());
        scaling.execute_scale(decision).unwrap();

        // Immediate second scale down should be blocked
        let decision2 = scaling.should_scale(&metrics).unwrap();
        assert!(decision2.is_no_action());

        // After cooldown, should work
        std::thread::sleep(Duration::from_millis(150));
        let decision3 = scaling.should_scale(&metrics).unwrap();
        assert!(decision3.is_scale_down());
    }

    #[test]
    fn test_pool_utilization() {
        let mut state = PoolState::new();

        let mut agent1 = PoolAgent::new(generate_queen_id("a1"));
        agent1.status = PoolAgentStatus::Active;
        let mut agent2 = PoolAgent::new(generate_queen_id("a2"));
        agent2.status = PoolAgentStatus::Idle;

        state.agents.insert("k1".to_string(), agent1);
        state.agents.insert("k2".to_string(), agent2);

        assert_eq!(state.utilization(), 0.5); // 1 active / 2 total
    }
}
