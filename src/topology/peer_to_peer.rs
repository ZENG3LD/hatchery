//! Peer-to-peer topology — flat swarm with consensus-based task assignment.

use super::{agent_id_to_key, Topology};
use crate::core::types::{AgentId, TaskId, TaskResult};
use anyhow::{anyhow, Result};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Configuration for peer-to-peer topology.
#[derive(Debug, Clone)]
pub struct PeerToPeerConfig {
    /// Interval for peer discovery broadcasts
    pub discovery_interval: Duration,
    /// Consensus threshold (0.0-1.0) for task assignment votes
    pub consensus_threshold: f64,
}

impl Default for PeerToPeerConfig {
    fn default() -> Self {
        PeerToPeerConfig {
            discovery_interval: Duration::from_secs(30),
            consensus_threshold: 0.51, // Simple majority
        }
    }
}

/// Peer information in the registry.
#[derive(Debug, Clone)]
struct PeerInfo {
    id: AgentId,
    last_seen: Instant,
    active_tasks: usize,
    completed_tasks: usize,
}

/// Vote cast by a peer for task assignment.
#[derive(Debug, Clone)]
struct Vote {
    voter: String,
    candidate: String,
    timestamp: Instant,
}

/// Consensus engine for peer-to-peer voting.
struct ConsensusEngine {
    threshold: f64,
}

impl ConsensusEngine {
    fn new(threshold: f64) -> Self {
        ConsensusEngine { threshold }
    }

    /// Tally votes and determine winner based on consensus threshold.
    fn tally_votes(&self, votes: &[Vote], total_peers: usize) -> Option<String> {
        if votes.is_empty() || total_peers == 0 {
            return None;
        }

        // Count votes per candidate
        let mut vote_counts: HashMap<String, usize> = HashMap::new();
        for vote in votes {
            *vote_counts.entry(vote.candidate.clone()).or_insert(0) += 1;
        }

        // Find candidate with most votes
        let mut best_candidate: Option<String> = None;
        let mut best_count = 0;

        for (candidate, count) in vote_counts {
            if count > best_count {
                best_count = count;
                best_candidate = Some(candidate);
            }
        }

        // Check if winner meets consensus threshold
        if let Some(winner) = best_candidate {
            let vote_ratio = best_count as f64 / total_peers as f64;
            if vote_ratio >= self.threshold {
                return Some(winner);
            }
        }

        None
    }

    /// Handle split-brain scenario by selecting highest AgentId as tiebreaker.
    fn resolve_split_brain(&self, candidates: &[String]) -> Option<String> {
        if candidates.is_empty() {
            return None;
        }

        // Sort candidates lexicographically and pick highest
        let mut sorted = candidates.to_vec();
        sorted.sort();
        sorted.last().cloned()
    }
}

/// Peer-to-peer topology implementation.
///
/// A flat swarm where agents vote on task assignments via consensus.
/// No central authority — decisions are made collectively.
pub struct PeerToPeerTopology {
    config: PeerToPeerConfig,
    /// Peer registry
    peers: Arc<RwLock<HashMap<String, PeerInfo>>>,
    /// Consensus engine
    consensus: ConsensusEngine,
    /// Last discovery broadcast time
    last_discovery: Arc<RwLock<Instant>>,
    /// Vote history for current round
    vote_history: Arc<RwLock<Vec<Vote>>>,
    /// Task assignments
    task_assignments: Arc<RwLock<HashMap<String, String>>>,
    /// Completed results
    results: Arc<RwLock<HashMap<String, TaskResult>>>,
}

impl PeerToPeerTopology {
    /// Create a new peer-to-peer topology with the given configuration.
    pub fn new(config: PeerToPeerConfig) -> Self {
        let consensus = ConsensusEngine::new(config.consensus_threshold);

        PeerToPeerTopology {
            config,
            peers: Arc::new(RwLock::new(HashMap::new())),
            consensus,
            last_discovery: Arc::new(RwLock::new(Instant::now())),
            vote_history: Arc::new(RwLock::new(Vec::new())),
            task_assignments: Arc::new(RwLock::new(HashMap::new())),
            results: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Simulate gossip protocol for peer discovery.
    fn gossip_discovery(&self) {
        let now = Instant::now();
        let mut last_discovery = self.last_discovery.write();

        if now.duration_since(*last_discovery) >= self.config.discovery_interval {
            eprintln!("[PeerToPeerTopology] Running peer discovery via gossip protocol");

            // Update last_seen for all peers
            let mut peers = self.peers.write();
            for peer in peers.values_mut() {
                peer.last_seen = now;
            }

            *last_discovery = now;
        }
    }

    /// Remove stale peers that haven't been seen recently.
    fn prune_stale_peers(&self, timeout: Duration) {
        let now = Instant::now();
        let mut peers = self.peers.write();

        let before = peers.len();
        peers.retain(|_, peer| now.duration_since(peer.last_seen) < timeout);
        let after = peers.len();

        if before != after {
            eprintln!(
                "[PeerToPeerTopology] Pruned {} stale peers ({} -> {})",
                before - after,
                before,
                after
            );
        }
    }

    /// Simulate voting process for task assignment.
    fn conduct_voting(&self, task_id: &TaskId) -> Result<AgentId> {
        eprintln!("[PeerToPeerTopology] Conducting vote for task {}", task_id.0);

        let peers = self.peers.read();
        if peers.is_empty() {
            return Err(anyhow!("No peers available for voting"));
        }

        // Simulate each peer voting for the least-loaded peer
        let mut votes = Vec::new();
        let peer_list: Vec<_> = peers.values().collect();

        for voter in &peer_list {
            // Each peer votes for the least-loaded peer
            let mut least_loaded = peer_list[0];
            for peer in &peer_list {
                if peer.active_tasks < least_loaded.active_tasks {
                    least_loaded = peer;
                }
            }

            votes.push(Vote {
                voter: agent_id_to_key(&voter.id),
                candidate: agent_id_to_key(&least_loaded.id),
                timestamp: Instant::now(),
            });
        }

        // Store vote history
        *self.vote_history.write() = votes.clone();

        // Tally votes
        let total_peers = peers.len();
        drop(peers); // Release lock before tallying

        if let Some(winner_key) = self.consensus.tally_votes(&votes, total_peers) {
            let peers = self.peers.read();
            peers
                .get(&winner_key)
                .map(|peer| peer.id.clone())
                .ok_or_else(|| anyhow!("Winner not found in peer registry: {}", winner_key))
        } else {
            // No consensus reached, use split-brain resolution
            eprintln!("[PeerToPeerTopology] No consensus reached, using split-brain resolution");

            let candidates: Vec<_> = votes.iter().map(|v| v.candidate.clone()).collect();
            if let Some(winner_key) = self.consensus.resolve_split_brain(&candidates) {
                let peers = self.peers.read();
                peers
                    .get(&winner_key)
                    .map(|peer| peer.id.clone())
                    .ok_or_else(|| anyhow!("Split-brain winner not found: {}", winner_key))
            } else {
                Err(anyhow!("Failed to resolve task assignment via voting"))
            }
        }
    }

    /// Get statistics about the peer network.
    pub fn stats(&self) -> PeerToPeerStats {
        let peers = self.peers.read();
        let total_active = peers.values().map(|p| p.active_tasks).sum();
        let total_completed = peers.values().map(|p| p.completed_tasks).sum();

        PeerToPeerStats {
            total_peers: peers.len(),
            total_active_tasks: total_active,
            total_completed_tasks: total_completed,
            last_vote_count: self.vote_history.read().len(),
        }
    }
}

impl Topology for PeerToPeerTopology {
    fn assign_task(&mut self, task_id: &TaskId) -> Result<Vec<AgentId>> {
        // Run gossip discovery
        self.gossip_discovery();

        // Prune stale peers (timeout: 5x discovery interval)
        let timeout = self.config.discovery_interval * 5;
        self.prune_stale_peers(timeout);

        // Conduct voting
        let winner = self.conduct_voting(task_id)?;
        let winner_key = agent_id_to_key(&winner);

        // Update peer info
        let mut peers = self.peers.write();
        if let Some(peer) = peers.get_mut(&winner_key) {
            peer.active_tasks += 1;
        }
        drop(peers);

        // Record assignment
        let task_key = task_id.0.clone();
        self.task_assignments.write().insert(task_key, winner_key.clone());

        eprintln!(
            "[PeerToPeerTopology] Assigned task {} to peer {} via consensus",
            task_id.0, winner_key
        );

        Ok(vec![winner])
    }

    fn handle_completion(&mut self, agent_id: AgentId, task_id: TaskId, result: TaskResult) {
        let agent_key = agent_id_to_key(&agent_id);
        eprintln!(
            "[PeerToPeerTopology] Peer {} completed task {} with status {:?}",
            agent_key, task_id.0, result.status
        );

        // Update peer info
        let mut peers = self.peers.write();
        if let Some(peer) = peers.get_mut(&agent_key) {
            peer.active_tasks = peer.active_tasks.saturating_sub(1);
            peer.completed_tasks += 1;
            peer.last_seen = Instant::now();
        }
        drop(peers);

        // Store result
        let task_key = task_id.0.clone();
        self.results.write().insert(task_key.clone(), result);

        // Remove assignment
        self.task_assignments.write().remove(&task_key);
    }

    fn agents(&self) -> Vec<AgentId> {
        self.peers
            .read()
            .values()
            .map(|peer| peer.id.clone())
            .collect()
    }

    fn add_agent(&mut self, agent_id: AgentId) -> Result<()> {
        let agent_key = agent_id_to_key(&agent_id);
        let mut peers = self.peers.write();

        if peers.contains_key(&agent_key) {
            return Err(anyhow!("Peer already registered: {}", agent_key));
        }

        peers.insert(
            agent_key.clone(),
            PeerInfo {
                id: agent_id.clone(),
                last_seen: Instant::now(),
                active_tasks: 0,
                completed_tasks: 0,
            },
        );

        eprintln!(
            "[PeerToPeerTopology] Added peer {} (total: {})",
            agent_key,
            peers.len()
        );

        Ok(())
    }

    fn remove_agent(&mut self, agent_id: AgentId) -> Result<()> {
        let agent_key = agent_id_to_key(&agent_id);
        let mut peers = self.peers.write();

        if peers.remove(&agent_key).is_none() {
            return Err(anyhow!("Peer not found: {}", agent_key));
        }

        eprintln!(
            "[PeerToPeerTopology] Removed peer {} (remaining: {})",
            agent_key,
            peers.len()
        );

        Ok(())
    }
}

/// Statistics about the peer-to-peer topology.
#[derive(Debug, Clone)]
pub struct PeerToPeerStats {
    pub total_peers: usize,
    pub total_active_tasks: usize,
    pub total_completed_tasks: usize,
    pub last_vote_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::types::{QueenId, TaskStatus};

    #[test]
    fn test_peer_to_peer_add_remove() {
        let config = PeerToPeerConfig::default();
        let mut topology = PeerToPeerTopology::new(config);

        let peer1 = AgentId::Queen(QueenId("Q1".to_string()));
        let peer2 = AgentId::Queen(QueenId("Q2".to_string()));

        topology.add_agent(peer1.clone()).unwrap();
        topology.add_agent(peer2.clone()).unwrap();

        assert_eq!(topology.agents().len(), 2);

        topology.remove_agent(peer1).unwrap();
        assert_eq!(topology.agents().len(), 1);
    }

    #[test]
    fn test_peer_to_peer_voting() {
        let config = PeerToPeerConfig {
            discovery_interval: Duration::from_secs(30),
            consensus_threshold: 0.51,
        };
        let mut topology = PeerToPeerTopology::new(config);

        let peer1 = AgentId::Queen(QueenId("Q1".to_string()));
        let peer2 = AgentId::Queen(QueenId("Q2".to_string()));
        let peer3 = AgentId::Queen(QueenId("Q3".to_string()));

        topology.add_agent(peer1.clone()).unwrap();
        topology.add_agent(peer2.clone()).unwrap();
        topology.add_agent(peer3.clone()).unwrap();

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

    #[test]
    fn test_consensus_tally() {
        let engine = ConsensusEngine::new(0.51);

        let votes = vec![
            Vote {
                voter: "v1".to_string(),
                candidate: "c1".to_string(),
                timestamp: Instant::now(),
            },
            Vote {
                voter: "v2".to_string(),
                candidate: "c1".to_string(),
                timestamp: Instant::now(),
            },
            Vote {
                voter: "v3".to_string(),
                candidate: "c2".to_string(),
                timestamp: Instant::now(),
            },
        ];

        let winner = engine.tally_votes(&votes, 3);
        assert_eq!(winner, Some("c1".to_string()));
    }

    #[test]
    fn test_split_brain_resolution() {
        let engine = ConsensusEngine::new(0.51);

        let candidates = vec!["agent1".to_string(), "agent2".to_string(), "agent3".to_string()];
        let winner = engine.resolve_split_brain(&candidates);

        // Should pick "agent3" as it's highest lexicographically
        assert_eq!(winner, Some("agent3".to_string()));
    }
}
