//! Scaling strategies for Hatchery swarm orchestration.
//!
//! Provides multiple scaling implementations for different deployment sizes:
//! - SmallScale: 3-8 agents (simple predictable scaling)
//! - MediumScale: 10-100 agents (gradual scaling with cooldown)
//! - LargeScale: 100-1000 agents (hierarchical coordinator tiers)
//! - VeryLargeScale: 1000+ agents (P2P mesh with partitions)
//! - EdgeCloud: Hybrid edge/cloud deployment
//! - ElasticPool: Dynamic spawn/teardown with zerg rush mode

use crate::core::types::AgentId;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::time::Duration;

// Re-export all scaling implementations
pub mod small;
pub mod medium;
pub mod large;
pub mod very_large;
pub mod edge_cloud;
pub mod elastic_pool;

pub use small::{SmallScaleConfig, SmallScaleScaling};
pub use medium::{MediumScaleConfig, MediumScaleScaling};
pub use large::{LargeScaleConfig, LargeScaleScaling};
pub use very_large::{VeryLargeScaleConfig, VeryLargeScaleScaling};
pub use edge_cloud::{EdgeCloudConfig, EdgeCloudScaling};
pub use elastic_pool::{ElasticPoolConfig, ElasticPoolScaling};

// ============================================================================
// Core Trait
// ============================================================================

/// Trait for implementing scaling strategies.
pub trait Scaling: Send + Sync {
    /// Determines if scaling action is needed based on current metrics.
    fn should_scale(&self, metrics: &ScaleMetrics) -> Result<ScaleDecision>;

    /// Executes a scaling decision, returning the list of affected agents.
    fn execute_scale(&mut self, decision: ScaleDecision) -> Result<Vec<AgentId>>;
}

// ============================================================================
// Core Types
// ============================================================================

/// Metrics used to make scaling decisions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScaleMetrics {
    /// Number of agents currently working on tasks
    pub active_agents: usize,
    /// Number of agents that are idle
    pub idle_agents: usize,
    /// Number of tasks ready to be assigned
    pub ready_tasks: usize,
    /// Total tasks waiting in queue (ready + blocked)
    pub queue_depth: usize,
    /// Average duration of completed tasks
    pub avg_task_duration: Duration,
    /// Error rate (errors / total tasks)
    pub error_rate: f64,
}

impl ScaleMetrics {
    /// Calculate total number of agents.
    pub fn total_agents(&self) -> usize {
        self.active_agents + self.idle_agents
    }

    /// Calculate utilization (0.0 to 1.0).
    pub fn utilization(&self) -> f64 {
        let total = self.total_agents();
        if total == 0 {
            return 0.0;
        }
        self.active_agents as f64 / total as f64
    }

    /// Calculate queue pressure (ready_tasks per active agent).
    pub fn queue_pressure(&self) -> f64 {
        if self.active_agents == 0 {
            return if self.ready_tasks > 0 { f64::INFINITY } else { 0.0 };
        }
        self.ready_tasks as f64 / self.active_agents as f64
    }
}

/// Decision on whether to scale up, down, or not at all.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScaleDecision {
    /// Scale up by adding N agents
    ScaleUp { count: usize },
    /// Scale down by removing N agents
    ScaleDown { count: usize },
    /// No scaling action needed
    NoAction,
}

impl ScaleDecision {
    /// Check if this is a scale up decision.
    pub fn is_scale_up(&self) -> bool {
        matches!(self, ScaleDecision::ScaleUp { .. })
    }

    /// Check if this is a scale down decision.
    pub fn is_scale_down(&self) -> bool {
        matches!(self, ScaleDecision::ScaleDown { .. })
    }

    /// Check if this is a no-action decision.
    pub fn is_no_action(&self) -> bool {
        matches!(self, ScaleDecision::NoAction)
    }

    /// Get the count for scale up/down, or 0 for no-action.
    pub fn count(&self) -> usize {
        match self {
            ScaleDecision::ScaleUp { count } => *count,
            ScaleDecision::ScaleDown { count } => *count,
            ScaleDecision::NoAction => 0,
        }
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Generate a unique Queen ID with the given prefix.
pub(crate) fn generate_queen_id(prefix: &str) -> AgentId {
    let id = uuid::Uuid::new_v4().to_string();
    AgentId::Queen(crate::core::types::QueenId(format!("{}-{}", prefix, &id[..8])))
}

/// Convert AgentId to string key for HashMap lookups.
/// AgentId doesn't implement Hash, so we use string representations.
pub(crate) fn agent_id_to_key(id: &AgentId) -> String {
    match id {
        AgentId::Nydus(nydus_id) => format!("nydus:{}", nydus_id.0),
        AgentId::Queen(queen_id) => format!("queen:{}", queen_id.0),
        AgentId::Overlord(overlord_id) => format!("overlord:{}", overlord_id.0),
        AgentId::Overmind(overmind_id) => format!("overmind:{}", overmind_id.0),
        AgentId::Validator => "validator".to_string(),
        AgentId::Operator => "operator".to_string(),
    }
}
