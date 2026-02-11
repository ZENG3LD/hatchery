//! Error types for infestation_pit (Claude Code session parsing)

use thiserror::Error;

/// Error type for Claude Code session parsing
#[derive(Debug, Error)]
pub enum ParseError {
    /// I/O operation failed
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON serialization/deserialization failed
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// Invalid session data
    #[error("Invalid session data: {0}")]
    InvalidSession(String),

    /// Generic error
    #[error("{0}")]
    Other(String),
}

/// Result type alias for parsing operations
pub type Result<T> = std::result::Result<T, ParseError>;
