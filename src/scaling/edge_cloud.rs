//! Edge/cloud hybrid deployment.
//!
//! Manages agents across edge nodes and cloud pools, with latency-aware
//! placement and migration capabilities.

use super::{generate_queen_id, ScaleDecision, ScaleMetrics, Scaling};
use crate::core::types::AgentId;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::time::Duration;

// ============================================================================
// Configuration
// ============================================================================

/// Configuration for edge/cloud hybrid deployment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeCloudConfig {
    /// Maximum agents per edge node
    pub edge_capacity: usize,
    /// Maximum agents in cloud pool
    pub cloud_capacity: usize,
    /// Prefer edge placement when possible
    pub prefer_edge: bool,
    /// Latency threshold for edge eligibility (ms)
    pub latency_threshold: Duration,
    /// Scale up threshold (queue pressure)
    pub scale_up_threshold: f64,
    /// Scale down threshold (idle ratio)
    pub scale_down_threshold: f64,
}

impl Default for EdgeCloudConfig {
    fn default() -> Self {
        EdgeCloudConfig {
            edge_capacity: 50,
            cloud_capacity: 200,
            prefer_edge: true,
            latency_threshold: Duration::from_millis(100),
            scale_up_threshold: 1.5,
            scale_down_threshold: 0.5,
        }
    }
}

impl EdgeCloudConfig {
    /// Create a new config.
    pub fn new(edge_capacity: usize, cloud_capacity: usize, prefer_edge: bool) -> Self {
        EdgeCloudConfig {
            edge_capacity,
            cloud_capacity,
            prefer_edge,
            ..Default::default()
        }
    }

    /// Validate configuration.
    pub fn validate(&self) -> Result<()> {
        if self.edge_capacity == 0 && self.cloud_capacity == 0 {
            anyhow::bail!("At least one of edge_capacity or cloud_capacity must be non-zero");
        }
        if self.scale_up_threshold <= 0.0 {
            anyhow::bail!("scale_up_threshold must be positive");
        }
        if self.scale_down_threshold < 0.0 || self.scale_down_threshold > 1.0 {
            anyhow::bail!("scale_down_threshold must be between 0.0 and 1.0");
        }
        Ok(())
    }
}

// ============================================================================
// EdgeNode
// ============================================================================

/// An edge computing node.
#[derive(Debug, Clone)]
pub struct EdgeNode {
    /// Node identifier
    pub id: String,
    /// Agents currently on this edge node
    pub agents: Vec<AgentId>,
    /// Maximum capacity
    pub capacity: usize,
    /// Average latency to this node
    pub latency: Duration,
}

impl EdgeNode {
    /// Create a new edge node.
    pub fn new(id: String, capacity: usize, latency: Duration) -> Self {
        EdgeNode {
            id,
            agents: Vec::new(),
            capacity,
            latency,
        }
    }

    /// Check if node has available capacity.
    pub fn has_capacity(&self) -> bool {
        self.agents.len() < self.capacity
    }

    /// Get current utilization (0.0 to 1.0).
    pub fn utilization(&self) -> f64 {
        if self.capacity == 0 {
            return 1.0;
        }
        self.agents.len() as f64 / self.capacity as f64
    }

    /// Add an agent to this node.
    pub fn add_agent(&mut self, agent_id: AgentId) -> Result<()> {
        if self.agents.len() >= self.capacity {
            return Err(anyhow::anyhow!("Edge node at capacity"));
        }
        self.agents.push(agent_id);
        Ok(())
    }

    /// Remove an agent from this node.
    pub fn remove_agent(&mut self, index: usize) -> Option<AgentId> {
        if index < self.agents.len() {
            Some(self.agents.remove(index))
        } else {
            None
        }
    }
}

// ============================================================================
// CloudPool
// ============================================================================

/// Cloud agent pool.
#[derive(Debug, Clone)]
pub struct CloudPool {
    /// Agents in the cloud pool
    pub agents: Vec<AgentId>,
    /// Maximum capacity
    pub capacity: usize,
}

impl CloudPool {
    /// Create a new cloud pool.
    pub fn new(capacity: usize) -> Self {
        CloudPool {
            agents: Vec::new(),
            capacity,
        }
    }

    /// Check if pool has available capacity.
    pub fn has_capacity(&self) -> bool {
        self.agents.len() < self.capacity
    }

    /// Get current utilization (0.0 to 1.0).
    pub fn utilization(&self) -> f64 {
        if self.capacity == 0 {
            return 1.0;
        }
        self.agents.len() as f64 / self.capacity as f64
    }

    /// Add an agent to the pool.
    pub fn add_agent(&mut self, agent_id: AgentId) -> Result<()> {
        if self.agents.len() >= self.capacity {
            return Err(anyhow::anyhow!("Cloud pool at capacity"));
        }
        self.agents.push(agent_id);
        Ok(())
    }

    /// Remove an agent from the pool.
    pub fn remove_agent(&mut self, index: usize) -> Option<AgentId> {
        if index < self.agents.len() {
            Some(self.agents.remove(index))
        } else {
            None
        }
    }
}

// ============================================================================
// EdgeCloudScaling
// ============================================================================

/// Edge/cloud hybrid scaling implementation.
pub struct EdgeCloudScaling {
    config: EdgeCloudConfig,
    edge_nodes: Vec<EdgeNode>,
    cloud_pool: CloudPool,
    next_agent_number: usize,
}

impl EdgeCloudScaling {
    /// Create a new edge/cloud scaling instance.
    pub fn new(config: EdgeCloudConfig) -> Result<Self> {
        config.validate()?;

        // Create default edge node
        let mut edge_nodes = Vec::new();
        if config.edge_capacity > 0 {
            edge_nodes.push(EdgeNode::new(
                "edge-0".to_string(),
                config.edge_capacity,
                Duration::from_millis(10),
            ));
        }

        let cloud_pool = CloudPool::new(config.cloud_capacity);

        Ok(EdgeCloudScaling {
            config,
            edge_nodes,
            cloud_pool,
            next_agent_number: 0,
        })
    }

    /// Create with default configuration.
    pub fn default() -> Result<Self> {
        Self::new(EdgeCloudConfig::default())
    }

    /// Get total agent count.
    pub fn total_agents(&self) -> usize {
        let edge_count: usize = self.edge_nodes.iter().map(|n| n.agents.len()).sum();
        edge_count + self.cloud_pool.agents.len()
    }

    /// Get total edge agents.
    pub fn edge_agents(&self) -> usize {
        self.edge_nodes.iter().map(|n| n.agents.len()).sum()
    }

    /// Get total cloud agents.
    pub fn cloud_agents(&self) -> usize {
        self.cloud_pool.agents.len()
    }

    /// Make placement decision for a new agent.
    pub fn placement_decision(&self) -> PlacementLocation {
        if self.config.prefer_edge {
            // Try edge first
            for (i, node) in self.edge_nodes.iter().enumerate() {
                if node.has_capacity() {
                    return PlacementLocation::Edge(i);
                }
            }
            // Fall back to cloud
            if self.cloud_pool.has_capacity() {
                return PlacementLocation::Cloud;
            }
        } else {
            // Try cloud first
            if self.cloud_pool.has_capacity() {
                return PlacementLocation::Cloud;
            }
            // Fall back to edge
            for (i, node) in self.edge_nodes.iter().enumerate() {
                if node.has_capacity() {
                    return PlacementLocation::Edge(i);
                }
            }
        }

        PlacementLocation::NoCapacity
    }

    /// Migrate an agent from cloud to edge.
    pub fn migrate_to_edge(&mut self, cloud_index: usize) -> Result<()> {
        // Find edge node with capacity
        let edge_idx = self.edge_nodes
            .iter()
            .position(|n| n.has_capacity())
            .ok_or_else(|| anyhow::anyhow!("No edge capacity available"))?;

        // Remove from cloud
        let agent_id = self.cloud_pool.remove_agent(cloud_index)
            .ok_or_else(|| anyhow::anyhow!("Agent not found in cloud pool"))?;

        // Add to edge
        self.edge_nodes[edge_idx].add_agent(agent_id)?;

        eprintln!("[EdgeCloudScaling] Migrated agent from cloud to edge node {}",
            self.edge_nodes[edge_idx].id);

        Ok(())
    }

    /// Migrate an agent from edge to cloud.
    pub fn migrate_to_cloud(&mut self, edge_idx: usize, agent_idx: usize) -> Result<()> {
        if edge_idx >= self.edge_nodes.len() {
            return Err(anyhow::anyhow!("Invalid edge node index"));
        }

        if !self.cloud_pool.has_capacity() {
            return Err(anyhow::anyhow!("Cloud pool at capacity"));
        }

        // Remove from edge
        let agent_id = self.edge_nodes[edge_idx].remove_agent(agent_idx)
            .ok_or_else(|| anyhow::anyhow!("Agent not found on edge node"))?;

        // Add to cloud
        self.cloud_pool.add_agent(agent_id)?;

        eprintln!("[EdgeCloudScaling] Migrated agent from edge node {} to cloud",
            self.edge_nodes[edge_idx].id);

        Ok(())
    }

    /// Calculate scale up count.
    fn calculate_scale_up(&self, metrics: &ScaleMetrics) -> usize {
        let total_capacity = self.config.edge_capacity + self.config.cloud_capacity;
        let current = self.total_agents();
        if current >= total_capacity {
            return 0;
        }

        let pressure = metrics.queue_pressure();
        if pressure > self.config.scale_up_threshold {
            let increment = ((current as f64 * 0.1).max(1.0)) as usize;
            increment.min(total_capacity - current)
        } else {
            0
        }
    }

    /// Calculate scale down count.
    fn calculate_scale_down(&self, metrics: &ScaleMetrics) -> usize {
        let current = self.total_agents();
        if current == 0 {
            return 0;
        }

        let idle_ratio = metrics.idle_agents as f64 / current as f64;
        if idle_ratio > self.config.scale_down_threshold {
            (metrics.idle_agents / 2).max(1)
        } else {
            0
        }
    }

    /// Add agents using placement strategy.
    fn add_agents(&mut self, count: usize) -> Vec<AgentId> {
        let mut new_agents = Vec::with_capacity(count);

        for _ in 0..count {
            let placement = self.placement_decision();
            let agent_id = generate_queen_id(&format!("ec-Q{}", self.next_agent_number));
            self.next_agent_number += 1;

            match placement {
                PlacementLocation::Edge(idx) => {
                    if let Err(e) = self.edge_nodes[idx].add_agent(agent_id.clone()) {
                        eprintln!("[EdgeCloudScaling] Failed to add agent to edge: {}", e);
                        continue;
                    }
                }
                PlacementLocation::Cloud => {
                    if let Err(e) = self.cloud_pool.add_agent(agent_id.clone()) {
                        eprintln!("[EdgeCloudScaling] Failed to add agent to cloud: {}", e);
                        continue;
                    }
                }
                PlacementLocation::NoCapacity => {
                    eprintln!("[EdgeCloudScaling] No capacity available for new agent");
                    break;
                }
            }

            new_agents.push(agent_id);
        }

        new_agents
    }

    /// Remove agents (cloud first, then edge).
    fn remove_agents(&mut self, count: usize) -> Vec<AgentId> {
        let mut removed = Vec::with_capacity(count);

        // Remove from cloud first
        for _ in 0..count {
            if self.cloud_pool.agents.len() > 0 {
                let idx = self.cloud_pool.agents.len() - 1;
                if let Some(agent_id) = self.cloud_pool.remove_agent(idx) {
                    removed.push(agent_id);
                    continue;
                }
            }

            // Then remove from edge
            let mut removed_from_edge = false;
            for node in &mut self.edge_nodes {
                if node.agents.len() > 0 {
                    let idx = node.agents.len() - 1;
                    if let Some(agent_id) = node.remove_agent(idx) {
                        removed.push(agent_id);
                        removed_from_edge = true;
                        break;
                    }
                }
            }

            if !removed_from_edge {
                break;
            }
        }

        removed
    }
}

/// Placement location for new agents.
#[derive(Debug, Clone, PartialEq)]
pub enum PlacementLocation {
    /// Place on edge node at index
    Edge(usize),
    /// Place in cloud pool
    Cloud,
    /// No capacity available
    NoCapacity,
}

impl Scaling for EdgeCloudScaling {
    fn should_scale(&self, metrics: &ScaleMetrics) -> Result<ScaleDecision> {
        // Check scale up first
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
                let new_agents = self.add_agents(count);
                eprintln!("[EdgeCloudScaling] Scaled up by {} agents (edge: {}, cloud: {})",
                    new_agents.len(), self.edge_agents(), self.cloud_agents());
                Ok(new_agents)
            }
            ScaleDecision::ScaleDown { count } => {
                let removed = self.remove_agents(count);
                eprintln!("[EdgeCloudScaling] Scaled down by {} agents (edge: {}, cloud: {})",
                    removed.len(), self.edge_agents(), self.cloud_agents());
                Ok(removed)
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
    fn test_edge_node() {
        let mut node = EdgeNode::new("edge-0".to_string(), 10, Duration::from_millis(10));
        assert!(node.has_capacity());

        node.add_agent(generate_queen_id("agent-1")).unwrap();
        assert_eq!(node.agents.len(), 1);
        assert_eq!(node.utilization(), 0.1);
    }

    #[test]
    fn test_cloud_pool() {
        let mut pool = CloudPool::new(100);
        assert!(pool.has_capacity());

        pool.add_agent(generate_queen_id("agent-1")).unwrap();
        assert_eq!(pool.agents.len(), 1);
    }

    #[test]
    fn test_placement_prefer_edge() {
        let config = EdgeCloudConfig::new(10, 100, true);
        let scaling = EdgeCloudScaling::new(config).unwrap();

        let placement = scaling.placement_decision();
        assert_eq!(placement, PlacementLocation::Edge(0));
    }

    #[test]
    fn test_placement_prefer_cloud() {
        let config = EdgeCloudConfig::new(10, 100, false);
        let scaling = EdgeCloudScaling::new(config).unwrap();

        let placement = scaling.placement_decision();
        assert_eq!(placement, PlacementLocation::Cloud);
    }

    #[test]
    fn test_scale_up_distribution() {
        let config = EdgeCloudConfig::new(5, 10, true);
        let mut scaling = EdgeCloudScaling::new(config).unwrap();

        let agents = scaling.add_agents(8);
        assert_eq!(agents.len(), 8);
        assert_eq!(scaling.edge_agents(), 5); // Edge full
        assert_eq!(scaling.cloud_agents(), 3); // Overflow to cloud
    }

    #[test]
    fn test_scale_down_cloud_first() {
        let config = EdgeCloudConfig::new(5, 10, true);
        let mut scaling = EdgeCloudScaling::new(config).unwrap();

        // Add agents
        scaling.add_agents(8);

        // Remove 4 agents (should remove from cloud first)
        let removed = scaling.remove_agents(4);
        assert_eq!(removed.len(), 4);
        assert_eq!(scaling.cloud_agents(), 0); // Cloud emptied first
        assert_eq!(scaling.edge_agents(), 4);  // Then edge
    }

    #[test]
    fn test_migration() {
        let config = EdgeCloudConfig::new(5, 10, true);
        let mut scaling = EdgeCloudScaling::new(config).unwrap();

        // Fill cloud
        scaling.cloud_pool.add_agent(generate_queen_id("agent-1")).unwrap();

        // Migrate to edge
        scaling.migrate_to_edge(0).unwrap();
        assert_eq!(scaling.cloud_agents(), 0);
        assert_eq!(scaling.edge_agents(), 1);
    }
}
