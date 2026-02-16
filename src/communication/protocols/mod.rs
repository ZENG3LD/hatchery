//! Protocol adapters for agent communication.
//!
//! This module provides implementations of various agent communication protocols:
//! - MCP: Model Context Protocol (local version)
//! - A2A: Agent-to-Agent protocol with agent cards
//! - ACP: Agent Communication Protocol (capability-based routing)
//! - ANP: Agent Network Protocol (network topology awareness)

pub mod a2a;
pub mod acp;
pub mod anp;
pub mod mcp;

pub use a2a::{A2aAgentCard, A2aCommunication, A2aConfig};
pub use acp::{AcpCommunication, AcpConfig};
pub use anp::{AnpCommunication, AnpConfig};
pub use mcp::{McpCommunication, McpConfig, McpTool};
