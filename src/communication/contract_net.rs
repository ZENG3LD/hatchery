//! Contract Net Protocol: call-for-proposals and bidding.

use super::{agent_key, Communication, Message};
use crate::core::types::AgentId;
use anyhow::{anyhow, Result};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

use super::direct::{DirectCommunication, DirectConfig};

/// Configuration for ContractNetCommunication.
#[derive(Debug, Clone)]
pub struct ContractNetConfig {
    /// Timeout for waiting for bids.
    pub bidding_timeout: Duration,
    /// Criteria for selecting winning bid.
    pub selection_criteria: SelectionCriteria,
    /// Direct communication config.
    pub direct_config: DirectConfig,
}

impl Default for ContractNetConfig {
    fn default() -> Self {
        ContractNetConfig {
            bidding_timeout: Duration::from_secs(30),
            selection_criteria: SelectionCriteria::Composite(vec![
                (SelectionCriteria::LowestCost, 0.4),
                (SelectionCriteria::HighestQuality, 0.4),
                (SelectionCriteria::FastestTime, 0.2),
            ]),
            direct_config: DirectConfig::default(),
        }
    }
}

/// Criteria for selecting a winning bid.
#[derive(Debug, Clone)]
pub enum SelectionCriteria {
    /// Select the lowest cost bid.
    LowestCost,
    /// Select the highest quality bid.
    HighestQuality,
    /// Select the fastest estimated time.
    FastestTime,
    /// Weighted composite of multiple criteria (criterion, weight).
    Composite(Vec<(SelectionCriteria, f64)>),
}

/// Call for proposals for a task.
#[derive(Debug, Clone)]
pub struct CallForProposal {
    pub id: String,
    pub task_id: String,
    pub requirements: Vec<String>,
    pub deadline: Instant,
    pub issuer: AgentId,
}

impl CallForProposal {
    /// Create a new CFP.
    pub fn new(task_id: String, requirements: Vec<String>, deadline: Instant, issuer: AgentId) -> Self {
        CallForProposal {
            id: uuid::Uuid::new_v4().to_string(),
            task_id,
            requirements,
            deadline,
            issuer,
        }
    }

    /// Check if the CFP has expired.
    pub fn is_expired(&self) -> bool {
        Instant::now() > self.deadline
    }
}

/// Bid submitted by an agent for a CFP.
#[derive(Debug, Clone)]
pub struct Bid {
    pub bidder: AgentId,
    pub cfp_id: String,
    pub cost: f64,
    pub quality: f64,       // 0.0 to 1.0
    pub estimated_time: Duration,
}

impl Bid {
    /// Calculate score based on selection criteria.
    pub fn score(&self, criteria: &SelectionCriteria) -> f64 {
        match criteria {
            SelectionCriteria::LowestCost => {
                // Lower cost = higher score (inverse)
                if self.cost > 0.0 {
                    1.0 / self.cost
                } else {
                    f64::MAX
                }
            }
            SelectionCriteria::HighestQuality => self.quality,
            SelectionCriteria::FastestTime => {
                // Shorter time = higher score (inverse)
                let secs = self.estimated_time.as_secs_f64();
                if secs > 0.0 {
                    1.0 / secs
                } else {
                    f64::MAX
                }
            }
            SelectionCriteria::Composite(components) => {
                let mut total_score = 0.0;
                let mut total_weight = 0.0;

                for (criterion, weight) in components {
                    total_score += self.score(criterion) * weight;
                    total_weight += weight;
                }

                if total_weight > 0.0 {
                    total_score / total_weight
                } else {
                    0.0
                }
            }
        }
    }
}

/// Contract Net Protocol communication.
///
/// Implements call-for-proposals and competitive bidding for task allocation.
pub struct ContractNetCommunication {
    config: ContractNetConfig,
    /// Underlying direct communication
    direct: DirectCommunication,
    /// Active CFPs indexed by CFP ID
    active_cfps: Arc<RwLock<HashMap<String, CallForProposal>>>,
    /// Collected bids indexed by CFP ID
    bids: Arc<RwLock<HashMap<String, Vec<Bid>>>>,
}

impl ContractNetCommunication {
    /// Create a new ContractNetCommunication instance.
    pub fn new(config: ContractNetConfig) -> Self {
        let direct = DirectCommunication::new(config.direct_config.clone());

        ContractNetCommunication {
            config,
            direct,
            active_cfps: Arc::new(RwLock::new(HashMap::new())),
            bids: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Broadcast a call for proposals.
    pub fn call_for_proposals(
        &self,
        issuer: AgentId,
        task_id: String,
        requirements: Vec<String>,
    ) -> Result<CallForProposal> {
        let deadline = Instant::now() + self.config.bidding_timeout;
        let cfp = CallForProposal::new(task_id, requirements, deadline, issuer.clone());

        // Store CFP
        self.active_cfps.write().insert(cfp.id.clone(), cfp.clone());
        self.bids.write().insert(cfp.id.clone(), Vec::new());

        // Broadcast CFP as a message
        let msg = Message::new(
            issuer.clone(),
            None,
            serde_json::json!({
                "type": "call_for_proposals",
                "cfp": {
                    "id": cfp.id,
                    "task_id": cfp.task_id,
                    "requirements": cfp.requirements,
                }
            }),
        );

        self.direct.broadcast(issuer, msg)?;

        Ok(cfp)
    }

    /// Submit a bid for a CFP.
    pub fn submit_bid(&self, bid: Bid) -> Result<()> {
        // Verify CFP exists and is not expired
        let cfps = self.active_cfps.read();
        let cfp = cfps
            .get(&bid.cfp_id)
            .ok_or_else(|| anyhow!("CFP not found: {}", bid.cfp_id))?;

        if cfp.is_expired() {
            return Err(anyhow!("CFP has expired: {}", bid.cfp_id));
        }

        // Store bid
        drop(cfps); // Release read lock
        self.bids
            .write()
            .entry(bid.cfp_id.clone())
            .or_insert_with(Vec::new)
            .push(bid.clone());

        // Send bid notification to issuer
        let cfp = self.active_cfps.read().get(&bid.cfp_id).cloned().unwrap();
        let issuer = cfp.issuer.clone();
        let msg = Message::new(
            bid.bidder.clone(),
            Some(issuer.clone()),
            serde_json::json!({
                "type": "bid_submitted",
                "cfp_id": bid.cfp_id,
                "cost": bid.cost,
                "quality": bid.quality,
                "estimated_time_secs": bid.estimated_time.as_secs(),
            }),
        );

        self.direct.send(bid.bidder, issuer, msg)?;

        Ok(())
    }

    /// Select the winning bid based on selection criteria.
    pub fn select_winner(&self, cfp_id: &str) -> Result<Option<Bid>> {
        let bids = self.bids.read();
        let bid_list = bids
            .get(cfp_id)
            .ok_or_else(|| anyhow!("CFP not found: {}", cfp_id))?;

        if bid_list.is_empty() {
            return Ok(None);
        }

        // Score all bids
        let mut scored_bids: Vec<(Bid, f64)> = bid_list
            .iter()
            .map(|bid| {
                let score = bid.score(&self.config.selection_criteria);
                (bid.clone(), score)
            })
            .collect();

        // Sort by score (highest first)
        scored_bids.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        // Return winner
        Ok(scored_bids.first().map(|(bid, _)| bid.clone()))
    }

    /// Award the contract to the winning bidder.
    pub fn award_contract(&self, cfp_id: &str) -> Result<Option<AgentId>> {
        let winner = self.select_winner(cfp_id)?;

        if let Some(ref bid) = winner {
            // Notify winner
            let msg = Message::new(
                AgentId::Operator, // Or get issuer from CFP
                Some(bid.bidder.clone()),
                serde_json::json!({
                    "type": "contract_awarded",
                    "cfp_id": cfp_id,
                }),
            );

            self.direct
                .send(AgentId::Operator, bid.bidder.clone(), msg)?;

            // Notify losers
            let all_bids = self.bids.read();
            if let Some(bid_list) = all_bids.get(cfp_id) {
                for losing_bid in bid_list {
                    let loser_key = agent_key(&losing_bid.bidder);
                    let winner_key = agent_key(&bid.bidder);

                    if loser_key != winner_key {
                        let reject_msg = Message::new(
                            AgentId::Operator,
                            Some(losing_bid.bidder.clone()),
                            serde_json::json!({
                                "type": "bid_rejected",
                                "cfp_id": cfp_id,
                            }),
                        );

                        let _ = self.direct.send(
                            AgentId::Operator,
                            losing_bid.bidder.clone(),
                            reject_msg,
                        );
                    }
                }
            }

            Ok(Some(bid.bidder.clone()))
        } else {
            Ok(None)
        }
    }

    /// Get all bids for a CFP.
    pub fn get_bids(&self, cfp_id: &str) -> Vec<Bid> {
        self.bids
            .read()
            .get(cfp_id)
            .cloned()
            .unwrap_or_default()
    }

    /// Get all active CFPs.
    pub fn active_cfps(&self) -> Vec<CallForProposal> {
        self.active_cfps.read().values().cloned().collect()
    }

    /// Close a CFP.
    pub fn close_cfp(&self, cfp_id: &str) {
        self.active_cfps.write().remove(cfp_id);
        self.bids.write().remove(cfp_id);
    }

    /// Get statistics about contract net activity.
    pub fn stats(&self) -> ContractNetStats {
        let cfps = self.active_cfps.read();
        let bids = self.bids.read();

        let total_cfps = cfps.len();
        let total_bids: usize = bids.values().map(|b| b.len()).sum();
        let avg_bids_per_cfp = if total_cfps > 0 {
            total_bids as f64 / total_cfps as f64
        } else {
            0.0
        };

        ContractNetStats {
            total_active_cfps: total_cfps,
            total_bids,
            avg_bids_per_cfp,
        }
    }
}

/// Contract Net statistics.
#[derive(Debug, Clone)]
pub struct ContractNetStats {
    pub total_active_cfps: usize,
    pub total_bids: usize,
    pub avg_bids_per_cfp: f64,
}

impl Communication for ContractNetCommunication {
    fn send(&self, from: AgentId, to: AgentId, message: Message) -> Result<()> {
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

    #[test]
    fn test_bid_scoring_lowest_cost() {
        let bid = Bid {
            bidder: AgentId::Queen(QueenId("Q1".to_string())),
            cfp_id: "cfp-1".to_string(),
            cost: 10.0,
            quality: 0.8,
            estimated_time: Duration::from_secs(100),
        };

        let score = bid.score(&SelectionCriteria::LowestCost);
        assert_eq!(score, 0.1); // 1 / 10
    }

    #[test]
    fn test_bid_scoring_composite() {
        let bid = Bid {
            bidder: AgentId::Queen(QueenId("Q1".to_string())),
            cfp_id: "cfp-1".to_string(),
            cost: 10.0,
            quality: 0.8,
            estimated_time: Duration::from_secs(100),
        };

        let criteria = SelectionCriteria::Composite(vec![
            (SelectionCriteria::LowestCost, 0.5),
            (SelectionCriteria::HighestQuality, 0.5),
        ]);

        let score = bid.score(&criteria);
        // (0.1 * 0.5 + 0.8 * 0.5) / 1.0 = 0.45
        assert!((score - 0.45).abs() < 0.001);
    }

    #[tokio::test]
    async fn test_contract_net_workflow() {
        let comm = ContractNetCommunication::new(ContractNetConfig::default());

        let issuer = AgentId::Queen(QueenId("Q-issuer".to_string()));
        let bidder1 = AgentId::Queen(QueenId("Q1".to_string()));
        let bidder2 = AgentId::Queen(QueenId("Q2".to_string()));

        // Create CFP
        let cfp = comm
            .call_for_proposals(
                issuer,
                "task-123".to_string(),
                vec!["skill-A".to_string(), "skill-B".to_string()],
            )
            .unwrap();

        // Submit bids
        let bid1 = Bid {
            bidder: bidder1,
            cfp_id: cfp.id.clone(),
            cost: 100.0,
            quality: 0.9,
            estimated_time: Duration::from_secs(3600),
        };

        let bid2 = Bid {
            bidder: bidder2,
            cfp_id: cfp.id.clone(),
            cost: 80.0,
            quality: 0.7,
            estimated_time: Duration::from_secs(1800),
        };

        comm.submit_bid(bid1).unwrap();
        comm.submit_bid(bid2).unwrap();

        // Select winner
        let winner = comm.select_winner(&cfp.id).unwrap();
        assert!(winner.is_some());

        let stats = comm.stats();
        assert_eq!(stats.total_active_cfps, 1);
        assert_eq!(stats.total_bids, 2);
    }
}
