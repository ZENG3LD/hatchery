//! P6: minimal task manager. Tasks live in SQLite under the server data dir. An operator
//! creates and assigns them; a node sees (and progresses) only the tasks assigned to it, and a
//! task can be assigned only to a node whose ledger entry holds the `tasks` grant (trusted by
//! default). The hub enforces that; this module is storage and state rules.

use crate::error::ServerError;
use rusqlite::{params, Connection};
use serde::Serialize;
use std::path::Path;

#[derive(Clone, Debug, Serialize, Eq, PartialEq)]
pub struct Task {
    pub id: i64,
    pub title: String,
    pub body: String,
    pub assignee: Option<String>,
    /// open | assigned | in_progress | done | cancelled
    pub status: String,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

pub struct TaskStore {
    conn: Connection,
}

fn unix_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}

fn row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Task> {
    Ok(Task {
        id: r.get(0)?,
        title: r.get(1)?,
        body: r.get(2)?,
        assignee: r.get(3)?,
        status: r.get(4)?,
        created_at_ms: r.get::<_, i64>(5)? as u64,
        updated_at_ms: r.get::<_, i64>(6)? as u64,
    })
}

fn db(e: rusqlite::Error) -> ServerError {
    ServerError::sub("tasks", e.to_string())
}

impl TaskStore {
    pub fn open(data_dir: &Path) -> Result<Self, ServerError> {
        let dir = data_dir.join("tasks");
        std::fs::create_dir_all(&dir)?;
        Self::from_conn(Connection::open(dir.join("tasks.db")).map_err(db)?)
    }

    pub fn in_memory() -> Result<Self, ServerError> {
        Self::from_conn(Connection::open_in_memory().map_err(db)?)
    }

    fn from_conn(conn: Connection) -> Result<Self, ServerError> {
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             CREATE TABLE IF NOT EXISTS tasks (
               id INTEGER PRIMARY KEY AUTOINCREMENT,
               title TEXT NOT NULL, body TEXT NOT NULL, assignee TEXT,
               status TEXT NOT NULL, created_at_ms INTEGER NOT NULL, updated_at_ms INTEGER NOT NULL);",
        )
        .map_err(db)?;
        Ok(Self { conn })
    }

    pub fn create(&self, title: &str, body: &str, assignee: Option<&str>) -> Result<Task, ServerError> {
        let now = unix_ms() as i64;
        let status = if assignee.is_some() { "assigned" } else { "open" };
        self.conn
            .execute("INSERT INTO tasks (title, body, assignee, status, created_at_ms, updated_at_ms) VALUES (?1,?2,?3,?4,?5,?5)", params![title, body, assignee, status, now])
            .map_err(db)?;
        self.get(self.conn.last_insert_rowid())?.ok_or_else(|| ServerError::sub("tasks", "lost row"))
    }

    pub fn get(&self, id: i64) -> Result<Option<Task>, ServerError> {
        let mut stmt = self.conn.prepare("SELECT id,title,body,assignee,status,created_at_ms,updated_at_ms FROM tasks WHERE id=?1").map_err(db)?;
        let mut rows = stmt.query_map(params![id], row).map_err(db)?;
        rows.next().transpose().map_err(db)
    }

    pub fn list(&self, assignee: Option<&str>, open_only: bool) -> Result<Vec<Task>, ServerError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id,title,body,assignee,status,created_at_ms,updated_at_ms FROM tasks WHERE (?1 IS NULL OR assignee=?1) AND (?2=0 OR status IN ('open','assigned','in_progress')) ORDER BY id")
            .map_err(db)?;
        let rows = stmt.query_map(params![assignee, open_only as i64], row).map_err(db)?;
        rows.collect::<Result<_, _>>().map_err(db)
    }

    pub fn assign(&self, id: i64, assignee: &str) -> Result<bool, ServerError> {
        let n = self
            .conn
            .execute("UPDATE tasks SET assignee=?2, status='assigned', updated_at_ms=?3 WHERE id=?1 AND status IN ('open','assigned')", params![id, assignee, unix_ms() as i64])
            .map_err(db)?;
        Ok(n == 1)
    }

    /// Operator cancel, or the assignee moving its own task along (`in_progress`, `done`).
    pub fn set_status(&self, id: i64, status: &str, only_assignee: Option<&str>) -> Result<bool, ServerError> {
        let n = self
            .conn
            .execute(
                "UPDATE tasks SET status=?2, updated_at_ms=?3 WHERE id=?1 AND status NOT IN ('done','cancelled') AND (?4 IS NULL OR assignee=?4)",
                params![id, status, unix_ms() as i64, only_assignee],
            )
            .map_err(db)?;
        Ok(n == 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle_and_ownership() {
        let s = TaskStore::in_memory().unwrap();
        let t = s.create("build", "run it", None).unwrap();
        assert_eq!(t.status, "open");
        assert!(s.assign(t.id, "alpha").unwrap());
        assert_eq!(s.list(Some("alpha"), true).unwrap().len(), 1);
        assert!(s.list(Some("beta"), true).unwrap().is_empty());
        // Only the assignee moves its own task.
        assert!(!s.set_status(t.id, "done", Some("beta")).unwrap());
        assert!(s.set_status(t.id, "in_progress", Some("alpha")).unwrap());
        assert!(s.set_status(t.id, "done", Some("alpha")).unwrap());
        // Finished is final; no reassign.
        assert!(!s.set_status(t.id, "open", None).unwrap());
        assert!(!s.assign(t.id, "beta").unwrap());
        assert!(s.list(Some("alpha"), true).unwrap().is_empty());
        assert_eq!(s.list(None, false).unwrap().len(), 1);
    }

    #[test]
    fn survives_reopen() {
        let dir = tempfile::tempdir().unwrap();
        TaskStore::open(dir.path()).unwrap().create("a", "b", Some("alpha")).unwrap();
        assert_eq!(TaskStore::open(dir.path()).unwrap().list(Some("alpha"), true).unwrap().len(), 1);
    }
}
