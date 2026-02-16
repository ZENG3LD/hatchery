//! Session memory implementation.
//!
//! Tracks agent sessions for recovery and state persistence. Each session
//! has a lifecycle (Active, Suspended, Completed, Failed) and associated data.

use super::Memory;
use crate::core::types::AgentId;
use anyhow::{Context, Result};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Configuration for SessionMemory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionConfig {
    /// Maximum number of sessions to keep in memory.
    pub max_sessions: usize,
    /// Time-to-live for completed/failed sessions.
    pub session_ttl: Duration,
}

impl Default for SessionConfig {
    fn default() -> Self {
        SessionConfig {
            max_sessions: 100,
            session_ttl: Duration::from_secs(86400), // 24 hours
        }
    }
}

/// Session lifecycle state.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SessionState {
    Active,
    Suspended,
    Completed,
    Failed,
}

/// A session record tracking an agent's work session.
#[derive(Debug, Clone)]
struct SessionRecord {
    session_id: String,
    agent_key: String,
    started_at: Instant,
    last_active: Instant,
    state: SessionState,
    data: HashMap<String, Value>,
    error_message: Option<String>,
}

/// Serializable session record for snapshots.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SerializableSession {
    session_id: String,
    agent_key: String,
    started_secs: u64,
    last_active_secs: u64,
    state: SessionState,
    data: HashMap<String, Value>,
    error_message: Option<String>,
}

/// Session memory for tracking agent sessions.
pub struct SessionMemory {
    config: SessionConfig,
    sessions: Arc<RwLock<HashMap<String, SessionRecord>>>,
    start_time: Instant,
}

impl SessionMemory {
    /// Create a new SessionMemory with default configuration.
    pub fn new() -> Self {
        Self::with_config(SessionConfig::default())
    }

    /// Create a new SessionMemory with custom configuration.
    pub fn with_config(config: SessionConfig) -> Self {
        SessionMemory {
            config,
            sessions: Arc::new(RwLock::new(HashMap::new())),
            start_time: Instant::now(),
        }
    }

    /// Create a new session for an agent.
    pub fn create_session(&mut self, agent: AgentId) -> String {
        let session_id = uuid::Uuid::new_v4().to_string();
        let agent_key = Self::agent_id_to_key(&agent);
        let now = Instant::now();

        let record = SessionRecord {
            session_id: session_id.clone(),
            agent_key,
            started_at: now,
            last_active: now,
            state: SessionState::Active,
            data: HashMap::new(),
            error_message: None,
        };

        self.sessions.write().insert(session_id.clone(), record);

        eprintln!("[SessionMemory] Created session: {}", session_id);
        session_id
    }

    /// Resume a suspended session.
    pub fn resume_session(&mut self, session_id: &str) -> Result<()> {
        let mut sessions = self.sessions.write();
        let session = sessions
            .get_mut(session_id)
            .context("Session not found")?;

        if session.state != SessionState::Suspended {
            anyhow::bail!("Cannot resume session in state: {:?}", session.state);
        }

        session.state = SessionState::Active;
        session.last_active = Instant::now();

        eprintln!("[SessionMemory] Resumed session: {}", session_id);
        Ok(())
    }

    /// Suspend an active session.
    pub fn suspend_session(&mut self, session_id: &str) -> Result<()> {
        let mut sessions = self.sessions.write();
        let session = sessions
            .get_mut(session_id)
            .context("Session not found")?;

        if session.state != SessionState::Active {
            anyhow::bail!("Cannot suspend session in state: {:?}", session.state);
        }

        session.state = SessionState::Suspended;
        session.last_active = Instant::now();

        eprintln!("[SessionMemory] Suspended session: {}", session_id);
        Ok(())
    }

    /// Complete a session successfully.
    pub fn complete_session(&mut self, session_id: &str) -> Result<()> {
        let mut sessions = self.sessions.write();
        let session = sessions
            .get_mut(session_id)
            .context("Session not found")?;

        session.state = SessionState::Completed;
        session.last_active = Instant::now();

        eprintln!("[SessionMemory] Completed session: {}", session_id);
        Ok(())
    }

    /// Mark a session as failed with an error message.
    pub fn fail_session(&mut self, session_id: &str, error: String) -> Result<()> {
        let mut sessions = self.sessions.write();
        let session = sessions
            .get_mut(session_id)
            .context("Session not found")?;

        session.state = SessionState::Failed;
        session.error_message = Some(error);
        session.last_active = Instant::now();

        eprintln!("[SessionMemory] Failed session: {}", session_id);
        Ok(())
    }

    /// Get session state.
    pub fn get_session_state(&self, session_id: &str) -> Option<SessionState> {
        self.sessions.read().get(session_id).map(|s| s.state.clone())
    }

    /// Get all sessions for a specific agent.
    pub fn get_agent_sessions(&self, agent: &AgentId) -> Vec<(String, SessionState)> {
        let agent_key = Self::agent_id_to_key(agent);
        self.sessions
            .read()
            .iter()
            .filter(|(_, session)| session.agent_key == agent_key)
            .map(|(id, session)| (id.clone(), session.state.clone()))
            .collect()
    }

    /// Get session data.
    pub fn get_session_data(&self, session_id: &str) -> Option<HashMap<String, Value>> {
        self.sessions
            .read()
            .get(session_id)
            .map(|s| s.data.clone())
    }

    /// List all sessions with their states.
    pub fn list_sessions(&self) -> Vec<(String, String, SessionState)> {
        self.sessions
            .read()
            .iter()
            .map(|(id, session)| {
                (
                    id.clone(),
                    session.agent_key.clone(),
                    session.state.clone(),
                )
            })
            .collect()
    }

    /// Convert AgentId to key.
    fn agent_id_to_key(agent: &AgentId) -> String {
        match agent {
            AgentId::Nydus(id) => format!("nydus:{}", id.0),
            AgentId::Queen(id) => format!("queen:{}", id.0),
            AgentId::Overlord(id) => format!("overlord:{}", id.0),
            AgentId::Overmind(id) => format!("overmind:{}", id.0),
            AgentId::Validator => "validator".to_string(),
            AgentId::Operator => "operator".to_string(),
        }
    }

    /// Parse session key format "session:<id>:<data_key>".
    fn parse_session_key(key: &str) -> Option<(String, String)> {
        if key.starts_with("session:") {
            let parts: Vec<&str> = key.splitn(3, ':').collect();
            if parts.len() == 3 {
                return Some((parts[1].to_string(), parts[2].to_string()));
            }
        }
        None
    }
}

impl Default for SessionMemory {
    fn default() -> Self {
        Self::new()
    }
}

impl Memory for SessionMemory {
    fn insert(&mut self, key: String, value: Value, _source: AgentId) -> Result<()> {
        // Key format: "session:<id>:<data_key>"
        let (session_id, data_key) = Self::parse_session_key(&key)
            .context("Invalid session key format")?;

        let mut sessions = self.sessions.write();
        let session = sessions
            .get_mut(&session_id)
            .context("Session not found")?;

        session.data.insert(data_key, value);
        session.last_active = Instant::now();

        Ok(())
    }

    fn get(&self, key: &str) -> Option<Value> {
        let (session_id, data_key) = Self::parse_session_key(key)?;

        self.sessions
            .read()
            .get(&session_id)?
            .data
            .get(&data_key)
            .cloned()
    }

    fn query(&self, pattern: &str) -> Result<Vec<(String, Value)>> {
        let sessions = self.sessions.read();
        let mut results = Vec::new();

        for (session_id, session) in sessions.iter() {
            for (data_key, value) in &session.data {
                let full_key = format!("session:{}:{}", session_id, data_key);
                if full_key.contains(pattern) || data_key.contains(pattern) {
                    results.push((full_key, value.clone()));
                }
            }
        }

        Ok(results)
    }

    fn evict(&mut self) -> Result<usize> {
        let now = Instant::now();
        let mut sessions = self.sessions.write();
        let initial_count = sessions.len();

        // Remove sessions that are completed/failed and older than TTL
        sessions.retain(|_, session| {
            let age = now.duration_since(session.last_active);
            let is_terminal = session.state == SessionState::Completed
                || session.state == SessionState::Failed;

            !(is_terminal && age > self.config.session_ttl)
        });

        // If still over limit, remove oldest completed/failed sessions
        if sessions.len() > self.config.max_sessions {
            let sorted: Vec<_> = sessions.iter().collect();
            let mut sorted_vec = sorted;
            sorted_vec.sort_by_key(|(_, s)| s.last_active);

            let to_remove = sessions.len() - self.config.max_sessions;
            let mut to_remove_ids = Vec::new();

            for (id, session) in sorted_vec.iter() {
                if to_remove_ids.len() >= to_remove {
                    break;
                }
                if session.state == SessionState::Completed
                    || session.state == SessionState::Failed
                {
                    to_remove_ids.push((*id).clone());
                }
            }

            for id in to_remove_ids {
                sessions.remove(&id);
            }
        }

        let evicted = initial_count - sessions.len();
        Ok(evicted)
    }

    fn snapshot(&self) -> Result<Value> {
        let sessions = self.sessions.read();
        let snapshot: HashMap<String, SerializableSession> = sessions
            .iter()
            .map(|(id, session)| {
                (
                    id.clone(),
                    SerializableSession {
                        session_id: session.session_id.clone(),
                        agent_key: session.agent_key.clone(),
                        started_secs: session.started_at.duration_since(self.start_time).as_secs(),
                        last_active_secs: session.last_active.duration_since(self.start_time).as_secs(),
                        state: session.state.clone(),
                        data: session.data.clone(),
                        error_message: session.error_message.clone(),
                    },
                )
            })
            .collect();

        serde_json::to_value(&snapshot).context("Failed to serialize session snapshot")
    }

    fn restore(&mut self, snapshot: Value) -> Result<()> {
        let snapshot_map: HashMap<String, SerializableSession> =
            serde_json::from_value(snapshot).context("Failed to deserialize session snapshot")?;

        let mut sessions = self.sessions.write();
        sessions.clear();

        for (id, ser_session) in snapshot_map {
            let started_at = self.start_time + Duration::from_secs(ser_session.started_secs);
            let last_active = self.start_time + Duration::from_secs(ser_session.last_active_secs);

            sessions.insert(
                id,
                SessionRecord {
                    session_id: ser_session.session_id,
                    agent_key: ser_session.agent_key,
                    started_at,
                    last_active,
                    state: ser_session.state,
                    data: ser_session.data,
                    error_message: ser_session.error_message,
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
    fn test_create_and_complete_session() {
        let mut mem = SessionMemory::new();
        let agent = AgentId::Queen(QueenId("Q0".to_string()));

        let session_id = mem.create_session(agent);
        assert_eq!(mem.get_session_state(&session_id), Some(SessionState::Active));

        mem.complete_session(&session_id).unwrap();
        assert_eq!(mem.get_session_state(&session_id), Some(SessionState::Completed));
    }

    #[test]
    fn test_suspend_and_resume() {
        let mut mem = SessionMemory::new();
        let agent = AgentId::Queen(QueenId("Q1".to_string()));

        let session_id = mem.create_session(agent);

        mem.suspend_session(&session_id).unwrap();
        assert_eq!(mem.get_session_state(&session_id), Some(SessionState::Suspended));

        mem.resume_session(&session_id).unwrap();
        assert_eq!(mem.get_session_state(&session_id), Some(SessionState::Active));
    }

    #[test]
    fn test_session_data() {
        let mut mem = SessionMemory::new();
        let agent = AgentId::Operator;

        let session_id = mem.create_session(agent.clone());
        let key = format!("session:{}:progress", session_id);

        mem.insert(key.clone(), Value::Number(50.into()), agent)
            .unwrap();

        let retrieved = mem.get(&key);
        assert_eq!(retrieved, Some(Value::Number(50.into())));
    }

    #[test]
    fn test_fail_session() {
        let mut mem = SessionMemory::new();
        let agent = AgentId::Validator;

        let session_id = mem.create_session(agent);

        mem.fail_session(&session_id, "Test error".to_string())
            .unwrap();

        assert_eq!(mem.get_session_state(&session_id), Some(SessionState::Failed));
    }
}
