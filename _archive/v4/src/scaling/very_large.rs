//! Very large-scale deployment (1000+ agents).
//!
//! Peer-to-peer mesh architecture with partitions. Agents are organized into
//! partitions that can discover and communicate with each other via gossip.

use super::{generate_queen_id, ScaleDecision, ScaleMetrics, Scaling};
use crate::core::types::AgentId;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::time::Duration;

// ============================================================================
// Configuration
// ============================================================================

/// Configuration for very large-scale deployment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VeryLargeScaleConfig {
    /// Minimum number of agents to maintain
    pub min_agents: usize,
    /// How often to run partition discovery
    pub peer_discovery_interval: Duration,
    /// Target size for each partition
    pub partition_size: usize,
    /// Maximum size before splitting partition
    pub max_partition_size: usize,
    /// Minimum size before merging partitions
    pub min_partition_size: usize,
}

impl Default for VeryLargeScaleConfig {
    fn default() -> Self {
        VeryLargeScaleConfig {
            min_agents: 1000,
            peer_discovery_interval: Duration::from_secs(60),
            partition_size: 100,
            max_partition_size: 200,
            min_partition_size: 50,
        }
    }
}

impl VeryLargeScaleConfig {
    /// Create a new config.
    pub fn new(min_agents: usize, partition_size: usize) -> Self {
        VeryLargeScaleConfig {
            min_agents,
            partition_size,
            max_partition_size: partition_size * 2,
            min_partition_size: partition_size / 2,
            ..Default::default()
        }
    }

    /// Validate configuration.
    pub fn validate(&self) -> Result<()> {
        if self.min_agents == 0 {
            anyhow::bail!("min_agents must be at least 1");
        }
        if self.partition_size == 0 {
            anyhow::bail!("partition_size must be at least 1");
        }
        if self.max_partition_size < self.partition_size {
            anyhow::bail!("max_partition_size must be >= partition_size");
        }
        if self.min_partition_size > self.partition_size {
            anyhow::bail!("min_partition_size must be <= partition_size");
        }
        Ok(())
    }
}

// ============================================================================
// Partition
// ============================================================================

/// A partition in the P2P mesh.
#[derive(Debug, Clone)]
pub struct Partition {
    /// Unique partition ID
    pub id: String,
    /// Agents in this partition
    pub agents: Vec<AgentId>,
    /// Current load (0.0 to 1.0)
    pub load: f64,
    /// Neighboring partition IDs (for gossip)
    pub neighbors: HashSet<String>,
}

impl Partition {
    /// Create a new partition.
    pub fn new(id: String) -> Self {
        Partition {
            id,
            agents: Vec::new(),
            load: 0.0,
            neighbors: HashSet::new(),
        }
    }

    /// Add an agent to this partition.
    pub fn add_agent(&mut self, agent_id: AgentId) {
        self.agents.push(agent_id);
    }

    /// Remove an agent from this partition.
    pub fn remove_agent(&mut self, index: usize) -> Option<AgentId> {
        if index < self.agents.len() {
            Some(self.agents.remove(index))
        } else {
            None
        }
    }

    /// Get the number of agents in this partition.
    pub fn size(&self) -> usize {
        self.agents.len()
    }

    /// Add a neighbor partition.
    pub fn add_neighbor(&mut self, neighbor_id: String) {
        self.neighbors.insert(neighbor_id);
    }

    /// Remove a neighbor partition.
    pub fn remove_neighbor(&mut self, neighbor_id: &str) {
        self.neighbors.remove(neighbor_id);
    }
}

// ============================================================================
// VeryLargeScaleScaling
// ============================================================================

/// Very large-scale scaling with P2P mesh and partitions.
pub struct VeryLargeScaleScaling {
    config: VeryLargeScaleConfig,
    partitions: HashMap<String, Partition>,
    next_partition_id: usize,
    next_agent_number: usize,
}

impl VeryLargeScaleScaling {
    /// Create a new very large-scale scaling instance.
    pub fn new(config: VeryLargeScaleConfig) -> Result<Self> {
        config.validate()?;

        let mut partitions = HashMap::new();
        let initial_partition = Partition::new("P0".to_string());
        partitions.insert("P0".to_string(), initial_partition);

        Ok(VeryLargeScaleScaling {
            config,
            partitions,
            next_partition_id: 1,
            next_agent_number: 0,
        })
    }

    /// Create with default configuration.
    pub fn default() -> Result<Self> {
        Self::new(VeryLargeScaleConfig::default())
    }

    /// Get total agent count across all partitions.
    pub fn total_agents(&self) -> usize {
        self.partitions.values().map(|p| p.size()).sum()
    }

    /// Find the partition with the lightest load.
    fn lightest_partition(&self) -> Option<&String> {
        self.partitions
            .iter()
            .min_by(|a, b| a.1.load.partial_cmp(&b.1.load).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(id, _)| id)
    }

    /// Find the partition with the heaviest load.
    fn heaviest_partition(&self) -> Option<&String> {
        self.partitions
            .iter()
            .max_by(|a, b| a.1.load.partial_cmp(&b.1.load).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(id, _)| id)
    }

    /// Split an overloaded partition into two.
    pub fn split_partition(&mut self, partition_id: &str) -> Result<String> {
        let partition = self.partitions.get_mut(partition_id)
            .ok_or_else(|| anyhow::anyhow!("Partition not found: {}", partition_id))?;

        if partition.size() <= self.config.min_partition_size {
            return Err(anyhow::anyhow!("Partition too small to split"));
        }

        // Create new partition
        let new_id = format!("P{}", self.next_partition_id);
        self.next_partition_id += 1;
        let mut new_partition = Partition::new(new_id.clone());

        // Move half the agents to new partition
        let split_point = partition.size() / 2;
        let moved_agents = partition.agents.split_off(split_point);
        new_partition.agents = moved_agents;
        new_partition.load = partition.load;

        // Update neighbor relationships
        new_partition.add_neighbor(partition_id.to_string());
        partition.add_neighbor(new_id.clone());

        self.partitions.insert(new_id.clone(), new_partition);

        eprintln!("[VeryLargeScaleScaling] Split partition {} into {} and {}",
            partition_id, partition_id, new_id);

        Ok(new_id)
    }

    /// Merge two underutilized partitions.
    pub fn merge_partitions(&mut self, source_id: &str, target_id: &str) -> Result<()> {
        if source_id == target_id {
            return Err(anyhow::anyhow!("Cannot merge partition with itself"));
        }

        // Remove source partition
        let mut source = self.partitions.remove(source_id)
            .ok_or_else(|| anyhow::anyhow!("Source partition not found"))?;

        // Collect neighbor updates before mutating target
        let neighbors_to_update: Vec<String> = source.neighbors.iter()
            .filter(|n| *n != target_id)
            .cloned()
            .collect();

        // Add all agents to target
        let target = self.partitions.get_mut(target_id)
            .ok_or_else(|| anyhow::anyhow!("Target partition not found"))?;

        target.agents.append(&mut source.agents);
        target.neighbors.remove(source_id);

        // Add new neighbors to target
        for neighbor_id in neighbors_to_update.iter() {
            target.add_neighbor(neighbor_id.clone());
        }

        // Update neighbor relationships
        for neighbor_id in neighbors_to_update {
            if let Some(neighbor) = self.partitions.get_mut(&neighbor_id) {
                neighbor.remove_neighbor(source_id);
                neighbor.add_neighbor(target_id.to_string());
            }
        }

        eprintln!("[VeryLargeScaleScaling] Merged partition {} into {}", source_id, target_id);

        Ok(())
    }

    /// Rebalance agents across partitions.
    pub fn rebalance_partitions(&mut self) -> Result<()> {
        // Calculate average partition size
        let total_agents = self.total_agents();
        let num_partitions = self.partitions.len();
        if num_partitions == 0 {
            return Ok(());
        }

        let _target_size = total_agents / num_partitions;

        // Check each partition
        let partition_ids: Vec<String> = self.partitions.keys().cloned().collect();

        for partition_id in partition_ids {
            let size = self.partitions.get(&partition_id).map(|p| p.size()).unwrap_or(0);

            // Split if too large
            if size > self.config.max_partition_size {
                self.split_partition(&partition_id)?;
            }
        }

        // Merge small partitions
        let small_partitions: Vec<String> = self.partitions
            .iter()
            .filter(|(_, p)| p.size() < self.config.min_partition_size)
            .map(|(id, _)| id.clone())
            .collect();

        for i in 0..small_partitions.len() / 2 {
            let source = &small_partitions[i * 2];
            let target = &small_partitions[i * 2 + 1];
            if self.partitions.contains_key(source) && self.partitions.contains_key(target) {
                self.merge_partitions(source, target)?;
            }
        }

        Ok(())
    }

    /// Calculate scale up count.
    fn calculate_scale_up(&self, metrics: &ScaleMetrics) -> usize {
        let pressure = metrics.queue_pressure();
        if pressure > 2.0 {
            // Add 10% of current agents
            ((self.total_agents() as f64 * 0.1).max(10.0)) as usize
        } else {
            0
        }
    }

    /// Calculate scale down count.
    fn calculate_scale_down(&self, metrics: &ScaleMetrics) -> usize {
        let current = self.total_agents();
        if current <= self.config.min_agents {
            return 0;
        }

        let utilization = metrics.utilization();
        if utilization < 0.4 {
            // Remove 10% of current agents
            let decrement = ((current as f64 * 0.1).max(10.0)) as usize;
            decrement.min(current - self.config.min_agents)
        } else {
            0
        }
    }

    /// Add agents to lightest partition.
    fn add_agents(&mut self, count: usize) -> Vec<AgentId> {
        let mut new_agents = Vec::with_capacity(count);

        for _ in 0..count {
            let partition_id = self.lightest_partition()
                .cloned()
                .unwrap_or_else(|| "P0".to_string());

            let agent_id = generate_queen_id(&format!("xl-Q{}", self.next_agent_number));
            self.next_agent_number += 1;

            if let Some(partition) = self.partitions.get_mut(&partition_id) {
                partition.add_agent(agent_id.clone());
                new_agents.push(agent_id);
            }
        }

        new_agents
    }

    /// Remove agents from heaviest partition.
    fn remove_agents(&mut self, count: usize) -> Vec<AgentId> {
        let mut removed = Vec::with_capacity(count);

        for _ in 0..count {
            let partition_id = self.heaviest_partition()
                .cloned();

            if let Some(pid) = partition_id {
                if let Some(partition) = self.partitions.get_mut(&pid) {
                    if partition.size() > 0 {
                        if let Some(agent_id) = partition.remove_agent(partition.size() - 1) {
                            removed.push(agent_id);
                        }
                    }
                }
            }
        }

        removed
    }
}

impl Scaling for VeryLargeScaleScaling {
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

                // Rebalance after adding
                self.rebalance_partitions()?;

                eprintln!("[VeryLargeScaleScaling] Scaled up by {} agents (total: {}, partitions: {})",
                    count, self.total_agents(), self.partitions.len());
                Ok(new_agents)
            }
            ScaleDecision::ScaleDown { count } => {
                let removed = self.remove_agents(count);

                // Rebalance after removing
                self.rebalance_partitions()?;

                eprintln!("[VeryLargeScaleScaling] Scaled down by {} agents (total: {}, partitions: {})",
                    count, self.total_agents(), self.partitions.len());
                Ok(removed)
            }
            ScaleDecision::NoAction => Ok(Vec::new()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

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
    fn test_partition_creation() {
        let mut partition = Partition::new("P0".to_string());
        partition.add_agent(generate_queen_id("agent-1"));
        partition.add_agent(generate_queen_id("agent-2"));

        assert_eq!(partition.size(), 2);
    }

    #[test]
    fn test_partition_neighbors() {
        let mut partition = Partition::new("P0".to_string());
        partition.add_neighbor("P1".to_string());
        partition.add_neighbor("P2".to_string());

        assert_eq!(partition.neighbors.len(), 2);
        assert!(partition.neighbors.contains("P1"));
    }

    #[test]
    fn test_split_partition() {
        let mut scaling = VeryLargeScaleScaling::default().unwrap();

        // Add agents to first partition
        scaling.execute_scale(ScaleDecision::ScaleUp { count: 150 }).unwrap();

        // Should have split into multiple partitions
        assert!(scaling.partitions.len() > 1);
    }

    #[test]
    fn test_lightest_partition() {
        let mut scaling = VeryLargeScaleScaling::default().unwrap();

        let p1 = Partition::new("P1".to_string());
        let mut p2 = Partition::new("P2".to_string());
        p2.load = 0.8;

        scaling.partitions.insert("P1".to_string(), p1);
        scaling.partitions.insert("P2".to_string(), p2);

        let lightest = scaling.lightest_partition().unwrap();
        assert_eq!(lightest, "P0"); // P0 has load 0.0
    }
}
