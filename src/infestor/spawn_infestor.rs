//! SpawnInfestor: Infestor as a StreamQueen with reviewer system prompt.
//!
//! The Infestor is a long-lived Claude Code process (StreamQueen) that reviews
//! completed Queen work before merge. It receives review tasks via normal task
//! assignment and returns results via TaskCompleted/TaskFailed events.

use crate::core::types::{QueenId, InfestorId};
use crate::queen::stream_queen::{self, StreamQueenConfig};
use crate::queen::handle::QueenEvent;
use crate::queen::completion::CompletionConfig;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{mpsc, Notify};
use anyhow::Result;

/// Configuration for Infestor.
#[derive(Debug, Clone)]
pub struct InfestorConfig {
    pub id: InfestorId,
    pub model: String,
    pub working_dir: PathBuf,
    pub wakeup_notify: Option<Arc<Notify>>,
    pub swarm_id: Option<String>,
    pub ipc_port: Option<u16>,
    pub setting_sources: Option<String>,
}

impl Default for InfestorConfig {
    fn default() -> Self {
        Self {
            id: InfestorId("infestor-0".to_string()),
            model: "sonnet".to_string(),
            working_dir: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            wakeup_notify: None,
            swarm_id: None,
            ipc_port: None,
            setting_sources: None,
        }
    }
}

/// Spawn an Infestor as a StreamQueen with reviewer system prompt.
///
/// Returns InfestorHandle, event receiver, and join handle.
///
/// # Errors
/// Returns an error if the StreamQueen fails to spawn.
pub fn spawn_infestor(
    config: InfestorConfig,
    shutdown_rx: tokio::sync::broadcast::Receiver<()>,
) -> Result<(crate::infestor::handle::InfestorHandle, mpsc::Receiver<QueenEvent>, tokio::task::JoinHandle<()>)> {
    let queen_id = QueenId(config.id.0.clone());

    // Build reviewer system prompt
    let system_prompt = Some(format!(
        "{}\n\n{}",
        crate::core::prompts::default_system_prompt("infestor"),
        crate::core::prompts::infestor_system_prompt(),
    ));

    let stream_config = StreamQueenConfig {
        id: queen_id.clone(),
        model: config.model,
        working_dir: config.working_dir,
        max_turns: Some(15), // Enough turns for diff review, cargo check, and verdict
        max_budget_usd: Some(0.50), // Cap review cost
        system_prompt,
        allowed_tools: Some("Read,Glob,Grep,Bash".to_string()), // Read-only + bash for cargo check
        completion: CompletionConfig::default(),
        swarm_id: config.swarm_id,
        ipc_port: config.ipc_port,
        setting_sources: config.setting_sources,
        wakeup_notify: config.wakeup_notify,
    };

    let (event_tx, event_rx) = mpsc::channel(64);
    let (queen_handle, join_handle) = stream_queen::spawn(stream_config, event_tx, shutdown_rx)?;

    let infestor_handle = crate::infestor::handle::InfestorHandle::from_queen_handle(config.id, queen_handle);

    Ok((infestor_handle, event_rx, join_handle))
}
