//! RAG (Retrieval-Augmented Generation) memory implementation.
//!
//! Document storage with TF-IDF based similarity search. No external embeddings
//! or API calls - uses in-memory TF-IDF for semantic retrieval.

use super::Memory;
use crate::core::types::AgentId;
use anyhow::{Context, Result};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// Configuration for RagMemory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RagConfig {
    /// Minimum similarity threshold for retrieval (0.0 to 1.0).
    pub similarity_threshold: f64,
    /// Maximum number of results to return.
    pub max_results: usize,
}

impl Default for RagConfig {
    fn default() -> Self {
        RagConfig {
            similarity_threshold: 0.1,
            max_results: 10,
        }
    }
}

/// A document with TF-IDF vectors for similarity search.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RagDocument {
    pub id: String,
    pub content: String,
    pub tokens: Vec<String>,
    pub tf_idf: HashMap<String, f64>,
    pub metadata: HashMap<String, Value>,
    #[serde(skip)]
    access_count: u64,
}

/// RAG memory with TF-IDF based retrieval.
pub struct RagMemory {
    config: RagConfig,
    documents: Arc<RwLock<HashMap<String, RagDocument>>>,
    // Inverted index: term -> [doc_ids]
    inverted_index: Arc<RwLock<HashMap<String, HashSet<String>>>>,
    // IDF cache: term -> idf value
    idf_cache: Arc<RwLock<HashMap<String, f64>>>,
    // Document count for IDF calculation
    total_docs: Arc<RwLock<usize>>,
}

impl RagMemory {
    /// Create a new RagMemory with default configuration.
    pub fn new() -> Self {
        Self::with_config(RagConfig::default())
    }

    /// Create a new RagMemory with custom configuration.
    pub fn with_config(config: RagConfig) -> Self {
        RagMemory {
            config,
            documents: Arc::new(RwLock::new(HashMap::new())),
            inverted_index: Arc::new(RwLock::new(HashMap::new())),
            idf_cache: Arc::new(RwLock::new(HashMap::new())),
            total_docs: Arc::new(RwLock::new(0)),
        }
    }

    /// Get document count.
    pub fn document_count(&self) -> usize {
        self.documents.read().len()
    }

    /// Get document by ID.
    pub fn get_document(&self, id: &str) -> Option<RagDocument> {
        self.documents.read().get(id).cloned()
    }

    /// Tokenize text into lowercase words, removing punctuation and stopwords.
    fn tokenize(text: &str) -> Vec<String> {
        let stopwords: HashSet<&str> = [
            "a", "an", "and", "are", "as", "at", "be", "but", "by", "for", "if", "in", "into",
            "is", "it", "no", "not", "of", "on", "or", "such", "that", "the", "their", "then",
            "there", "these", "they", "this", "to", "was", "will", "with",
        ]
        .iter()
        .copied()
        .collect();

        text.to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .filter_map(|word| {
                let w = word.trim();
                if !w.is_empty() && !stopwords.contains(w) && w.len() > 2 {
                    Some(w.to_string())
                } else {
                    None
                }
            })
            .collect()
    }

    /// Compute term frequency for a document.
    fn compute_tf(tokens: &[String]) -> HashMap<String, f64> {
        let mut tf = HashMap::new();
        let total = tokens.len() as f64;

        for token in tokens {
            *tf.entry(token.clone()).or_insert(0.0) += 1.0;
        }

        // Normalize by document length
        for count in tf.values_mut() {
            *count /= total;
        }

        tf
    }

    /// Compute inverse document frequency.
    fn compute_idf(&self, term: &str) -> f64 {
        let inverted_index = self.inverted_index.read();
        let total_docs = *self.total_docs.read() as f64;

        if total_docs == 0.0 {
            return 0.0;
        }

        let doc_count = inverted_index
            .get(term)
            .map(|docs| docs.len() as f64)
            .unwrap_or(0.0);

        if doc_count == 0.0 {
            0.0
        } else {
            ((total_docs + 1.0) / (doc_count + 1.0)).ln()
        }
    }

    /// Compute TF-IDF vector for a document.
    fn compute_tf_idf(&self, tf: HashMap<String, f64>) -> HashMap<String, f64> {
        let mut tf_idf = HashMap::new();

        for (term, tf_value) in tf {
            let idf = {
                let cache = self.idf_cache.read();
                cache.get(&term).copied().unwrap_or_else(|| {
                    drop(cache);
                    self.compute_idf(&term)
                })
            };
            tf_idf.insert(term, tf_value * idf);
        }

        tf_idf
    }

    /// Calculate cosine similarity between two TF-IDF vectors.
    fn cosine_similarity(vec1: &HashMap<String, f64>, vec2: &HashMap<String, f64>) -> f64 {
        let mut dot_product = 0.0;
        let mut norm1 = 0.0;
        let mut norm2 = 0.0;

        for (term, val1) in vec1 {
            dot_product += val1 * vec2.get(term).unwrap_or(&0.0);
            norm1 += val1 * val1;
        }

        for val2 in vec2.values() {
            norm2 += val2 * val2;
        }

        if norm1 == 0.0 || norm2 == 0.0 {
            0.0
        } else {
            dot_product / (norm1.sqrt() * norm2.sqrt())
        }
    }

    /// Search for similar documents using TF-IDF cosine similarity.
    pub fn search(&self, query: &str) -> Vec<(String, f64, String)> {
        let query_tokens = Self::tokenize(query);
        let query_tf = Self::compute_tf(&query_tokens);
        let query_tf_idf = self.compute_tf_idf(query_tf);

        let documents = self.documents.read();
        let mut results = Vec::new();

        for (id, doc) in documents.iter() {
            let similarity = Self::cosine_similarity(&query_tf_idf, &doc.tf_idf);

            if similarity >= self.config.similarity_threshold {
                results.push((id.clone(), similarity, doc.content.clone()));
            }
        }

        // Sort by similarity (descending)
        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        // Limit results
        results.truncate(self.config.max_results);

        results
    }

    /// Rebuild IDF cache for all terms.
    fn rebuild_idf_cache(&self) {
        let inverted_index = self.inverted_index.read();
        let total_docs = *self.total_docs.read() as f64;
        let mut idf_cache = self.idf_cache.write();

        idf_cache.clear();

        for term in inverted_index.keys() {
            let doc_count = inverted_index.get(term).unwrap().len() as f64;
            let idf = ((total_docs + 1.0) / (doc_count + 1.0)).ln();
            idf_cache.insert(term.clone(), idf);
        }
    }

    /// Update inverted index with document tokens.
    fn update_inverted_index(&self, doc_id: &str, tokens: &[String]) {
        let mut inverted_index = self.inverted_index.write();

        for token in tokens {
            inverted_index
                .entry(token.clone())
                .or_insert_with(HashSet::new)
                .insert(doc_id.to_string());
        }
    }

    /// Remove document from inverted index.
    fn remove_from_inverted_index(&self, doc_id: &str, tokens: &[String]) {
        let mut inverted_index = self.inverted_index.write();

        for token in tokens {
            if let Some(doc_set) = inverted_index.get_mut(token) {
                doc_set.remove(doc_id);
                if doc_set.is_empty() {
                    inverted_index.remove(token);
                }
            }
        }
    }
}

impl Default for RagMemory {
    fn default() -> Self {
        Self::new()
    }
}

impl Memory for RagMemory {
    fn insert(&mut self, key: String, value: Value, _source: AgentId) -> Result<()> {
        // Extract content and metadata from value
        let (content, metadata) = if let Some(obj) = value.as_object() {
            let content = obj
                .get("content")
                .and_then(|v| v.as_str())
                .context("Document must have 'content' field")?
                .to_string();

            let metadata = obj
                .get("metadata")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default();

            (content, metadata)
        } else if let Some(s) = value.as_str() {
            (s.to_string(), HashMap::new())
        } else {
            anyhow::bail!("Invalid document format");
        };

        // Tokenize content
        let tokens = Self::tokenize(&content);

        // Compute TF
        let tf = Self::compute_tf(&tokens);

        // Compute TF-IDF
        let tf_idf = self.compute_tf_idf(tf);

        // Create document
        let doc = RagDocument {
            id: key.clone(),
            content,
            tokens: tokens.clone(),
            tf_idf,
            metadata,
            access_count: 0,
        };

        // Update inverted index
        self.update_inverted_index(&key, &tokens);

        // Store document
        let mut documents = self.documents.write();
        let is_new = !documents.contains_key(&key);

        if is_new {
            *self.total_docs.write() += 1;
        } else {
            // Remove old tokens from inverted index
            if let Some(old_doc) = documents.get(&key) {
                self.remove_from_inverted_index(&key, &old_doc.tokens);
            }
        }

        documents.insert(key, doc);

        // Rebuild IDF cache
        drop(documents);
        self.rebuild_idf_cache();

        Ok(())
    }

    fn get(&self, key: &str) -> Option<Value> {
        let mut documents = self.documents.write();
        documents.get_mut(key).map(|doc| {
            doc.access_count += 1;
            serde_json::json!({
                "id": doc.id,
                "content": doc.content,
                "metadata": doc.metadata,
                "access_count": doc.access_count
            })
        })
    }

    fn query(&self, pattern: &str) -> Result<Vec<(String, Value)>> {
        let results = self.search(pattern);
        let mut output = Vec::new();

        for (id, similarity, content) in results {
            let value = serde_json::json!({
                "id": id,
                "content": content,
                "similarity": similarity
            });
            output.push((id, value));
        }

        Ok(output)
    }

    fn evict(&mut self) -> Result<usize> {
        // Evict documents with lowest access count
        let documents = self.documents.read();
        let mut sorted: Vec<_> = documents.iter().collect();
        sorted.sort_by_key(|(_, doc)| doc.access_count);

        let to_evict_count = documents.len() / 10; // Evict bottom 10%
        let to_evict: Vec<(String, Vec<String>)> = sorted
            .iter()
            .take(to_evict_count)
            .map(|(id, doc)| (id.to_string(), doc.tokens.clone()))
            .collect();

        drop(documents);

        if to_evict.is_empty() {
            return Ok(0);
        }

        let mut documents = self.documents.write();
        for (id, tokens) in &to_evict {
            documents.remove(id);
            self.remove_from_inverted_index(id, tokens);
        }

        let evicted = to_evict.len();
        *self.total_docs.write() -= evicted;

        drop(documents);
        self.rebuild_idf_cache();

        Ok(evicted)
    }

    fn snapshot(&self) -> Result<Value> {
        let documents = self.documents.read();
        serde_json::to_value(&*documents).context("Failed to serialize RAG snapshot")
    }

    fn restore(&mut self, snapshot: Value) -> Result<()> {
        let restored: HashMap<String, RagDocument> =
            serde_json::from_value(snapshot).context("Failed to deserialize RAG snapshot")?;

        // Clear current state
        self.documents.write().clear();
        self.inverted_index.write().clear();
        self.idf_cache.write().clear();
        *self.total_docs.write() = 0;

        // Restore documents and rebuild indices
        for (id, doc) in restored {
            self.update_inverted_index(&id, &doc.tokens);
            self.documents.write().insert(id, doc);
            *self.total_docs.write() += 1;
        }

        self.rebuild_idf_cache();

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::types::QueenId;

    #[test]
    fn test_tokenize() {
        let tokens = RagMemory::tokenize("The quick brown fox jumps over the lazy dog");
        assert!(tokens.contains(&"quick".to_string()));
        assert!(tokens.contains(&"brown".to_string()));
        assert!(!tokens.contains(&"the".to_string())); // Stopword
    }

    #[test]
    fn test_insert_and_search() {
        let mut mem = RagMemory::new();
        let agent = AgentId::Queen(QueenId("Q0".to_string()));

        let doc1 = serde_json::json!({
            "content": "Rust is a systems programming language focused on safety and performance.",
            "metadata": {"category": "programming"}
        });

        let doc2 = serde_json::json!({
            "content": "Python is a high-level programming language used for data science.",
            "metadata": {"category": "programming"}
        });

        mem.insert("doc1".to_string(), doc1, agent.clone()).unwrap();
        mem.insert("doc2".to_string(), doc2, agent).unwrap();

        let results = mem.search("systems programming safety");
        assert!(!results.is_empty());
        assert_eq!(results[0].0, "doc1");
    }

    #[test]
    fn test_similarity_threshold() {
        let mut mem = RagMemory::with_config(RagConfig {
            similarity_threshold: 0.5,
            max_results: 10,
        });
        let agent = AgentId::Operator;

        mem.insert(
            "doc1".to_string(),
            serde_json::json!({"content": "machine learning deep neural networks"}),
            agent.clone(),
        )
        .unwrap();

        mem.insert(
            "doc2".to_string(),
            serde_json::json!({"content": "database optimization query performance"}),
            agent,
        )
        .unwrap();

        let results = mem.search("machine learning");
        // Should only return doc1 due to high threshold
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_document_count() {
        let mut mem = RagMemory::new();
        let agent = AgentId::Validator;

        mem.insert("d1".to_string(), serde_json::json!({"content": "test"}), agent.clone())
            .unwrap();
        mem.insert("d2".to_string(), serde_json::json!({"content": "test"}), agent)
            .unwrap();

        assert_eq!(mem.document_count(), 2);
    }
}
