//! Ontology memory implementation.
//!
//! Knowledge graph with semantic relationships. Stores entities and their
//! connections using nodes and edges with typed relationships.

use super::Memory;
use crate::core::types::AgentId;
use anyhow::{Context, Result};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

/// Configuration for OntologyMemory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OntologyConfig {
    /// Maximum number of nodes in the graph.
    pub max_nodes: usize,
    /// Maximum number of edges in the graph.
    pub max_edges: usize,
}

impl Default for OntologyConfig {
    fn default() -> Self {
        OntologyConfig {
            max_nodes: 10000,
            max_edges: 50000,
        }
    }
}

/// Type of ontology node.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum NodeType {
    Entity,
    Concept,
    Action,
    Relation,
    Attribute,
}

/// A node in the knowledge graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OntologyNode {
    pub id: String,
    pub node_type: NodeType,
    pub properties: HashMap<String, Value>,
}

/// Type of relationship between nodes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum RelationType {
    IsA,
    HasA,
    DependsOn,
    RelatedTo,
    Produces,
    Consumes,
    Extends,
}

/// An edge connecting two nodes in the knowledge graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OntologyEdge {
    pub from: String,
    pub to: String,
    pub relation: RelationType,
    pub weight: f64,
}

/// Ontology memory as an in-memory knowledge graph.
pub struct OntologyMemory {
    config: OntologyConfig,
    nodes: Arc<RwLock<HashMap<String, OntologyNode>>>,
    // Adjacency list: from_node -> [(to_node, edge)]
    edges: Arc<RwLock<HashMap<String, Vec<OntologyEdge>>>>,
    // Reverse adjacency list for incoming edges: to_node -> from_nodes
    reverse_edges: Arc<RwLock<HashMap<String, Vec<String>>>>,
}

impl OntologyMemory {
    /// Create a new OntologyMemory with default configuration.
    pub fn new() -> Self {
        Self::with_config(OntologyConfig::default())
    }

    /// Create a new OntologyMemory with custom configuration.
    pub fn with_config(config: OntologyConfig) -> Self {
        OntologyMemory {
            config,
            nodes: Arc::new(RwLock::new(HashMap::new())),
            edges: Arc::new(RwLock::new(HashMap::new())),
            reverse_edges: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Add an edge connecting two nodes.
    pub fn add_edge(
        &mut self,
        from: String,
        to: String,
        relation: RelationType,
        weight: f64,
    ) -> Result<()> {
        // Verify both nodes exist
        {
            let nodes = self.nodes.read();
            if !nodes.contains_key(&from) {
                anyhow::bail!("Source node not found: {}", from);
            }
            if !nodes.contains_key(&to) {
                anyhow::bail!("Target node not found: {}", to);
            }
        }

        let mut edges = self.edges.write();
        let total_edges: usize = edges.values().map(|v| v.len()).sum();

        if total_edges >= self.config.max_edges {
            anyhow::bail!("Maximum edge limit reached: {}", self.config.max_edges);
        }

        let edge = OntologyEdge {
            from: from.clone(),
            to: to.clone(),
            relation,
            weight,
        };

        edges.entry(from.clone()).or_insert_with(Vec::new).push(edge);

        // Update reverse edges
        let mut reverse_edges = self.reverse_edges.write();
        reverse_edges
            .entry(to.clone())
            .or_insert_with(Vec::new)
            .push(from.clone());

        Ok(())
    }

    /// Get all neighbors of a node (outgoing edges).
    pub fn neighbors(&self, node_id: &str) -> Vec<(String, RelationType, f64)> {
        self.edges
            .read()
            .get(node_id)
            .map(|edges| {
                edges
                    .iter()
                    .map(|e| (e.to.clone(), e.relation.clone(), e.weight))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get all incoming edges to a node.
    pub fn incoming_neighbors(&self, node_id: &str) -> Vec<String> {
        self.reverse_edges
            .read()
            .get(node_id)
            .cloned()
            .unwrap_or_default()
    }

    /// Find shortest path between two nodes using BFS.
    pub fn shortest_path(&self, from: &str, to: &str) -> Option<Vec<String>> {
        if from == to {
            return Some(vec![from.to_string()]);
        }

        let edges = self.edges.read();
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        let mut parent: HashMap<String, String> = HashMap::new();

        queue.push_back(from.to_string());
        visited.insert(from.to_string());

        while let Some(current) = queue.pop_front() {
            if current == to {
                // Reconstruct path
                let mut path = vec![to.to_string()];
                let mut node = to;
                while let Some(p) = parent.get(node) {
                    path.push(p.clone());
                    node = p;
                }
                path.reverse();
                return Some(path);
            }

            if let Some(neighbors) = edges.get(&current) {
                for edge in neighbors {
                    if !visited.contains(&edge.to) {
                        visited.insert(edge.to.clone());
                        parent.insert(edge.to.clone(), current.clone());
                        queue.push_back(edge.to.clone());
                    }
                }
            }
        }

        None
    }

    /// Get all nodes of a specific type.
    pub fn get_nodes_by_type(&self, node_type: NodeType) -> Vec<OntologyNode> {
        self.nodes
            .read()
            .values()
            .filter(|n| n.node_type == node_type)
            .cloned()
            .collect()
    }

    /// Get all edges with a specific relation type.
    pub fn get_edges_by_relation(&self, relation: RelationType) -> Vec<OntologyEdge> {
        let edges = self.edges.read();
        edges
            .values()
            .flat_map(|edge_list| edge_list.iter().filter(|e| e.relation == relation).cloned())
            .collect()
    }

    /// Get node count.
    pub fn node_count(&self) -> usize {
        self.nodes.read().len()
    }

    /// Get edge count.
    pub fn edge_count(&self) -> usize {
        self.edges.read().values().map(|v| v.len()).sum()
    }

    /// Find nodes matching a property value.
    pub fn find_by_property(&self, property_key: &str, property_value: &Value) -> Vec<OntologyNode> {
        self.nodes
            .read()
            .values()
            .filter(|n| {
                n.properties
                    .get(property_key)
                    .map(|v| v == property_value)
                    .unwrap_or(false)
            })
            .cloned()
            .collect()
    }
}

impl Default for OntologyMemory {
    fn default() -> Self {
        Self::new()
    }
}

impl Memory for OntologyMemory {
    fn insert(&mut self, key: String, value: Value, _source: AgentId) -> Result<()> {
        // Parse node from value
        let obj = value.as_object().context("Node value must be an object")?;

        let node_type = if let Some(type_val) = obj.get("node_type") {
            serde_json::from_value(type_val.clone()).unwrap_or(NodeType::Concept)
        } else {
            NodeType::Concept
        };

        let properties = if let Some(props_val) = obj.get("properties") {
            serde_json::from_value(props_val.clone()).unwrap_or_default()
        } else {
            HashMap::new()
        };

        let node = OntologyNode {
            id: key.clone(),
            node_type,
            properties,
        };

        let mut nodes = self.nodes.write();

        if nodes.len() >= self.config.max_nodes && !nodes.contains_key(&key) {
            anyhow::bail!("Maximum node limit reached: {}", self.config.max_nodes);
        }

        nodes.insert(key, node);

        Ok(())
    }

    fn get(&self, key: &str) -> Option<Value> {
        self.nodes.read().get(key).and_then(|node| {
            serde_json::to_value(node).ok()
        })
    }

    fn query(&self, pattern: &str) -> Result<Vec<(String, Value)>> {
        let nodes = self.nodes.read();
        let mut results = Vec::new();
        let pattern_lower = pattern.to_lowercase();

        for (id, node) in nodes.iter() {
            // Search in node ID and properties
            let matches = id.to_lowercase().contains(&pattern_lower)
                || node.properties.values().any(|v| {
                    v.as_str()
                        .map(|s| s.to_lowercase().contains(&pattern_lower))
                        .unwrap_or(false)
                });

            if matches {
                if let Ok(value) = serde_json::to_value(node) {
                    results.push((id.clone(), value));
                }
            }
        }

        Ok(results)
    }

    fn evict(&mut self) -> Result<usize> {
        // Find orphan nodes (nodes with no incoming or outgoing edges)
        let nodes = self.nodes.read().clone();
        let edges = self.edges.read();
        let reverse_edges = self.reverse_edges.read();

        let mut orphans = Vec::new();

        for node_id in nodes.keys() {
            let has_outgoing = edges.get(node_id).map(|e| !e.is_empty()).unwrap_or(false);
            let has_incoming = reverse_edges
                .get(node_id)
                .map(|e| !e.is_empty())
                .unwrap_or(false);

            if !has_outgoing && !has_incoming {
                orphans.push(node_id.clone());
            }
        }

        drop(nodes);
        drop(edges);
        drop(reverse_edges);

        let count = orphans.len();

        if count > 0 {
            let mut nodes = self.nodes.write();
            for orphan in orphans {
                nodes.remove(&orphan);
            }
            eprintln!("[OntologyMemory] Evicted {} orphan nodes", count);
        }

        Ok(count)
    }

    fn snapshot(&self) -> Result<Value> {
        let mut snapshot = serde_json::Map::new();

        let nodes = self.nodes.read();
        snapshot.insert("nodes".to_string(), serde_json::to_value(&*nodes)?);

        let edges = self.edges.read();
        snapshot.insert("edges".to_string(), serde_json::to_value(&*edges)?);

        Ok(Value::Object(snapshot))
    }

    fn restore(&mut self, snapshot: Value) -> Result<()> {
        let obj = snapshot.as_object().context("Snapshot must be an object")?;

        if let Some(nodes_val) = obj.get("nodes") {
            let restored_nodes: HashMap<String, OntologyNode> =
                serde_json::from_value(nodes_val.clone())?;
            let mut nodes = self.nodes.write();
            *nodes = restored_nodes;
        }

        if let Some(edges_val) = obj.get("edges") {
            let restored_edges: HashMap<String, Vec<OntologyEdge>> =
                serde_json::from_value(edges_val.clone())?;

            let mut edges = self.edges.write();
            *edges = restored_edges.clone();

            // Rebuild reverse edges
            let mut reverse_edges = self.reverse_edges.write();
            reverse_edges.clear();

            for edge_list in restored_edges.values() {
                for edge in edge_list {
                    reverse_edges
                        .entry(edge.to.clone())
                        .or_insert_with(Vec::new)
                        .push(edge.from.clone());
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::types::QueenId;

    #[test]
    fn test_add_node() {
        let mut mem = OntologyMemory::new();
        let agent = AgentId::Queen(QueenId("Q0".to_string()));

        let node = serde_json::json!({
            "node_type": "Entity",
            "properties": {"name": "TestEntity"}
        });

        mem.insert("entity1".to_string(), node, agent).unwrap();
        assert_eq!(mem.node_count(), 1);
    }

    #[test]
    fn test_add_edge() {
        let mut mem = OntologyMemory::new();
        let agent = AgentId::Operator;

        let node1 = serde_json::json!({"node_type": "Entity", "properties": {}});
        let node2 = serde_json::json!({"node_type": "Entity", "properties": {}});

        mem.insert("node1".to_string(), node1, agent.clone()).unwrap();
        mem.insert("node2".to_string(), node2, agent).unwrap();

        mem.add_edge(
            "node1".to_string(),
            "node2".to_string(),
            RelationType::DependsOn,
            1.0,
        )
        .unwrap();

        assert_eq!(mem.edge_count(), 1);
    }

    #[test]
    fn test_neighbors() {
        let mut mem = OntologyMemory::new();
        let agent = AgentId::Validator;

        mem.insert("a".to_string(), serde_json::json!({"node_type": "Entity", "properties": {}}), agent.clone())
            .unwrap();
        mem.insert("b".to_string(), serde_json::json!({"node_type": "Entity", "properties": {}}), agent.clone())
            .unwrap();
        mem.insert("c".to_string(), serde_json::json!({"node_type": "Entity", "properties": {}}), agent)
            .unwrap();

        mem.add_edge("a".to_string(), "b".to_string(), RelationType::IsA, 1.0)
            .unwrap();
        mem.add_edge("a".to_string(), "c".to_string(), RelationType::HasA, 1.0)
            .unwrap();

        let neighbors = mem.neighbors("a");
        assert_eq!(neighbors.len(), 2);
    }

    #[test]
    fn test_shortest_path() {
        let mut mem = OntologyMemory::new();
        let agent = AgentId::Operator;

        for id in &["a", "b", "c"] {
            mem.insert(
                id.to_string(),
                serde_json::json!({"node_type": "Entity", "properties": {}}),
                agent.clone(),
            )
            .unwrap();
        }

        mem.add_edge("a".to_string(), "b".to_string(), RelationType::RelatedTo, 1.0)
            .unwrap();
        mem.add_edge("b".to_string(), "c".to_string(), RelationType::RelatedTo, 1.0)
            .unwrap();

        let path = mem.shortest_path("a", "c");
        assert_eq!(path, Some(vec!["a".to_string(), "b".to_string(), "c".to_string()]));
    }
}
