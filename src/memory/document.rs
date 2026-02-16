//! Document memory implementation.
//!
//! Stores design documents, PRDs, architecture notes, API specs, and other
//! structured documentation with versioning.

use super::Memory;
use crate::core::types::AgentId;
use anyhow::{Context, Result};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

/// Configuration for DocumentMemory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentConfig {
    /// Optional directory for persisting artifacts to disk.
    pub artifacts_dir: Option<PathBuf>,
}

impl Default for DocumentConfig {
    fn default() -> Self {
        DocumentConfig {
            artifacts_dir: None,
        }
    }
}

/// Type of document stored.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DocType {
    Prd,
    DesignDoc,
    ArchitectureNote,
    ApiSpec,
    TestPlan,
    Changelog,
    Custom(String),
}

/// A document entry with metadata and versioning.
#[derive(Debug, Clone)]
struct DocumentEntry {
    title: String,
    doc_type: DocType,
    content: String,
    version: u32,
    created_at: Instant,
    updated_at: Instant,
    author: String,
    tags: Vec<String>,
}

/// Serializable document entry for snapshots.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SerializableDocument {
    title: String,
    doc_type: DocType,
    content: String,
    version: u32,
    created_secs: u64,
    updated_secs: u64,
    author: String,
    tags: Vec<String>,
}

/// Document memory for storing structured documentation.
pub struct DocumentMemory {
    config: DocumentConfig,
    documents: Arc<RwLock<HashMap<String, DocumentEntry>>>,
    start_time: Instant,
}

impl DocumentMemory {
    /// Create a new DocumentMemory with default configuration.
    pub fn new() -> Self {
        Self::with_config(DocumentConfig::default())
    }

    /// Create a new DocumentMemory with custom configuration.
    pub fn with_config(config: DocumentConfig) -> Self {
        DocumentMemory {
            config,
            documents: Arc::new(RwLock::new(HashMap::new())),
            start_time: Instant::now(),
        }
    }

    /// List all documents with metadata (key, title, type, version).
    pub fn list_documents(&self) -> Vec<(String, String, DocType, u32)> {
        self.documents
            .read()
            .iter()
            .map(|(key, doc)| (key.clone(), doc.title.clone(), doc.doc_type.clone(), doc.version))
            .collect()
    }

    /// Get document metadata without content.
    pub fn get_metadata(&self, key: &str) -> Option<(String, DocType, u32, String)> {
        self.documents
            .read()
            .get(key)
            .map(|doc| (doc.title.clone(), doc.doc_type.clone(), doc.version, doc.author.clone()))
    }

    /// Get documents by type.
    pub fn get_by_type(&self, doc_type: DocType) -> Vec<(String, String, String)> {
        self.documents
            .read()
            .iter()
            .filter(|(_, doc)| doc.doc_type == doc_type)
            .map(|(key, doc)| (key.clone(), doc.title.clone(), doc.content.clone()))
            .collect()
    }

    /// Search documents by tag.
    pub fn search_by_tag(&self, tag: &str) -> Vec<(String, String, String)> {
        self.documents
            .read()
            .iter()
            .filter(|(_, doc)| doc.tags.iter().any(|t| t.eq_ignore_ascii_case(tag)))
            .map(|(key, doc)| (key.clone(), doc.title.clone(), doc.content.clone()))
            .collect()
    }

    /// Update a document's content (increments version).
    pub fn update_document(&mut self, key: &str, content: String, author: AgentId) -> Result<u32> {
        let author_str = Self::agent_id_to_string(&author);
        let mut documents = self.documents.write();

        if let Some(doc) = documents.get_mut(key) {
            doc.content = content;
            doc.version += 1;
            doc.updated_at = Instant::now();
            doc.author = author_str;
            Ok(doc.version)
        } else {
            anyhow::bail!("Document not found: {}", key);
        }
    }

    /// Add tags to a document.
    pub fn add_tags(&mut self, key: &str, tags: Vec<String>) -> Result<()> {
        let mut documents = self.documents.write();
        if let Some(doc) = documents.get_mut(key) {
            for tag in tags {
                if !doc.tags.contains(&tag) {
                    doc.tags.push(tag);
                }
            }
            Ok(())
        } else {
            anyhow::bail!("Document not found: {}", key);
        }
    }

    /// Convert AgentId to string.
    fn agent_id_to_string(agent: &AgentId) -> String {
        match agent {
            AgentId::Nydus(id) => format!("Nydus({})", id.0),
            AgentId::Queen(id) => format!("Queen({})", id.0),
            AgentId::Overlord(id) => format!("Overlord({})", id.0),
            AgentId::Overmind(id) => format!("Overmind({})", id.0),
            AgentId::Validator => "Validator".to_string(),
            AgentId::Operator => "Operator".to_string(),
        }
    }

    /// Parse document value from JSON.
    fn parse_document_value(value: &Value) -> Result<(String, DocType, String, Vec<String>)> {
        let obj = value.as_object().context("Document value must be an object")?;

        let title = obj
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("Untitled")
            .to_string();

        let doc_type = if let Some(type_val) = obj.get("doc_type") {
            serde_json::from_value(type_val.clone()).unwrap_or(DocType::Custom("unknown".to_string()))
        } else {
            DocType::Custom("unknown".to_string())
        };

        let content = obj
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let tags = if let Some(tags_val) = obj.get("tags") {
            serde_json::from_value(tags_val.clone()).unwrap_or_default()
        } else {
            Vec::new()
        };

        Ok((title, doc_type, content, tags))
    }
}

impl Default for DocumentMemory {
    fn default() -> Self {
        Self::new()
    }
}

impl Memory for DocumentMemory {
    fn insert(&mut self, key: String, value: Value, source: AgentId) -> Result<()> {
        let author = Self::agent_id_to_string(&source);
        let now = Instant::now();

        let (title, doc_type, content, tags) = Self::parse_document_value(&value)?;

        let mut documents = self.documents.write();

        if let Some(doc) = documents.get_mut(&key) {
            // Update existing document
            doc.content = content;
            doc.version += 1;
            doc.updated_at = now;
            doc.author = author;
            // Merge tags
            for tag in tags {
                if !doc.tags.contains(&tag) {
                    doc.tags.push(tag);
                }
            }
        } else {
            // Create new document
            documents.insert(
                key,
                DocumentEntry {
                    title,
                    doc_type,
                    content,
                    version: 1,
                    created_at: now,
                    updated_at: now,
                    author,
                    tags,
                },
            );
        }

        Ok(())
    }

    fn get(&self, key: &str) -> Option<Value> {
        self.documents.read().get(key).map(|doc| {
            serde_json::json!({
                "title": doc.title,
                "doc_type": doc.doc_type,
                "content": doc.content,
                "version": doc.version,
                "author": doc.author,
                "tags": doc.tags
            })
        })
    }

    fn query(&self, pattern: &str) -> Result<Vec<(String, Value)>> {
        let documents = self.documents.read();
        let mut results = Vec::new();
        let pattern_lower = pattern.to_lowercase();

        for (key, doc) in documents.iter() {
            // Search in key, title, and content
            let matches = key.to_lowercase().contains(&pattern_lower)
                || doc.title.to_lowercase().contains(&pattern_lower)
                || doc.content.to_lowercase().contains(&pattern_lower)
                || doc.tags.iter().any(|tag| tag.to_lowercase().contains(&pattern_lower));

            if matches {
                let value = serde_json::json!({
                    "title": doc.title,
                    "doc_type": doc.doc_type,
                    "content": doc.content,
                    "version": doc.version,
                    "author": doc.author,
                    "tags": doc.tags
                });
                results.push((key.clone(), value));
            }
        }

        Ok(results)
    }

    fn evict(&mut self) -> Result<usize> {
        // Documents are persistent - no automatic eviction
        Ok(0)
    }

    fn snapshot(&self) -> Result<Value> {
        let documents = self.documents.read();
        let snapshot: HashMap<String, SerializableDocument> = documents
            .iter()
            .map(|(key, doc)| {
                (
                    key.clone(),
                    SerializableDocument {
                        title: doc.title.clone(),
                        doc_type: doc.doc_type.clone(),
                        content: doc.content.clone(),
                        version: doc.version,
                        created_secs: doc.created_at.duration_since(self.start_time).as_secs(),
                        updated_secs: doc.updated_at.duration_since(self.start_time).as_secs(),
                        author: doc.author.clone(),
                        tags: doc.tags.clone(),
                    },
                )
            })
            .collect();

        serde_json::to_value(&snapshot).context("Failed to serialize document snapshot")
    }

    fn restore(&mut self, snapshot: Value) -> Result<()> {
        let snapshot_map: HashMap<String, SerializableDocument> =
            serde_json::from_value(snapshot).context("Failed to deserialize document snapshot")?;

        let mut documents = self.documents.write();
        documents.clear();

        for (key, ser_doc) in snapshot_map {
            let created_at = self.start_time + std::time::Duration::from_secs(ser_doc.created_secs);
            let updated_at = self.start_time + std::time::Duration::from_secs(ser_doc.updated_secs);

            documents.insert(
                key,
                DocumentEntry {
                    title: ser_doc.title,
                    doc_type: ser_doc.doc_type,
                    content: ser_doc.content,
                    version: ser_doc.version,
                    created_at,
                    updated_at,
                    author: ser_doc.author,
                    tags: ser_doc.tags,
                },
            );
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::types::QueenId;

    #[test]
    fn test_document_insert_and_get() {
        let mut mem = DocumentMemory::new();
        let agent = AgentId::Queen(QueenId("Q0".to_string()));

        let doc = serde_json::json!({
            "title": "Test PRD",
            "doc_type": "Prd",
            "content": "This is a test PRD document.",
            "tags": ["test", "prd"]
        });

        mem.insert("prd:main".to_string(), doc, agent).unwrap();

        let result = mem.get("prd:main");
        assert!(result.is_some());

        let retrieved = result.unwrap();
        assert_eq!(retrieved["title"], "Test PRD");
        assert_eq!(retrieved["version"], 1);
    }

    #[test]
    fn test_document_versioning() {
        let mut mem = DocumentMemory::new();
        let agent = AgentId::Operator;

        let doc = serde_json::json!({
            "title": "Design Doc",
            "doc_type": "DesignDoc",
            "content": "v1",
            "tags": []
        });

        mem.insert("design:topology".to_string(), doc, agent.clone())
            .unwrap();

        let new_version = mem
            .update_document("design:topology", "v2 content".to_string(), agent)
            .unwrap();

        assert_eq!(new_version, 2);
    }

    #[test]
    fn test_list_documents() {
        let mut mem = DocumentMemory::new();
        let agent = AgentId::Validator;

        mem.insert(
            "prd:1".to_string(),
            serde_json::json!({"title": "PRD 1", "doc_type": "Prd", "content": "test", "tags": []}),
            agent.clone(),
        )
        .unwrap();

        mem.insert(
            "design:1".to_string(),
            serde_json::json!({"title": "Design 1", "doc_type": "DesignDoc", "content": "test", "tags": []}),
            agent,
        )
        .unwrap();

        let docs = mem.list_documents();
        assert_eq!(docs.len(), 2);
    }

    #[test]
    fn test_search_by_tag() {
        let mut mem = DocumentMemory::new();
        let agent = AgentId::Operator;

        mem.insert(
            "doc1".to_string(),
            serde_json::json!({"title": "Doc 1", "doc_type": "Custom", "content": "test", "tags": ["important"]}),
            agent.clone(),
        )
        .unwrap();

        mem.insert(
            "doc2".to_string(),
            serde_json::json!({"title": "Doc 2", "doc_type": "Custom", "content": "test", "tags": ["draft"]}),
            agent,
        )
        .unwrap();

        let results = mem.search_by_tag("important");
        assert_eq!(results.len(), 1);
    }
}
