//! Topology module — defines how agents are organized and how tasks are assigned.
//!
//! This module provides multiple topology implementations for different orchestration patterns:
//! - Centralized: Single coordinator assigns tasks round-robin
//! - Hierarchical: Tree structure with coordinators and leaf agents
//! - Peer-to-peer: Flat swarm with consensus-based assignment
//! - Blackboard: Shared semantic space with agent self-selection
//! - GraphDAG: DAG-based topology with dependency tracking
//! - Conversational: Multi-round debate before assignment
//! - Hybrid: Composes multiple topologies based on task properties

use crate::core::types::{AgentId, TaskId, TaskResult};
use anyhow::Result;

/// Topology defines how agents are organized and how tasks are assigned.
pub trait Topology: Send + Sync {
    /// Assign a task to one or more agents based on the topology's strategy.
    fn assign_task(&mut self, task_id: &TaskId) -> Result<Vec<AgentId>>;

    /// Handle task completion notification from an agent.
    fn handle_completion(&mut self, agent_id: AgentId, task_id: TaskId, result: TaskResult);

    /// Get all registered agents in this topology.
    fn agents(&self) -> Vec<AgentId>;

    /// Add a new agent to the topology.
    fn add_agent(&mut self, agent_id: AgentId) -> Result<()>;

    /// Remove an agent from the topology.
    fn remove_agent(&mut self, agent_id: AgentId) -> Result<()>;
}

// Re-export all topology implementations
mod centralized;
mod hierarchical;
mod peer_to_peer;
mod blackboard;
mod graph_dag;
mod conversational;
mod hybrid;

pub use centralized::{CentralizedConfig, CentralizedTopology};
pub use hierarchical::{HierarchicalConfig, HierarchicalTopology};
pub use peer_to_peer::{PeerToPeerConfig, PeerToPeerTopology};
pub use blackboard::{BlackboardConfig, BlackboardTopology};
pub use graph_dag::{GraphDagConfig, GraphDagTopology};
pub use conversational::{ConversationalConfig, ConversationalTopology, VotingProtocol};
pub use hybrid::{HybridTopology, TaskSelector};

/// Helper function to convert AgentId to a string key for HashMap lookups.
/// This avoids needing to implement PartialEq/Eq/Hash for AgentId.
pub(crate) fn agent_id_to_key(agent_id: &AgentId) -> String {
    match agent_id {
        AgentId::Nydus(id) => format!("nydus:{}", id.0),
        AgentId::Queen(id) => format!("queen:{}", id.0),
        AgentId::Overlord(id) => format!("overlord:{}", id.0),
        AgentId::Overmind(id) => format!("overmind:{}", id.0),
        AgentId::Validator => "validator".to_string(),
        AgentId::Operator => "operator".to_string(),
    }
}
