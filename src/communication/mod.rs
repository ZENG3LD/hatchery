//! Communication layer for Hatchery swarm orchestration.
//!
//! This module provides various communication patterns for agent coordination:
//! - Direct: Point-to-point messaging via channels
//! - Broadcast: Topic-based pub/sub
//! - Blackboard: Shared knowledge space
//! - MessageBus: Centralized routing with audit logging
//! - Handoff: Task ownership transfer tracking
//! - ContractNet: Call-for-proposals bidding protocol
//! - RippleEffect: Signal propagation through agent network
//! - Protocols: MCP, A2A, ACP, ANP protocol adapters

use crate::core::types::AgentId;
use anyhow::Result;
use tokio::sync::mpsc;

/// Message exchanged between agents.
#[derive(Debug, Clone)]
pub struct Message {
    pub id: String,
    pub from: AgentId,
    pub to: Option<AgentId>, // None for broadcast
    pub topic: Option<String>,
    pub payload: serde_json::Value,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

impl Message {
    /// Create a new message.
    pub fn new(from: AgentId, to: Option<AgentId>, payload: serde_json::Value) -> Self {
        Message {
            id: uuid::Uuid::new_v4().to_string(),
            from,
            to,
            topic: None,
            payload,
            timestamp: chrono::Utc::now(),
        }
    }

    /// Create a message with a topic.
    pub fn with_topic(
        from: AgentId,
        topic: String,
        payload: serde_json::Value,
    ) -> Self {
        Message {
            id: uuid::Uuid::new_v4().to_string(),
            from,
            to: None,
            topic: Some(topic),
            payload,
            timestamp: chrono::Utc::now(),
        }
    }
}

/// Communication defines how agents exchange messages.
pub trait Communication: Send + Sync {
    /// Send a message from one agent to another.
    fn send(&self, from: AgentId, to: AgentId, message: Message) -> Result<()>;

    /// Broadcast a message from an agent to all other agents.
    fn broadcast(&self, from: AgentId, message: Message) -> Result<()>;

    /// Subscribe an agent to a specific topic.
    fn subscribe(&self, agent_id: AgentId, topic: &str) -> Result<mpsc::Receiver<Message>>;

    /// Publish a message to a topic.
    fn publish(&self, topic: &str, message: Message) -> Result<()>;

    /// Get a receiver for messages addressed to this agent.
    fn receiver(&self, agent_id: AgentId) -> Result<mpsc::Receiver<Message>>;
}

/// Convert AgentId to string key for HashMap indexing.
pub fn agent_key(agent_id: &AgentId) -> String {
    match agent_id {
        AgentId::Nydus(id) => format!("nydus:{}", id.0),
        AgentId::Queen(id) => format!("queen:{}", id.0),
        AgentId::Overlord(id) => format!("overlord:{}", id),
        AgentId::Overmind(id) => format!("overmind:{}", id),
        AgentId::Validator => "validator".to_string(),
        AgentId::Operator => "operator".to_string(),
    }
}

// Re-export all implementations
pub mod blackboard;
pub mod broadcast;
pub mod contract_net;
pub mod direct;
pub mod handoff;
pub mod message_bus;
pub mod protocols;
pub mod ripple_effect;

pub use blackboard::{BlackboardCommunication, BlackboardCommunicationConfig, BlackboardEntry};
pub use broadcast::{BroadcastCommunication, BroadcastConfig};
pub use contract_net::{
    Bid, CallForProposal, ContractNetCommunication, ContractNetConfig, SelectionCriteria,
};
pub use direct::{DirectCommunication, DirectConfig};
pub use handoff::{HandoffChain, HandoffCommunication, HandoffConfig, HandoffTransfer};
pub use message_bus::{AuditEntry, MessageBusCommunication, MessageBusConfig};
pub use ripple_effect::{
    PropagationGraph, RippleEffectCommunication, RippleEffectConfig, Signal, SignalType,
};
