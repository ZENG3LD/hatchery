//! Voting-based consensus for critical decisions.

use super::{Resilience, ResilienceAction, HealthStatus, RecoveryPlan, RecoveryAction};
use crate::core::types::{TaskId, AgentId};
use anyhow::{Result, anyhow};
use std::collections::HashMap;
use std::time::{Duration, Instant};

// ============================================================================
// Configuration
// ============================================================================

/// Voting protocol for consensus.
#[derive(Debug, Clone)]
pub enum VotingProtocol {
    /// Simple majority (>50% approval)
    Majority,
    /// Approval voting (all votes count, highest approval wins)
    Approval,
    /// Requires 100% agreement
    Unanimity,
    /// Configurable threshold (e.g., 0.66 for 2/3 majority)
    SuperMajority(f64),
}

/// Configuration for consensus behavior.
#[derive(Debug, Clone)]
pub struct ConsensusConfig {
    /// Voting protocol to use
    pub voting_protocol: VotingProtocol,
    /// Minimum number of votes required
    pub min_votes: usize,
    /// Timeout for voting rounds
    pub timeout: Duration,
}

impl Default for ConsensusConfig {
    fn default() -> Self {
        ConsensusConfig {
            voting_protocol: VotingProtocol::Majority,
            min_votes: 2,
            timeout: Duration::from_secs(60),
        }
    }
}

// ============================================================================
// Vote Types
// ============================================================================

/// A vote in a consensus round.
#[derive(Debug, Clone)]
pub struct Vote {
    /// Identity of the voter
    pub voter: String,
    /// Whether the voter approves
    pub approve: bool,
    /// Confidence level (0.0-1.0)
    pub confidence: f64,
    /// Reasoning for the vote
    pub reasoning: String,
}

/// A consensus voting round.
#[derive(Debug, Clone)]
struct ConsensusRound {
    id: String,
    topic: String,
    votes: Vec<Vote>,
    deadline: Instant,
    resolved: bool,
    resolution: Option<bool>,
}

impl ConsensusRound {
    fn new(id: String, topic: String, deadline: Instant) -> Self {
        ConsensusRound {
            id,
            topic,
            votes: Vec::new(),
            deadline,
            resolved: false,
            resolution: None,
        }
    }

    fn is_expired(&self) -> bool {
        Instant::now() > self.deadline
    }
}

// ============================================================================
// Implementation
// ============================================================================

/// Consensus resilience handler with voting protocols.
pub struct ConsensusResilience {
    config: ConsensusConfig,
    active_rounds: HashMap<String, ConsensusRound>,
    round_counter: usize,
}

impl ConsensusResilience {
    /// Create a new consensus resilience handler with the given configuration.
    pub fn new(config: ConsensusConfig) -> Self {
        ConsensusResilience {
            config,
            active_rounds: HashMap::new(),
            round_counter: 0,
        }
    }

    /// Create with default configuration.
    pub fn default() -> Self {
        Self::new(ConsensusConfig::default())
    }

    /// Start a new voting round on a topic.
    pub fn start_round(&mut self, topic: String) -> String {
        self.round_counter += 1;
        let round_id = format!("round_{}", self.round_counter);

        let deadline = Instant::now() + self.config.timeout;
        let round = ConsensusRound::new(round_id.clone(), topic, deadline);

        self.active_rounds.insert(round_id.clone(), round);
        round_id
    }

    /// Cast a vote in an active round.
    pub fn cast_vote(&mut self, round_id: &str, vote: Vote) -> Result<()> {
        let round = self.active_rounds
            .get_mut(round_id)
            .ok_or_else(|| anyhow!("Round not found: {}", round_id))?;

        if round.resolved {
            return Err(anyhow!("Round already resolved"));
        }

        if round.is_expired() {
            return Err(anyhow!("Round expired"));
        }

        round.votes.push(vote);
        Ok(())
    }

    /// Resolve a voting round based on the configured protocol.
    pub fn resolve_round(&mut self, round_id: &str) -> Result<bool> {
        let round = self.active_rounds
            .get_mut(round_id)
            .ok_or_else(|| anyhow!("Round not found: {}", round_id))?;

        if round.resolved {
            return round.resolution.ok_or_else(|| anyhow!("Round resolved but no resolution recorded"));
        }

        if round.votes.len() < self.config.min_votes {
            return Err(anyhow!(
                "Not enough votes: {} < {}",
                round.votes.len(),
                self.config.min_votes
            ));
        }

        let approved = match &self.config.voting_protocol {
            VotingProtocol::Majority => {
                let approve_count = round.votes.iter().filter(|v| v.approve).count();
                let total = round.votes.len();
                approve_count as f64 > total as f64 / 2.0
            }
            VotingProtocol::Approval => {
                // Weighted by confidence
                let approve_score: f64 = round.votes.iter()
                    .filter(|v| v.approve)
                    .map(|v| v.confidence)
                    .sum();
                let reject_score: f64 = round.votes.iter()
                    .filter(|v| !v.approve)
                    .map(|v| v.confidence)
                    .sum();
                approve_score > reject_score
            }
            VotingProtocol::Unanimity => {
                round.votes.iter().all(|v| v.approve)
            }
            VotingProtocol::SuperMajority(threshold) => {
                let approve_count = round.votes.iter().filter(|v| v.approve).count();
                let total = round.votes.len();
                (approve_count as f64 / total as f64) >= *threshold
            }
        };

        round.resolved = true;
        round.resolution = Some(approved);

        Ok(approved)
    }

    /// Get the status of a voting round.
    pub fn get_round_status(&self, round_id: &str) -> Option<(bool, usize, bool)> {
        self.active_rounds.get(round_id).map(|round| {
            (round.resolved, round.votes.len(), round.is_expired())
        })
    }

    /// Get all active rounds.
    pub fn get_active_rounds(&self) -> Vec<String> {
        self.active_rounds
            .iter()
            .filter(|(_, round)| !round.resolved)
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Clean up expired and resolved rounds.
    pub fn cleanup_rounds(&mut self) {
        self.active_rounds.retain(|_, round| {
            !round.resolved && !round.is_expired()
        });
    }

    /// Get vote details for a round.
    pub fn get_votes(&self, round_id: &str) -> Option<Vec<Vote>> {
        self.active_rounds.get(round_id).map(|round| round.votes.clone())
    }
}

impl Resilience for ConsensusResilience {
    fn handle_failure(&mut self, task_id: TaskId, _agent_id: AgentId, error: String) -> Result<ResilienceAction> {
        // Start a consensus round to decide on retry vs abandon
        let topic = format!(
            "Should retry task {} after error: {}",
            task_id.0, error
        );

        let _round_id = self.start_round(topic);

        // For now, return a retry action with a delay to allow voting
        // In a real implementation, this would wait for votes before deciding
        // This is a simplified version that assumes votes will be cast externally

        Ok(ResilienceAction::Retry {
            delay: self.config.timeout,
        })
    }

    fn check_health(&self, _agent_id: AgentId) -> Result<HealthStatus> {
        let active_count = self.active_rounds
            .values()
            .filter(|r| !r.resolved)
            .count();

        let status = if active_count == 0 {
            HealthStatus::Healthy
        } else if active_count < 5 {
            HealthStatus::Degraded
        } else {
            HealthStatus::Critical
        };

        Ok(status)
    }

    fn plan_recovery(&mut self, agent_id: AgentId) -> Result<RecoveryPlan> {
        // Start a consensus round for recovery decision
        let topic = format!("Should recover agent: {:?}", agent_id);
        let _round_id = self.start_round(topic);

        // Default to restart while waiting for consensus
        Ok(RecoveryPlan {
            agent_id: agent_id.clone(),
            action: RecoveryAction::Restart,
        })
    }

    fn record_failure(&mut self, _task_id: TaskId) -> Result<()> {
        // Consensus doesn't track individual failures
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_majority_voting() {
        let mut consensus = ConsensusResilience::new(ConsensusConfig {
            voting_protocol: VotingProtocol::Majority,
            min_votes: 3,
            timeout: Duration::from_secs(60),
        });

        let round_id = consensus.start_round("Test decision".to_string());

        // Cast votes: 2 approve, 1 reject
        consensus
            .cast_vote(&round_id, Vote {
                voter: "agent1".to_string(),
                approve: true,
                confidence: 1.0,
                reasoning: "Looks good".to_string(),
            })
            .unwrap();

        consensus
            .cast_vote(&round_id, Vote {
                voter: "agent2".to_string(),
                approve: true,
                confidence: 1.0,
                reasoning: "Agree".to_string(),
            })
            .unwrap();

        consensus
            .cast_vote(&round_id, Vote {
                voter: "agent3".to_string(),
                approve: false,
                confidence: 1.0,
                reasoning: "Disagree".to_string(),
            })
            .unwrap();

        // Resolve - should approve (2/3 > 50%)
        let result = consensus.resolve_round(&round_id).unwrap();
        assert!(result);
    }

    #[test]
    fn test_unanimity() {
        let mut consensus = ConsensusResilience::new(ConsensusConfig {
            voting_protocol: VotingProtocol::Unanimity,
            min_votes: 2,
            timeout: Duration::from_secs(60),
        });

        let round_id = consensus.start_round("Critical decision".to_string());

        // Cast votes: 1 approve, 1 reject
        consensus
            .cast_vote(&round_id, Vote {
                voter: "agent1".to_string(),
                approve: true,
                confidence: 1.0,
                reasoning: "Yes".to_string(),
            })
            .unwrap();

        consensus
            .cast_vote(&round_id, Vote {
                voter: "agent2".to_string(),
                approve: false,
                confidence: 1.0,
                reasoning: "No".to_string(),
            })
            .unwrap();

        // Should not approve (not unanimous)
        let result = consensus.resolve_round(&round_id).unwrap();
        assert!(!result);
    }

    #[test]
    fn test_super_majority() {
        let mut consensus = ConsensusResilience::new(ConsensusConfig {
            voting_protocol: VotingProtocol::SuperMajority(0.66),
            min_votes: 3,
            timeout: Duration::from_secs(60),
        });

        let round_id = consensus.start_round("Important decision".to_string());

        // Cast votes: 2 approve, 1 reject (66.7% approval)
        for i in 0..3 {
            consensus
                .cast_vote(&round_id, Vote {
                    voter: format!("agent{}", i),
                    approve: i < 2,
                    confidence: 1.0,
                    reasoning: "Vote".to_string(),
                })
                .unwrap();
        }

        // Should approve (2/3 >= 0.66)
        let result = consensus.resolve_round(&round_id).unwrap();
        assert!(result);
    }
}
