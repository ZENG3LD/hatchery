//! Conversational topology — multi-round debate among agents before task assignment.

use super::{agent_id_to_key, Topology};
use crate::core::types::{AgentId, TaskId, TaskResult};
use anyhow::{anyhow, Result};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;

/// Voting protocol for conversational consensus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VotingProtocol {
    /// Simple majority (>50%)
    Majority,
    /// Approval voting (agent with most approvals wins)
    Approval,
    /// Requires unanimous agreement
    Unanimity,
}

/// Configuration for conversational topology.
#[derive(Debug, Clone)]
pub struct ConversationalConfig {
    /// Maximum number of debate rounds
    pub max_turns: usize,
    /// Voting protocol to use
    pub voting_protocol: VotingProtocol,
}

impl Default for ConversationalConfig {
    fn default() -> Self {
        ConversationalConfig {
            max_turns: 3,
            voting_protocol: VotingProtocol::Majority,
        }
    }
}

/// Proposal made by an agent during a debate round.
#[derive(Debug, Clone)]
pub struct Proposal {
    pub agent_id: String,
    pub proposed_assignee: String,
    pub reasoning: String,
}

/// Vote cast by an agent.
#[derive(Debug, Clone)]
pub struct Vote {
    pub voter: String,
    pub target: String,
    pub approve: bool,
}

/// A single debate round.
#[derive(Debug, Clone)]
pub struct DebateRound {
    pub round_number: usize,
    pub proposals: Vec<Proposal>,
    pub votes: Vec<Vote>,
}

impl DebateRound {
    fn new(round_number: usize) -> Self {
        DebateRound {
            round_number,
            proposals: Vec::new(),
            votes: Vec::new(),
        }
    }

    /// Add a proposal to this round.
    fn add_proposal(&mut self, proposal: Proposal) {
        self.proposals.push(proposal);
    }

    /// Add a vote to this round.
    fn add_vote(&mut self, vote: Vote) {
        self.votes.push(vote);
    }
}

/// Vote tallying engine.
struct VoteTallier {
    protocol: VotingProtocol,
}

impl VoteTallier {
    fn new(protocol: VotingProtocol) -> Self {
        VoteTallier { protocol }
    }

    /// Tally votes and determine winner based on the voting protocol.
    fn tally(&self, votes: &[Vote], total_agents: usize) -> Option<String> {
        if votes.is_empty() || total_agents == 0 {
            return None;
        }

        match self.protocol {
            VotingProtocol::Majority => self.tally_majority(votes, total_agents),
            VotingProtocol::Approval => self.tally_approval(votes),
            VotingProtocol::Unanimity => self.tally_unanimity(votes, total_agents),
        }
    }

    /// Simple majority: candidate with >50% votes wins.
    fn tally_majority(&self, votes: &[Vote], total_agents: usize) -> Option<String> {
        let mut vote_counts: HashMap<String, usize> = HashMap::new();

        for vote in votes {
            if vote.approve {
                *vote_counts.entry(vote.target.clone()).or_insert(0) += 1;
            }
        }

        // For simple majority, we need more than half (>50%)
        // With 3 agents, we need at least 2 votes (2/3 = 66% > 50%)
        // With 4 agents, we need at least 3 votes (3/4 = 75% > 50%)
        let threshold = total_agents / 2;

        for (candidate, count) in vote_counts {
            if count > threshold {
                return Some(candidate);
            }
        }

        None
    }

    /// Approval voting: candidate with most approvals wins.
    fn tally_approval(&self, votes: &[Vote]) -> Option<String> {
        let mut approval_counts: HashMap<String, usize> = HashMap::new();

        for vote in votes {
            if vote.approve {
                *approval_counts.entry(vote.target.clone()).or_insert(0) += 1;
            }
        }

        approval_counts
            .into_iter()
            .max_by_key(|(_, count)| *count)
            .map(|(candidate, _)| candidate)
    }

    /// Unanimity: all agents must agree on the same candidate.
    fn tally_unanimity(&self, votes: &[Vote], total_agents: usize) -> Option<String> {
        let mut vote_counts: HashMap<String, usize> = HashMap::new();

        for vote in votes {
            if vote.approve {
                *vote_counts.entry(vote.target.clone()).or_insert(0) += 1;
            }
        }

        for (candidate, count) in vote_counts {
            if count == total_agents {
                return Some(candidate);
            }
        }

        None
    }
}

/// Debate facilitator that manages multi-round discussions.
struct DebateFacilitator {
    max_turns: usize,
    tallier: VoteTallier,
}

impl DebateFacilitator {
    fn new(max_turns: usize, protocol: VotingProtocol) -> Self {
        DebateFacilitator {
            max_turns,
            tallier: VoteTallier::new(protocol),
        }
    }

    /// Run a multi-round debate and return the winner.
    fn facilitate(
        &self,
        task_id: &TaskId,
        agents: &HashMap<String, AgentInfo>,
    ) -> Result<String> {
        let mut rounds = Vec::new();

        for round_num in 0..self.max_turns {
            let mut round = DebateRound::new(round_num);

            // Phase 1: Proposals
            // Each agent proposes the least-loaded agent as assignee
            let agent_list: Vec<_> = agents.iter().collect();
            for (proposer_key, _) in &agent_list {
                let proposed = self.select_least_loaded(agents);
                let reasoning = format!(
                    "Agent {} has lowest load ({} tasks)",
                    proposed,
                    agents.get(&proposed).map(|a| a.active_tasks).unwrap_or(0)
                );

                round.add_proposal(Proposal {
                    agent_id: proposer_key.to_string(),
                    proposed_assignee: proposed,
                    reasoning,
                });
            }

            // Phase 2: Voting
            // Each agent votes for the most commonly proposed agent
            let proposal_counts = self.count_proposals(&round.proposals);
            let most_proposed = proposal_counts
                .into_iter()
                .max_by_key(|(_, count)| *count)
                .map(|(agent, _)| agent);

            if let Some(target) = most_proposed {
                for (voter_key, _) in &agent_list {
                    round.add_vote(Vote {
                        voter: voter_key.to_string(),
                        target: target.clone(),
                        approve: true,
                    });
                }
            }

            rounds.push(round.clone());

            // Check for consensus
            if let Some(winner) = self.tallier.tally(&round.votes, agents.len()) {
                eprintln!(
                    "[ConversationalTopology] Consensus reached on round {} for task {}: {}",
                    round_num, task_id.0, winner
                );
                return Ok(winner);
            }
        }

        // No consensus after max rounds, use fallback (least loaded)
        eprintln!(
            "[ConversationalTopology] No consensus after {} rounds, using fallback",
            self.max_turns
        );
        Ok(self.select_least_loaded(agents))
    }

    /// Count how many times each agent was proposed.
    fn count_proposals(&self, proposals: &[Proposal]) -> HashMap<String, usize> {
        let mut counts = HashMap::new();
        for proposal in proposals {
            *counts
                .entry(proposal.proposed_assignee.clone())
                .or_insert(0) += 1;
        }
        counts
    }

    /// Select the least-loaded agent as fallback.
    fn select_least_loaded(&self, agents: &HashMap<String, AgentInfo>) -> String {
        agents
            .iter()
            .min_by_key(|(_, info)| info.active_tasks)
            .map(|(key, _)| key.clone())
            .unwrap_or_default()
    }
}

/// Agent information for conversational topology.
#[derive(Debug, Clone)]
struct AgentInfo {
    id: AgentId,
    active_tasks: usize,
    completed_tasks: usize,
}

/// Conversational topology implementation.
///
/// Agents engage in multi-round debates before deciding who should handle a task.
/// This models deliberative decision-making and can lead to better task assignments
/// when agents have different perspectives on workload and capabilities.
pub struct ConversationalTopology {
    config: ConversationalConfig,
    /// Debate facilitator
    facilitator: DebateFacilitator,
    /// Agent registry
    agents: Arc<RwLock<HashMap<String, AgentInfo>>>,
    /// Debate history (task_id -> rounds)
    debate_history: Arc<RwLock<HashMap<String, Vec<DebateRound>>>>,
    /// Task assignments
    task_assignments: Arc<RwLock<HashMap<String, String>>>,
    /// Completed results
    results: Arc<RwLock<HashMap<String, TaskResult>>>,
}

impl ConversationalTopology {
    /// Create a new conversational topology with the given configuration.
    pub fn new(config: ConversationalConfig) -> Self {
        let facilitator = DebateFacilitator::new(config.max_turns, config.voting_protocol);

        ConversationalTopology {
            config,
            facilitator,
            agents: Arc::new(RwLock::new(HashMap::new())),
            debate_history: Arc::new(RwLock::new(HashMap::new())),
            task_assignments: Arc::new(RwLock::new(HashMap::new())),
            results: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Get debate history for a task.
    pub fn get_debate_history(&self, task_id: &TaskId) -> Option<Vec<DebateRound>> {
        self.debate_history.read().get(&task_id.0).cloned()
    }

    /// Get statistics about the topology.
    pub fn stats(&self) -> ConversationalStats {
        let agents = self.agents.read();
        let total_active = agents.values().map(|a| a.active_tasks).sum();
        let total_completed = agents.values().map(|a| a.completed_tasks).sum();
        let total_debates = self.debate_history.read().len();

        ConversationalStats {
            total_agents: agents.len(),
            total_active_tasks: total_active,
            total_completed_tasks: total_completed,
            total_debates,
        }
    }
}

impl Topology for ConversationalTopology {
    fn assign_task(&mut self, task_id: &TaskId) -> Result<Vec<AgentId>> {
        eprintln!(
            "[ConversationalTopology] Starting debate for task {}",
            task_id.0
        );

        // Run debate
        let winner_key = {
            let agents = self.agents.read();
            self.facilitator.facilitate(task_id, &agents)?
        };

        // Get winner agent ID
        let winner_id = {
            let agents = self.agents.read();
            agents
                .get(&winner_key)
                .map(|info| info.id.clone())
                .ok_or_else(|| anyhow!("Winner not found in agent registry: {}", winner_key))?
        };

        // Update agent info
        {
            let mut agents = self.agents.write();
            if let Some(info) = agents.get_mut(&winner_key) {
                info.active_tasks += 1;
            }
        }

        // Record assignment
        let task_key = task_id.0.clone();
        self.task_assignments
            .write()
            .insert(task_key, winner_key.clone());

        eprintln!(
            "[ConversationalTopology] Assigned task {} to agent {} after debate",
            task_id.0, winner_key
        );

        Ok(vec![winner_id])
    }

    fn handle_completion(&mut self, agent_id: AgentId, task_id: TaskId, result: TaskResult) {
        let agent_key = agent_id_to_key(&agent_id);
        eprintln!(
            "[ConversationalTopology] Agent {} completed task {} with status {:?}",
            agent_key, task_id.0, result.status
        );

        // Update agent info
        {
            let mut agents = self.agents.write();
            if let Some(info) = agents.get_mut(&agent_key) {
                info.active_tasks = info.active_tasks.saturating_sub(1);
                info.completed_tasks += 1;
            }
        }

        // Store result
        let task_key = task_id.0.clone();
        self.results.write().insert(task_key.clone(), result);

        // Remove assignment
        self.task_assignments.write().remove(&task_key);
    }

    fn agents(&self) -> Vec<AgentId> {
        self.agents
            .read()
            .values()
            .map(|info| info.id.clone())
            .collect()
    }

    fn add_agent(&mut self, agent_id: AgentId) -> Result<()> {
        let agent_key = agent_id_to_key(&agent_id);
        let mut agents = self.agents.write();

        if agents.contains_key(&agent_key) {
            return Err(anyhow!("Agent already registered: {}", agent_key));
        }

        agents.insert(
            agent_key.clone(),
            AgentInfo {
                id: agent_id.clone(),
                active_tasks: 0,
                completed_tasks: 0,
            },
        );

        eprintln!(
            "[ConversationalTopology] Added agent {} (total: {})",
            agent_key,
            agents.len()
        );

        Ok(())
    }

    fn remove_agent(&mut self, agent_id: AgentId) -> Result<()> {
        let agent_key = agent_id_to_key(&agent_id);
        let mut agents = self.agents.write();

        if agents.remove(&agent_key).is_none() {
            return Err(anyhow!("Agent not found: {}", agent_key));
        }

        eprintln!(
            "[ConversationalTopology] Removed agent {} (remaining: {})",
            agent_key,
            agents.len()
        );

        Ok(())
    }
}

/// Statistics about the conversational topology.
#[derive(Debug, Clone)]
pub struct ConversationalStats {
    pub total_agents: usize,
    pub total_active_tasks: usize,
    pub total_completed_tasks: usize,
    pub total_debates: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::types::{QueenId, TaskStatus};
    use std::time::Duration;

    #[test]
    fn test_vote_tally_majority() {
        let tallier = VoteTallier::new(VotingProtocol::Majority);

        let votes = vec![
            Vote {
                voter: "v1".to_string(),
                target: "a1".to_string(),
                approve: true,
            },
            Vote {
                voter: "v2".to_string(),
                target: "a1".to_string(),
                approve: true,
            },
            Vote {
                voter: "v3".to_string(),
                target: "a2".to_string(),
                approve: true,
            },
        ];

        let winner = tallier.tally(&votes, 3);
        assert_eq!(winner, Some("a1".to_string()));
    }

    #[test]
    fn test_vote_tally_approval() {
        let tallier = VoteTallier::new(VotingProtocol::Approval);

        let votes = vec![
            Vote {
                voter: "v1".to_string(),
                target: "a1".to_string(),
                approve: true,
            },
            Vote {
                voter: "v2".to_string(),
                target: "a2".to_string(),
                approve: true,
            },
            Vote {
                voter: "v3".to_string(),
                target: "a1".to_string(),
                approve: true,
            },
        ];

        let winner = tallier.tally(&votes, 3);
        assert_eq!(winner, Some("a1".to_string()));
    }

    #[test]
    fn test_conversational_topology() {
        let config = ConversationalConfig::default();
        let mut topology = ConversationalTopology::new(config);

        let agent1 = AgentId::Queen(QueenId("Q1".to_string()));
        let agent2 = AgentId::Queen(QueenId("Q2".to_string()));

        topology.add_agent(agent1.clone()).unwrap();
        topology.add_agent(agent2.clone()).unwrap();

        let task1 = TaskId("task1".to_string());
        let assigned = topology.assign_task(&task1).unwrap();
        assert_eq!(assigned.len(), 1);

        // Complete task
        let result = TaskResult {
            status: TaskStatus::Completed,
            output: "done".to_string(),
            artifacts: vec![],
            duration: Duration::from_secs(1),
            git_sha: None,
        };
        topology.handle_completion(assigned[0].clone(), task1, result);
    }
}
