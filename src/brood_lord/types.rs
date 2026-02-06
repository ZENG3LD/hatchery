//! Brood Lord–specific types.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub type L2Id = usize;

/// Sub-PRD created by Opus Manager's decomposition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubPrd {
    pub id: L2Id,
    pub name: String,
    /// Markdown content (standalone PRD with checkboxes).
    pub content: String,
    /// IDs of other sub-PRDs this one depends on.
    pub cross_deps: Vec<L2Id>,
    /// How many workers Opus recommends for this sub-PRD.
    pub worker_count: usize,
}

/// Opus Manager decomposition result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Decomposition {
    pub sub_prds: Vec<SubPrd>,
    pub rationale: String,
}

/// Status of a single L2 sub-swarm.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct L2Status {
    pub l2_id: L2Id,
    pub name: String,
    pub tasks_completed: usize,
    pub tasks_total: usize,
    pub workers_active: usize,
    pub workers_total: usize,
    pub elapsed_secs: u64,
    pub last_update: DateTime<Utc>,
    pub state: L2State,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum L2State {
    Pending,
    Running,
    Completed,
    Failed,
    Stalled,
}

/// Global shared memory state for cross-L2 coordination.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalState {
    pub version: u32,
    /// Cross-swarm knowledge, namespaced: "L2.0.auth.method"
    pub knowledge: std::collections::HashMap<String, serde_json::Value>,
    /// Status of each L2 sub-swarm.
    pub l2_status: std::collections::HashMap<L2Id, L2Status>,
    /// Messages between L2s / Opus.
    pub messages: std::collections::VecDeque<CrossMessage>,
    pub metadata: GlobalMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossMessage {
    pub from: L2Id,
    pub to: CrossTarget,
    pub text: String,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CrossTarget {
    L2(L2Id),
    AllL2s,
    OpusManager,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalMetadata {
    pub l2_count: usize,
    pub opus_model: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// JSON command from Opus Manager.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "cmd")]
pub enum OpusCommand {
    /// Decompose master PRD.
    #[serde(rename = "decomposition")]
    Decomposition { sub_prds: Vec<SubPrd>, rationale: String },
    /// Spawn an L2 sub-swarm.
    #[serde(rename = "spawn_l2")]
    SpawnL2 { l2_id: L2Id, workers: usize },
    /// Send message between L2s.
    #[serde(rename = "send_message")]
    SendMessage { to: String, text: String },
    /// Escalate to Lead.
    #[serde(rename = "escalate")]
    Escalate { l2_id: L2Id, issue: String },
    /// Report progress.
    #[serde(rename = "progress")]
    Progress,
    /// Add workers to L2.
    #[serde(rename = "add_workers")]
    AddWorkers { l2_id: L2Id, count: usize },
    /// Kill L2.
    #[serde(rename = "kill_l2")]
    KillL2 { l2_id: L2Id },
}
