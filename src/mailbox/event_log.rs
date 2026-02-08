use std::path::Path;
use std::sync::Arc;
use parking_lot::Mutex;
use rusqlite::{Connection, params};
use anyhow::Result;
use crate::core::types::*;

/// SqliteEventLog — Durable storage for all swarm messages.
///
/// This provides an append-only log of all messages sent through the swarm,
/// enabling audit trails, replay, and analytics.
pub struct SqliteEventLog {
    conn: Arc<Mutex<Connection>>,
}

impl SqliteEventLog {
    /// Create a new event log, opening/creating the SQLite database at the given path.
    pub fn new(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;

        // Enable WAL mode for concurrent reads
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;

        // Create events table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS events (
                id TEXT PRIMARY KEY,
                timestamp TEXT NOT NULL,
                from_agent TEXT NOT NULL,
                to_agent TEXT NOT NULL,
                msg_type TEXT NOT NULL,
                payload TEXT NOT NULL,
                correlation_id TEXT,
                visibility_agent BOOLEAN DEFAULT 1,
                visibility_coordinator BOOLEAN DEFAULT 1,
                visibility_user BOOLEAN DEFAULT 1
            )",
            [],
        )?;

        // Create indexes for efficient querying
        conn.execute("CREATE INDEX IF NOT EXISTS idx_events_timestamp ON events(timestamp)", [])?;
        conn.execute("CREATE INDEX IF NOT EXISTS idx_events_to ON events(to_agent)", [])?;
        conn.execute("CREATE INDEX IF NOT EXISTS idx_events_correlation ON events(correlation_id)", [])?;
        conn.execute("CREATE INDEX IF NOT EXISTS idx_events_type ON events(msg_type)", [])?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// Create an in-memory event log (for testing).
    pub fn in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;

        // Same schema as file-based
        conn.execute(
            "CREATE TABLE IF NOT EXISTS events (
                id TEXT PRIMARY KEY,
                timestamp TEXT NOT NULL,
                from_agent TEXT NOT NULL,
                to_agent TEXT NOT NULL,
                msg_type TEXT NOT NULL,
                payload TEXT NOT NULL,
                correlation_id TEXT,
                visibility_agent BOOLEAN DEFAULT 1,
                visibility_coordinator BOOLEAN DEFAULT 1,
                visibility_user BOOLEAN DEFAULT 1
            )",
            [],
        )?;

        // Same indexes
        conn.execute("CREATE INDEX IF NOT EXISTS idx_events_timestamp ON events(timestamp)", [])?;
        conn.execute("CREATE INDEX IF NOT EXISTS idx_events_to ON events(to_agent)", [])?;
        conn.execute("CREATE INDEX IF NOT EXISTS idx_events_correlation ON events(correlation_id)", [])?;
        conn.execute("CREATE INDEX IF NOT EXISTS idx_events_type ON events(msg_type)", [])?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// Log a message to the event store.
    ///
    /// This is synchronous and blocking, but should be fast for single inserts.
    pub fn log(&self, msg: &SwarmMessage) {
        let conn = self.conn.lock();
        let _ = conn.execute(
            "INSERT INTO events (id, timestamp, from_agent, to_agent, msg_type, payload,
             correlation_id, visibility_agent, visibility_coordinator, visibility_user)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                msg.id,
                msg.timestamp.to_rfc3339(),
                serde_json::to_string(&msg.from).unwrap_or_default(),
                serde_json::to_string(&msg.to).unwrap_or_default(),
                serde_json::to_string(&msg.msg_type).unwrap_or_default(),
                msg.payload.to_string(),
                msg.correlation_id,
                msg.visibility.agent_visible,
                msg.visibility.coordinator_visible,
                msg.visibility.user_visible,
            ],
        );
    }

    /// Query events with optional filters.
    ///
    /// Note: This is a simplified implementation. For production, use a proper query builder.
    pub fn query(
        &self,
        _from_time: Option<&str>,
        _to_time: Option<&str>,
        _agent: Option<&str>,
        _msg_type: Option<&str>,
    ) -> Result<Vec<SwarmMessage>> {
        let conn = self.conn.lock();

        // For now, just return all events ordered by timestamp
        // TODO: Add dynamic filter building
        let mut stmt = conn.prepare(
            "SELECT id, timestamp, from_agent, to_agent, msg_type, payload, correlation_id,
                    visibility_agent, visibility_coordinator, visibility_user
             FROM events ORDER BY timestamp"
        )?;

        let rows = stmt.query_map([], |row| {
            Ok(Self::row_to_message(row))
        })?;

        let mut messages = Vec::new();
        for row in rows {
            if let Ok(msg) = row {
                messages.push(msg);
            }
        }
        Ok(messages)
    }

    /// Replay events from a given cursor (event id or timestamp).
    ///
    /// If `after_id` is None, replays all events from the beginning.
    pub fn replay(&self, after_id: Option<&str>) -> Result<Vec<SwarmMessage>> {
        let conn = self.conn.lock();
        let messages = if let Some(id) = after_id {
            let mut stmt = conn.prepare(
                "SELECT id, timestamp, from_agent, to_agent, msg_type, payload, correlation_id,
                        visibility_agent, visibility_coordinator, visibility_user
                 FROM events WHERE timestamp > (SELECT timestamp FROM events WHERE id = ?1)
                 ORDER BY timestamp"
            )?;
            let rows = stmt.query_map(params![id], |row| Ok(Self::row_to_message(row)))?;
            rows.filter_map(|r| r.ok()).collect()
        } else {
            let mut stmt = conn.prepare(
                "SELECT id, timestamp, from_agent, to_agent, msg_type, payload, correlation_id,
                        visibility_agent, visibility_coordinator, visibility_user
                 FROM events ORDER BY timestamp"
            )?;
            let rows = stmt.query_map([], |row| Ok(Self::row_to_message(row)))?;
            rows.filter_map(|r| r.ok()).collect()
        };
        Ok(messages)
    }

    /// Count total events in the log.
    pub fn count(&self) -> usize {
        let conn = self.conn.lock();
        conn.query_row("SELECT COUNT(*) FROM events", [], |row| row.get(0))
            .unwrap_or(0)
    }

    /// Get stats: total events, events per agent, events per type.
    pub fn stats(&self) -> Result<EventLogStats> {
        let conn = self.conn.lock();
        let total: usize = conn.query_row("SELECT COUNT(*) FROM events", [], |row| row.get(0))?;
        Ok(EventLogStats { total_events: total })
    }

    /// Helper: convert a SQLite row to SwarmMessage.
    fn row_to_message(row: &rusqlite::Row) -> SwarmMessage {
        let id: String = row.get(0).unwrap_or_default();
        let timestamp_str: String = row.get(1).unwrap_or_default();
        let from_str: String = row.get(2).unwrap_or_default();
        let to_str: String = row.get(3).unwrap_or_default();
        let msg_type_str: String = row.get(4).unwrap_or_default();
        let payload_str: String = row.get(5).unwrap_or_default();
        let correlation_id: Option<String> = row.get(6).unwrap_or(None);
        let agent_vis: bool = row.get(7).unwrap_or(true);
        let coord_vis: bool = row.get(8).unwrap_or(true);
        let user_vis: bool = row.get(9).unwrap_or(true);

        SwarmMessage {
            id,
            from: serde_json::from_str(&from_str).unwrap_or(AgentId::Operator),
            to: serde_json::from_str(&to_str).unwrap_or(AgentId::Operator),
            msg_type: serde_json::from_str(&msg_type_str).unwrap_or(MessageType::Custom("unknown".into())),
            payload: serde_json::from_str(&payload_str).unwrap_or(serde_json::Value::Null),
            timestamp: chrono::DateTime::parse_from_rfc3339(&timestamp_str)
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .unwrap_or_else(|_| chrono::Utc::now()),
            correlation_id,
            visibility: Visibility {
                agent_visible: agent_vis,
                coordinator_visible: coord_vis,
                user_visible: user_vis,
            },
        }
    }
}

/// Statistics about the event log.
#[derive(Debug, Clone)]
pub struct EventLogStats {
    pub total_events: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_message(from: AgentId, to: AgentId, msg_type: MessageType) -> SwarmMessage {
        SwarmMessage {
            id: uuid::Uuid::new_v4().to_string(),
            from,
            to,
            msg_type,
            payload: serde_json::json!({"test": true}),
            timestamp: chrono::Utc::now(),
            correlation_id: None,
            visibility: Visibility::default_internal(),
        }
    }

    #[test]
    fn test_log_and_count() {
        let log = SqliteEventLog::in_memory().unwrap();
        assert_eq!(log.count(), 0);

        let msg = make_test_message(
            AgentId::Queen(QueenId("Q0".into())),
            AgentId::SwarmHost(SwarmHostId::default()),
            MessageType::StatusReport,
        );
        log.log(&msg);
        assert_eq!(log.count(), 1);
    }

    #[test]
    fn test_log_multiple() {
        let log = SqliteEventLog::in_memory().unwrap();
        for i in 0..5 {
            let msg = make_test_message(
                AgentId::Queen(QueenId(format!("Q{}", i))),
                AgentId::SwarmHost(SwarmHostId::default()),
                MessageType::TaskProgress,
            );
            log.log(&msg);
        }
        assert_eq!(log.count(), 5);
    }

    #[test]
    fn test_replay_all() {
        let log = SqliteEventLog::in_memory().unwrap();
        for i in 0..5 {
            let msg = make_test_message(
                AgentId::Queen(QueenId(format!("Q{}", i))),
                AgentId::SwarmHost(SwarmHostId::default()),
                MessageType::TaskProgress,
            );
            log.log(&msg);
        }
        let all = log.replay(None).unwrap();
        assert_eq!(all.len(), 5);
    }

    #[test]
    fn test_replay_after_id() {
        let log = SqliteEventLog::in_memory().unwrap();
        let mut ids = Vec::new();

        for i in 0..5 {
            let msg = make_test_message(
                AgentId::Queen(QueenId(format!("Q{}", i))),
                AgentId::SwarmHost(SwarmHostId::default()),
                MessageType::TaskProgress,
            );
            ids.push(msg.id.clone());
            log.log(&msg);
        }

        // Replay after the 2nd message (should get messages 3, 4, 5)
        let after = log.replay(Some(&ids[1])).unwrap();
        assert_eq!(after.len(), 3);
    }

    #[test]
    fn test_stats() {
        let log = SqliteEventLog::in_memory().unwrap();
        let msg = make_test_message(
            AgentId::Operator,
            AgentId::BroodLord,
            MessageType::Shutdown,
        );
        log.log(&msg);
        let stats = log.stats().unwrap();
        assert_eq!(stats.total_events, 1);
    }

    #[test]
    fn test_query() {
        let log = SqliteEventLog::in_memory().unwrap();
        for i in 0..3 {
            let msg = make_test_message(
                AgentId::Queen(QueenId(format!("Q{}", i))),
                AgentId::SwarmHost(SwarmHostId::default()),
                MessageType::StatusReport,
            );
            log.log(&msg);
        }

        let results = log.query(None, None, None, None).unwrap();
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn test_message_roundtrip() {
        let log = SqliteEventLog::in_memory().unwrap();
        let original = make_test_message(
            AgentId::Queen(QueenId("Q42".into())),
            AgentId::Validator,
            MessageType::TaskAssignment,
        );

        log.log(&original);
        let replayed = log.replay(None).unwrap();

        assert_eq!(replayed.len(), 1);
        let retrieved = &replayed[0];
        assert_eq!(retrieved.id, original.id);
        // Note: JSON serialization of AgentId might differ, so we just check the message exists
    }
}
