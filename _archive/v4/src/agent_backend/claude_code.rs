//! Claude Code agent backend — spawns Claude Code subprocesses via NDJSON pipe.
//!
//! This wraps the existing Queen/StreamQueen implementation as an AgentBackend.

use super::{AgentBackend, AgentEvent, AgentSpawnConfig};
use crate::core::types::{AgentId, QueenId, Task, TaskContext, TaskId};
use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;
use tokio::sync::mpsc;

/// Claude Code agent backend using NDJSON pipe protocol.
pub struct ClaudeCodeBackend {
    agents: HashMap<String, AgentState>,
    event_rx: mpsc::Receiver<AgentEvent>,
    event_tx: mpsc::Sender<AgentEvent>,
    next_id: u32,
}

struct AgentState {
    id: AgentId,
    working_dir: std::path::PathBuf,
    is_alive: bool,
}

impl ClaudeCodeBackend {
    pub fn new() -> Self {
        let (event_tx, event_rx) = mpsc::channel(256);
        Self {
            agents: HashMap::new(),
            event_rx,
            event_tx,
            next_id: 0,
        }
    }
}

#[async_trait]
impl AgentBackend for ClaudeCodeBackend {
    async fn spawn(&mut self, config: AgentSpawnConfig) -> Result<AgentId> {
        let id = self.next_id;
        self.next_id += 1;
        let agent_id = AgentId::Queen(QueenId(format!("Q{}", id)));
        let key = format!("Q{}", id);

        self.agents.insert(
            key,
            AgentState {
                id: agent_id.clone(),
                working_dir: config.working_dir,
                is_alive: true,
            },
        );

        // TODO: In production, this would spawn a real Claude Code subprocess
        // via pipe_process::spawn_claude_code() and wire up the NDJSON pipe.
        // For now, the agent is registered but no subprocess is spawned.
        // Integration with queen::stream_queen will connect the real implementation.

        Ok(agent_id)
    }

    async fn assign_task(
        &self,
        agent_id: &AgentId,
        _task: Task,
        _context: TaskContext,
    ) -> Result<()> {
        let key = agent_id_to_key(agent_id);
        if !self.agents.contains_key(&key) {
            anyhow::bail!("Agent {} not found", key);
        }
        // TODO: Send task via NDJSON pipe to the Claude Code subprocess.
        // This will integrate with QueenCommand::Assign.
        Ok(())
    }

    async fn abort_task(
        &self,
        agent_id: &AgentId,
        _task_id: &TaskId,
        _reason: &str,
    ) -> Result<()> {
        let key = agent_id_to_key(agent_id);
        if !self.agents.contains_key(&key) {
            anyhow::bail!("Agent {} not found", key);
        }
        // TODO: Send abort via QueenCommand::AbortTask
        Ok(())
    }

    async fn shutdown(&self, agent_id: &AgentId) -> Result<()> {
        let key = agent_id_to_key(agent_id);
        if !self.agents.contains_key(&key) {
            anyhow::bail!("Agent {} not found", key);
        }
        // TODO: Send QueenCommand::Shutdown
        Ok(())
    }

    async fn next_event(&mut self) -> Option<AgentEvent> {
        self.event_rx.recv().await
    }

    fn active_agents(&self) -> Vec<AgentId> {
        self.agents
            .values()
            .filter(|s| s.is_alive)
            .map(|s| s.id.clone())
            .collect()
    }

    fn is_alive(&self, agent_id: &AgentId) -> bool {
        let key = agent_id_to_key(agent_id);
        self.agents
            .get(&key)
            .map(|s| s.is_alive)
            .unwrap_or(false)
    }
}

fn agent_id_to_key(agent_id: &AgentId) -> String {
    match agent_id {
        AgentId::Nydus(id) => format!("nydus:{}", id.0),
        AgentId::Queen(id) => format!("queen:{}", id.0),
        AgentId::Overlord(id) => format!("overlord:{}", id.0),
        AgentId::Overmind(id) => format!("overmind:{}", id.0),
        AgentId::Validator => "validator".to_string(),
        AgentId::Operator => "operator".to_string(),
    }
}
