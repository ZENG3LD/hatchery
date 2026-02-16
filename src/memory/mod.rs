//! Memory subsystem for Hatchery orchestration.
//!
//! Provides multiple memory implementations for different use cases:
//! - `SharedStateMemory`: Simple key-value store with versioning
//! - `ConversationMemory`: Tracks conversation history per agent with auto-summarization
//! - `MultiTierMemory`: Four-tier memory (short-term, long-term, entity, contextual)
//! - `DocumentMemory`: Stores design documents, PRDs, architecture notes
//! - `IsolatedMemory`: Per-agent isolated namespaced storage
//! - `SessionMemory`: Tracks agent sessions for recovery and persistence
//! - `CollaborativeMemory`: Dual-tier memory with access control
//! - `OntologyMemory`: Knowledge graph with semantic relationships
//! - `RagMemory`: Retrieval-augmented memory with TF-IDF similarity

use crate::core::types::AgentId;
use anyhow::Result;
use serde_json::Value;

/// Memory trait defines how state and knowledge are stored and retrieved.
pub trait Memory: Send + Sync {
    /// Insert or update a value in memory.
    fn insert(&mut self, key: String, value: Value, source: AgentId) -> Result<()>;

    /// Retrieve a value by key.
    fn get(&self, key: &str) -> Option<Value>;

    /// Query memory using a pattern (supports wildcards).
    fn query(&self, pattern: &str) -> Result<Vec<(String, Value)>>;

    /// Evict old or unused entries (implementation-specific).
    /// Returns the number of entries evicted.
    fn evict(&mut self) -> Result<usize>;

    /// Create a snapshot of the entire memory state.
    fn snapshot(&self) -> Result<Value>;

    /// Restore memory state from a snapshot.
    fn restore(&mut self, snapshot: Value) -> Result<()>;
}

// Re-export all implementations
pub mod shared_state;
pub use shared_state::{SharedStateConfig, SharedStateMemory};

pub mod conversation;
pub use conversation::{ConversationConfig, ConversationMemory};

pub mod multi_tier;
pub use multi_tier::{MultiTierConfig, MultiTierMemory};

pub mod document;
pub use document::{DocumentConfig, DocumentMemory};

pub mod isolated;
pub use isolated::{IsolatedConfig, IsolatedMemory};

pub mod session;
pub use session::{SessionConfig, SessionMemory};

pub mod collaborative;
pub use collaborative::{CollaborativeConfig, CollaborativeMemory};

pub mod ontology;
pub use ontology::{OntologyConfig, OntologyMemory};

pub mod rag;
pub use rag::{RagConfig, RagMemory};
