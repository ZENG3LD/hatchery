//! Task ownership transfer tracking (handoff protocol).

use super::{Communication, Message};
use crate::core::types::AgentId;
use anyhow::Result;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc;

use super::direct::{DirectCommunication, DirectConfig};

/// Configuration for HandoffCommunication.
#[derive(Debug, Clone)]
pub struct HandoffConfig {
    /// Track full handoff chains for audit.
    pub track_chains: bool,
    /// Direct communication config.
    pub direct_config: DirectConfig,
}

impl Default for HandoffConfig {
    fn default() -> Self {
        HandoffConfig {
            track_chains: true,
            direct_config: DirectConfig::default(),
        }
    }
}

/// A single transfer in a handoff chain.
#[derive(Debug, Clone)]
pub struct HandoffTransfer {
    pub from: AgentId,
    pub to: AgentId,
    pub timestamp: Instant,
    pub reason: String,
}

/// Complete handoff chain for a task.
#[derive(Debug, Clone)]
pub struct HandoffChain {
    pub task_id: String,
    pub transfers: Vec<HandoffTransfer>,
}

impl HandoffChain {
    /// Create a new handoff chain.
    pub fn new(task_id: String) -> Self {
        HandoffChain {
            task_id,
            transfers: Vec::new(),
        }
    }

    /// Add a transfer to the chain.
    pub fn add_transfer(&mut self, transfer: HandoffTransfer) {
        self.transfers.push(transfer);
    }

    /// Get the current owner (last in chain).
    pub fn current_owner(&self) -> Option<&AgentId> {
        self.transfers.last().map(|t| &t.to)
    }

    /// Get the original owner (first in chain).
    pub fn original_owner(&self) -> Option<&AgentId> {
        self.transfers.first().map(|t| &t.from)
    }

    /// Get the number of transfers.
    pub fn transfer_count(&self) -> usize {
        self.transfers.len()
    }
}

/// Bidirectional agent-to-agent task ownership transfer.
///
/// Wraps DirectCommunication and tracks handoff chains for tasks.
pub struct HandoffCommunication {
    config: HandoffConfig,
    /// Underlying direct communication
    direct: DirectCommunication,
    /// Handoff chains indexed by task_id
    chains: Arc<RwLock<HashMap<String, HandoffChain>>>,
}

impl HandoffCommunication {
    /// Create a new HandoffCommunication instance.
    pub fn new(config: HandoffConfig) -> Self {
        let direct = DirectCommunication::new(config.direct_config.clone());

        HandoffCommunication {
            config,
            direct,
            chains: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Record a handoff transfer.
    fn record_handoff(&self, from: AgentId, to: AgentId, task_id: String, reason: String) {
        if !self.config.track_chains {
            return;
        }

        let transfer = HandoffTransfer {
            from: from.clone(),
            to: to.clone(),
            timestamp: Instant::now(),
            reason,
        };

        let mut chains = self.chains.write();
        let chain = chains
            .entry(task_id.clone())
            .or_insert_with(|| HandoffChain::new(task_id));

        chain.add_transfer(transfer);
    }

    /// Check if a message indicates a handoff.
    fn is_handoff_message(&self, message: &Message) -> Option<(String, String)> {
        // Look for handoff indicators in payload
        if let Some(obj) = message.payload.as_object() {
            if let (Some(task_id), Some(reason)) = (
                obj.get("handoff_task_id").and_then(|v| v.as_str()),
                obj.get("handoff_reason").and_then(|v| v.as_str()),
            ) {
                return Some((task_id.to_string(), reason.to_string()));
            }
        }
        None
    }

    /// Get the handoff chain for a task.
    pub fn handoff_history(&self, task_id: &str) -> Option<HandoffChain> {
        self.chains.read().get(task_id).cloned()
    }

    /// Get the current owner of a task.
    pub fn current_owner(&self, task_id: &str) -> Option<AgentId> {
        self.chains
            .read()
            .get(task_id)
            .and_then(|chain| chain.current_owner().cloned())
    }

    /// Get all tasks currently owned by an agent.
    pub fn tasks_owned_by(&self, agent_id: &AgentId) -> Vec<String> {
        self.chains
            .read()
            .iter()
            .filter_map(|(task_id, chain)| {
                chain.current_owner().and_then(|owner| {
                    // Compare by serializing both to string (since AgentId doesn't derive Eq)
                    let owner_str = format!("{:?}", owner);
                    let agent_str = format!("{:?}", agent_id);
                    if owner_str == agent_str {
                        Some(task_id.clone())
                    } else {
                        None
                    }
                })
            })
            .collect()
    }

    /// Get all active handoff chains.
    pub fn all_chains(&self) -> Vec<HandoffChain> {
        self.chains.read().values().cloned().collect()
    }

    /// Clear handoff history for a task.
    pub fn clear_task_history(&self, task_id: &str) {
        self.chains.write().remove(task_id);
    }

    /// Get statistics about handoffs.
    pub fn handoff_stats(&self) -> HandoffStats {
        let chains = self.chains.read();

        let total_tasks = chains.len();
        let total_transfers = chains.values().map(|c| c.transfer_count()).sum();
        let avg_transfers = if total_tasks > 0 {
            total_transfers as f64 / total_tasks as f64
        } else {
            0.0
        };

        let max_transfers = chains
            .values()
            .map(|c| c.transfer_count())
            .max()
            .unwrap_or(0);

        HandoffStats {
            total_tasks,
            total_transfers,
            avg_transfers_per_task: avg_transfers,
            max_transfers: max_transfers,
        }
    }
}

/// Handoff statistics.
#[derive(Debug, Clone)]
pub struct HandoffStats {
    pub total_tasks: usize,
    pub total_transfers: usize,
    pub avg_transfers_per_task: f64,
    pub max_transfers: usize,
}

impl Communication for HandoffCommunication {
    fn send(&self, from: AgentId, to: AgentId, message: Message) -> Result<()> {
        // Check if this is a handoff message
        if let Some((task_id, reason)) = self.is_handoff_message(&message) {
            self.record_handoff(from.clone(), to.clone(), task_id, reason);
        }

        // Forward via direct communication
        self.direct.send(from, to, message)
    }

    fn broadcast(&self, from: AgentId, message: Message) -> Result<()> {
        self.direct.broadcast(from, message)
    }

    fn subscribe(&self, agent_id: AgentId, topic: &str) -> Result<mpsc::Receiver<Message>> {
        self.direct.subscribe(agent_id, topic)
    }

    fn publish(&self, topic: &str, message: Message) -> Result<()> {
        self.direct.publish(topic, message)
    }

    fn receiver(&self, agent_id: AgentId) -> Result<mpsc::Receiver<Message>> {
        self.direct.receiver(agent_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::types::QueenId;

    #[tokio::test]
    async fn test_handoff_tracking() {
        let comm = HandoffCommunication::new(HandoffConfig::default());

        let queen1 = AgentId::Queen(QueenId("Q1".to_string()));
        let queen2 = AgentId::Queen(QueenId("Q2".to_string()));
        let queen3 = AgentId::Queen(QueenId("Q3".to_string()));

        let _rx2 = comm.receiver(queen2.clone()).unwrap();
        let _rx3 = comm.receiver(queen3.clone()).unwrap();

        // First handoff: Q1 -> Q2
        let msg1 = Message::new(
            queen1.clone(),
            Some(queen2.clone()),
            serde_json::json!({
                "handoff_task_id": "task-123",
                "handoff_reason": "blocked on dependency"
            }),
        );
        comm.send(queen1.clone(), queen2.clone(), msg1).unwrap();

        // Second handoff: Q2 -> Q3
        let msg2 = Message::new(
            queen2.clone(),
            Some(queen3.clone()),
            serde_json::json!({
                "handoff_task_id": "task-123",
                "handoff_reason": "specialized expertise needed"
            }),
        );
        comm.send(queen2, queen3.clone(), msg2).unwrap();

        // Check chain
        let chain = comm.handoff_history("task-123").unwrap();
        assert_eq!(chain.transfer_count(), 2);

        let owner = comm.current_owner("task-123").unwrap();
        let owner_str = format!("{:?}", owner);
        let q3_str = format!("{:?}", queen3);
        assert_eq!(owner_str, q3_str);
    }

    #[test]
    fn test_handoff_stats() {
        let comm = HandoffCommunication::new(HandoffConfig::default());

        let queen1 = AgentId::Queen(QueenId("Q1".to_string()));
        let queen2 = AgentId::Queen(QueenId("Q2".to_string()));

        let _rx = comm.receiver(queen2.clone()).unwrap();

        for i in 0..5 {
            let msg = Message::new(
                queen1.clone(),
                Some(queen2.clone()),
                serde_json::json!({
                    "handoff_task_id": format!("task-{}", i),
                    "handoff_reason": "test"
                }),
            );
            let _ = comm.send(queen1.clone(), queen2.clone(), msg);
        }

        let stats = comm.handoff_stats();
        assert_eq!(stats.total_tasks, 5);
        assert_eq!(stats.total_transfers, 5);
        assert_eq!(stats.avg_transfers_per_task, 1.0);
    }
}
