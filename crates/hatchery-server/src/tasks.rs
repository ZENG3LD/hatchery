//! hq-core work model: one task store (former P6 tasks + the harness kernel's task/run
//! ideas). Tasks carry a revision for compare-and-set and can be scoped to a role; a run
//! binds a task to one real provider session on a node; sessions are recorded only when
//! they are real provider conversations (drafts stay in hq-edge). Operator calls are
//! idempotent by `op_id`. SQLite under the server data dir.
//!
//! Task states: backlog | open | assigned | in_progress | review | done | failed | cancelled.
//! Run states: running | review | done | failed | cancelled.

use crate::error::ServerError;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use serde_json::Value;
use std::path::Path;

pub const TASK_STATES: &[&str] =
    &["backlog", "open", "assigned", "in_progress", "review", "done", "failed", "cancelled"];
pub const FINAL_TASK_STATES: &[&str] = &["done", "failed", "cancelled", "archived"];
pub const RUN_STATES: &[&str] = &["running", "review", "done", "failed", "cancelled"];

#[derive(Clone, Debug, Serialize, Eq, PartialEq)]
pub struct Task {
    pub id: i64,
    pub revision: i64,
    pub title: String,
    pub body: String,
    pub assignee: Option<String>,
    pub role: Option<String>,
    pub parent_id: Option<i64>,
    pub status: String,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

#[derive(Clone, Debug, Serialize, Eq, PartialEq)]
pub struct Run {
    pub id: i64,
    pub task_id: i64,
    pub node_id: String,
    pub session_id: String,
    pub status: String,
    pub branch: Option<String>,
    pub commit_id: Option<String>,
    pub result_message_id: Option<String>,
    pub closed_by: Option<String>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

#[derive(Clone, Debug, Serialize, Eq, PartialEq)]
pub struct Session {
    pub session_id: String,
    pub node_id: String,
    pub provider: String,
    pub workspace_id: Option<String>,
    pub role: Option<String>,
    pub state: String,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

/// Why a mutation did not apply.
#[derive(Debug, PartialEq, Eq)]
pub enum Refused {
    NotFound,
    Conflict(&'static str),
    Invalid(&'static str),
}

pub struct TaskStore {
    conn: Connection,
}

fn unix_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as i64
}

fn db(e: rusqlite::Error) -> ServerError {
    ServerError::sub("tasks", e.to_string())
}

const TASK_COLS: &str = "id,revision,title,body,assignee,role,parent_id,status,created_at_ms,updated_at_ms";
const RUN_COLS: &str = "id,task_id,node_id,session_id,status,branch,commit_id,result_message_id,closed_by,created_at_ms,updated_at_ms";
const SESSION_COLS: &str = "session_id,node_id,provider,workspace_id,role,state,created_at_ms,updated_at_ms";

fn task_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Task> {
    Ok(Task {
        id: r.get(0)?,
        revision: r.get(1)?,
        title: r.get(2)?,
        body: r.get(3)?,
        assignee: r.get(4)?,
        role: r.get(5)?,
        parent_id: r.get(6)?,
        status: r.get(7)?,
        created_at_ms: r.get::<_, i64>(8)? as u64,
        updated_at_ms: r.get::<_, i64>(9)? as u64,
    })
}

fn run_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Run> {
    Ok(Run {
        id: r.get(0)?,
        task_id: r.get(1)?,
        node_id: r.get(2)?,
        session_id: r.get(3)?,
        status: r.get(4)?,
        branch: r.get(5)?,
        commit_id: r.get(6)?,
        result_message_id: r.get(7)?,
        closed_by: r.get(8)?,
        created_at_ms: r.get::<_, i64>(9)? as u64,
        updated_at_ms: r.get::<_, i64>(10)? as u64,
    })
}

fn session_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Session> {
    Ok(Session {
        session_id: r.get(0)?,
        node_id: r.get(1)?,
        provider: r.get(2)?,
        workspace_id: r.get(3)?,
        role: r.get(4)?,
        state: r.get(5)?,
        created_at_ms: r.get::<_, i64>(6)? as u64,
        updated_at_ms: r.get::<_, i64>(7)? as u64,
    })
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
               status TEXT NOT NULL, created_at_ms INTEGER NOT NULL, updated_at_ms INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS runs (
               id INTEGER PRIMARY KEY AUTOINCREMENT, task_id INTEGER NOT NULL, node_id TEXT NOT NULL,
               session_id TEXT NOT NULL, status TEXT NOT NULL, branch TEXT, commit_id TEXT,
               result_message_id TEXT, closed_by TEXT,
               created_at_ms INTEGER NOT NULL, updated_at_ms INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS sessions (
               session_id TEXT PRIMARY KEY, node_id TEXT NOT NULL, provider TEXT NOT NULL,
               workspace_id TEXT, role TEXT, state TEXT NOT NULL,
               created_at_ms INTEGER NOT NULL, updated_at_ms INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS signals (
               id INTEGER PRIMARY KEY AUTOINCREMENT, node_id TEXT NOT NULL, workspace_id TEXT NOT NULL,
               instance_id INTEGER NOT NULL, provider TEXT NOT NULL, kind TEXT NOT NULL, hint TEXT,
               at_ms INTEGER NOT NULL);
             CREATE INDEX IF NOT EXISTS signals_node ON signals(node_id, id);
             CREATE TABLE IF NOT EXISTS provider_inventory (
               node_id TEXT NOT NULL, provider TEXT NOT NULL, source TEXT NOT NULL, models TEXT NOT NULL,
               updated_at_ms INTEGER NOT NULL, PRIMARY KEY (node_id, provider));
             CREATE TABLE IF NOT EXISTS routing_roles (
               role TEXT PRIMARY KEY, preset TEXT NOT NULL, auto INTEGER NOT NULL, updated_at_ms INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS ops (
               op_id TEXT PRIMARY KEY, result TEXT NOT NULL, at_ms INTEGER NOT NULL);",
        )
        .map_err(db)?;
        // Additive migration of the P6 table.
        for (col, ddl) in [
            ("revision", "ALTER TABLE tasks ADD COLUMN revision INTEGER NOT NULL DEFAULT 1"),
            ("role", "ALTER TABLE tasks ADD COLUMN role TEXT"),
            ("parent_id", "ALTER TABLE tasks ADD COLUMN parent_id INTEGER"),
        ] {
            let has: bool = conn
                .prepare("SELECT 1 FROM pragma_table_info('tasks') WHERE name=?1")
                .and_then(|mut s| s.exists(params![col]))
                .map_err(db)?;
            if !has {
                conn.execute(ddl, []).map_err(db)?;
            }
        }
        let has_nick: bool = conn
            .prepare("SELECT 1 FROM pragma_table_info('sessions') WHERE name='nick'")
            .and_then(|mut s| s.exists([]))
            .map_err(db)?;
        if !has_nick {
            conn.execute("ALTER TABLE sessions ADD COLUMN nick TEXT", []).map_err(db)?;
        }
        Ok(Self { conn })
    }

    pub fn set_session_nick(&self, session_id: &str, nick: &str) -> Result<(), ServerError> {
        self.conn.execute("UPDATE sessions SET nick=?2 WHERE session_id=?1", params![session_id, nick]).map_err(db)?;
        Ok(())
    }

    pub fn session_nick(&self, session_id: &str) -> Result<Option<String>, ServerError> {
        self.conn
            .query_row("SELECT nick FROM sessions WHERE session_id=?1", params![session_id], |r| r.get::<_, Option<String>>(0))
            .optional()
            .map(Option::flatten)
            .map_err(db)
    }

    /// (run, session nick) for every active run whose session has a nick.
    pub fn active_runs_with_nick(&self) -> Result<Vec<(Run, String)>, ServerError> {
        let mut stmt = self
            .conn
            .prepare(&format!(
                "SELECT {}, s.nick FROM runs r JOIN sessions s ON s.session_id=r.session_id WHERE r.status IN ('running','review') AND s.nick IS NOT NULL",
                RUN_COLS.split(',').map(|c| format!("r.{c}")).collect::<Vec<_>>().join(",")
            ))
            .map_err(db)?;
        let rows = stmt.query_map([], |r| Ok((run_row(r)?, r.get::<_, String>(11)?))).map_err(db)?;
        rows.collect::<Result<_, _>>().map_err(db)
    }

    /// Replay a recorded operation result, if `op_id` was seen.
    pub fn replay(&self, op_id: Option<&str>) -> Result<Option<Value>, ServerError> {
        let Some(op) = op_id else { return Ok(None) };
        let raw: Option<String> = self
            .conn
            .query_row("SELECT result FROM ops WHERE op_id=?1", params![op], |r| r.get(0))
            .optional()
            .map_err(db)?;
        Ok(raw.and_then(|r| serde_json::from_str(&r).ok()))
    }

    pub fn record(&self, op_id: Option<&str>, result: &Value) -> Result<(), ServerError> {
        if let Some(op) = op_id {
            self.conn
                .execute("INSERT OR IGNORE INTO ops (op_id, result, at_ms) VALUES (?1,?2,?3)", params![op, result.to_string(), unix_ms()])
                .map_err(db)?;
        }
        Ok(())
    }

    pub fn create(&self, title: &str, body: &str, assignee: Option<&str>) -> Result<Task, ServerError> {
        self.create_full(title, body, assignee, None, None, None)
    }

    pub fn create_full(
        &self,
        title: &str,
        body: &str,
        assignee: Option<&str>,
        role: Option<&str>,
        parent_id: Option<i64>,
        status: Option<&str>,
    ) -> Result<Task, ServerError> {
        let now = unix_ms();
        let status = status.unwrap_or(if assignee.is_some() { "assigned" } else { "open" });
        self.conn
            .execute(
                "INSERT INTO tasks (title, body, assignee, role, parent_id, status, revision, created_at_ms, updated_at_ms) VALUES (?1,?2,?3,?4,?5,?6,1,?7,?7)",
                params![title, body, assignee, role, parent_id, status, now],
            )
            .map_err(db)?;
        self.get(self.conn.last_insert_rowid())?.ok_or_else(|| ServerError::sub("tasks", "lost row"))
    }

    pub fn get(&self, id: i64) -> Result<Option<Task>, ServerError> {
        self.conn
            .query_row(&format!("SELECT {TASK_COLS} FROM tasks WHERE id=?1"), params![id], task_row)
            .optional()
            .map_err(db)
    }

    pub fn list(&self, assignee: Option<&str>, open_only: bool) -> Result<Vec<Task>, ServerError> {
        self.list_filtered(assignee, None, open_only)
    }

    /// `roles`: when set, only tasks without a role or with one of these roles.
    pub fn list_filtered(&self, assignee: Option<&str>, roles: Option<&[String]>, open_only: bool) -> Result<Vec<Task>, ServerError> {
        let mut stmt = self
            .conn
            .prepare(&format!(
                "SELECT {TASK_COLS} FROM tasks WHERE (?1 IS NULL OR assignee=?1) AND status<>'archived' AND (?2=0 OR status NOT IN ('done','failed','cancelled')) ORDER BY id"
            ))
            .map_err(db)?;
        let rows = stmt.query_map(params![assignee, open_only as i64], task_row).map_err(db)?;
        let all: Vec<Task> = rows.collect::<Result<_, _>>().map_err(db)?;
        Ok(match roles {
            None => all,
            Some(roles) => all.into_iter().filter(|t| t.role.as_ref().is_none_or(|r| roles.iter().any(|x| x == r))).collect(),
        })
    }

    fn bump(&self, id: i64, expected: Option<i64>, set: &str, extra: &[&dyn rusqlite::ToSql]) -> Result<Result<Task, Refused>, ServerError> {
        let Some(task) = self.get(id)? else { return Ok(Err(Refused::NotFound)) };
        if FINAL_TASK_STATES.contains(&task.status.as_str()) {
            return Ok(Err(Refused::Conflict("task is final")));
        }
        if expected.is_some_and(|e| e != task.revision) {
            return Ok(Err(Refused::Conflict("stale revision")));
        }
        let sql = format!("UPDATE tasks SET {set}, revision=revision+1, updated_at_ms=?2 WHERE id=?1 AND revision=?3");
        let mut args: Vec<&dyn rusqlite::ToSql> = vec![&id];
        let now = unix_ms();
        args.push(&now);
        args.push(&task.revision);
        args.extend_from_slice(extra);
        let n = self.conn.execute(&sql, args.as_slice()).map_err(db)?;
        if n != 1 {
            return Ok(Err(Refused::Conflict("stale revision")));
        }
        Ok(Ok(self.get(id)?.expect("task exists")))
    }

    pub fn assign(&self, id: i64, assignee: &str) -> Result<bool, ServerError> {
        let Some(t) = self.get(id)? else { return Ok(false) };
        if !matches!(t.status.as_str(), "backlog" | "open" | "assigned") {
            return Ok(false);
        }
        Ok(self.bump(id, None, "assignee=?4, status='assigned'", &[&assignee])?.is_ok())
    }

    /// Move a task to `status` (operator), with optional revision check.
    pub fn move_to(&self, id: i64, status: &str, expected: Option<i64>) -> Result<Result<Task, Refused>, ServerError> {
        if !TASK_STATES.contains(&status) {
            return Ok(Err(Refused::Invalid("unknown status")));
        }
        self.bump(id, expected, "status=?4", &[&status])
    }

    /// Operator cancel, or the assignee moving its own task along (`in_progress`, `done`).
    pub fn set_status(&self, id: i64, status: &str, only_assignee: Option<&str>) -> Result<bool, ServerError> {
        if let Some(who) = only_assignee {
            match self.get(id)? {
                Some(t) if t.assignee.as_deref() == Some(who) => {}
                _ => return Ok(false),
            }
        }
        Ok(self.move_to(id, status, None)?.is_ok())
    }

    /// Operator archive of a finished task: hidden from lists, kept for history.
    pub fn archive_task(&self, id: i64) -> Result<Result<Task, Refused>, ServerError> {
        let Some(task) = self.get(id)? else { return Ok(Err(Refused::NotFound)) };
        if !FINAL_TASK_STATES.contains(&task.status.as_str()) && task.status != "archived" {
            return Ok(Err(Refused::Conflict("only a finished task can be archived")));
        }
        self.conn
            .execute("UPDATE tasks SET status='archived', revision=revision+1, updated_at_ms=?2 WHERE id=?1", params![id, unix_ms() as i64])
            .map_err(db)?;
        Ok(self.get(id)?.ok_or(Refused::NotFound))
    }

    /// Operator archive of a session record with no running run: hidden from lists.
    pub fn archive_session(&self, session_id: &str) -> Result<Result<(), Refused>, ServerError> {
        if self.session(session_id)?.is_none() {
            return Ok(Err(Refused::NotFound));
        }
        let running: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM runs WHERE session_id=?1 AND status='running'", params![session_id], |r| r.get(0))
            .map_err(db)?;
        if running > 0 {
            return Ok(Err(Refused::Conflict("session has a running run")));
        }
        self.conn
            .execute("UPDATE sessions SET state='archived', updated_at_ms=?2 WHERE session_id=?1", params![session_id, unix_ms() as i64])
            .map_err(db)?;
        Ok(Ok(()))
    }

    // ---- provider inventory (node-reported) and the user's role choices ----

    /// Replace one node's provider inventory (what the node detected; nothing else).
    pub fn set_inventory(&self, node_id: &str, providers: &[crate::routing::ProviderInventory]) -> Result<(), ServerError> {
        let now = unix_ms() as i64;
        let tx = self.conn.unchecked_transaction().map_err(db)?;
        tx.execute("DELETE FROM provider_inventory WHERE node_id=?1", params![node_id]).map_err(db)?;
        for p in providers {
            let models = serde_json::to_string(&p.models).unwrap_or_else(|_| "[]".into());
            tx.execute(
                "INSERT INTO provider_inventory (node_id,provider,source,models,updated_at_ms) VALUES (?1,?2,?3,?4,?5)",
                params![node_id, p.provider, p.source, models, now],
            )
            .map_err(db)?;
        }
        tx.commit().map_err(db)
    }

    pub fn inventory(&self) -> Result<Vec<crate::routing::NodeInventory>, ServerError> {
        let mut stmt = self.conn.prepare("SELECT node_id,provider,source,models,updated_at_ms FROM provider_inventory ORDER BY node_id,provider").map_err(db)?;
        let rows = stmt
            .query_map([], |r| {
                let models: String = r.get(3)?;
                Ok(crate::routing::NodeInventory {
                    node: r.get(0)?,
                    provider: r.get(1)?,
                    source: r.get(2)?,
                    models: serde_json::from_str(&models).unwrap_or_default(),
                    updated_at_ms: r.get::<_, i64>(4)? as u64,
                })
            })
            .map_err(db)?;
        rows.collect::<Result<_, _>>().map_err(db)
    }

    pub fn set_role(&self, role: &str, r: &crate::routing::Role) -> Result<(), ServerError> {
        self.conn
            .execute(
                "INSERT INTO routing_roles (role,preset,auto,updated_at_ms) VALUES (?1,?2,?3,?4)
                 ON CONFLICT(role) DO UPDATE SET preset=?2, auto=?3, updated_at_ms=?4",
                params![role, serde_json::to_string(&r.preset).unwrap_or_else(|_| "[]".into()), r.auto as i64, unix_ms() as i64],
            )
            .map_err(db)?;
        Ok(())
    }

    pub fn delete_role(&self, role: &str) -> Result<bool, ServerError> {
        Ok(self.conn.execute("DELETE FROM routing_roles WHERE role=?1", params![role]).map_err(db)? > 0)
    }

    pub fn roles(&self) -> Result<std::collections::BTreeMap<String, crate::routing::Role>, ServerError> {
        let mut stmt = self.conn.prepare("SELECT role,preset,auto FROM routing_roles ORDER BY role").map_err(db)?;
        let rows = stmt
            .query_map([], |r| {
                let preset: String = r.get(1)?;
                Ok((r.get::<_, String>(0)?, crate::routing::Role { preset: serde_json::from_str(&preset).unwrap_or_default(), auto: r.get::<_, i64>(2)? != 0 }))
            })
            .map_err(db)?;
        rows.collect::<Result<_, _>>().map_err(db)
    }

    // ---- sessions (only real provider sessions) ----

    pub fn upsert_session(&self, s: &Session) -> Result<Session, ServerError> {
        let now = unix_ms();
        self.conn
            .execute(
                "INSERT INTO sessions (session_id,node_id,provider,workspace_id,role,state,created_at_ms,updated_at_ms) VALUES (?1,?2,?3,?4,?5,?6,?7,?7)
                 ON CONFLICT(session_id) DO UPDATE SET workspace_id=COALESCE(?4,workspace_id), role=COALESCE(?5,role), state=?6, updated_at_ms=?7",
                params![s.session_id, s.node_id, s.provider, s.workspace_id, s.role, s.state, now],
            )
            .map_err(db)?;
        self.session(&s.session_id)?.ok_or_else(|| ServerError::sub("tasks", "lost session"))
    }

    pub fn session(&self, id: &str) -> Result<Option<Session>, ServerError> {
        self.conn
            .query_row(&format!("SELECT {SESSION_COLS} FROM sessions WHERE session_id=?1"), params![id], session_row)
            .optional()
            .map_err(db)
    }

    pub fn sessions(&self, node: Option<&str>) -> Result<Vec<Session>, ServerError> {
        let mut stmt = self
            .conn
            .prepare(&format!("SELECT {SESSION_COLS} FROM sessions WHERE (?1 IS NULL OR node_id=?1) AND state<>'archived' ORDER BY updated_at_ms DESC"))
            .map_err(db)?;
        let rows = stmt.query_map(params![node], session_row).map_err(db)?;
        rows.collect::<Result<_, _>>().map_err(db)
    }

    // ---- runs ----

    /// Bind a task to a recorded session. The task moves to `in_progress`.
    pub fn start_run(&self, task_id: i64, session_id: &str) -> Result<Result<Run, Refused>, ServerError> {
        let Some(session) = self.session(session_id)? else { return Ok(Err(Refused::Invalid("unknown session"))) };
        let Some(task) = self.get(task_id)? else { return Ok(Err(Refused::NotFound)) };
        if self.active_run(task_id)?.is_some() {
            return Ok(Err(Refused::Conflict("task has an active run")));
        }
        if let Err(e) = self.move_to(task_id, "in_progress", Some(task.revision))? {
            return Ok(Err(e));
        }
        let now = unix_ms();
        self.conn
            .execute(
                "INSERT INTO runs (task_id,node_id,session_id,status,created_at_ms,updated_at_ms) VALUES (?1,?2,?3,'running',?4,?4)",
                params![task_id, session.node_id, session_id, now],
            )
            .map_err(db)?;
        Ok(Ok(self.run(self.conn.last_insert_rowid())?.expect("run")))
    }

    pub fn run(&self, id: i64) -> Result<Option<Run>, ServerError> {
        self.conn.query_row(&format!("SELECT {RUN_COLS} FROM runs WHERE id=?1"), params![id], run_row).optional().map_err(db)
    }

    pub fn runs(&self, task_id: i64) -> Result<Vec<Run>, ServerError> {
        let mut stmt = self.conn.prepare(&format!("SELECT {RUN_COLS} FROM runs WHERE task_id=?1 ORDER BY id")).map_err(db)?;
        let rows = stmt.query_map(params![task_id], run_row).map_err(db)?;
        rows.collect::<Result<_, _>>().map_err(db)
    }

    pub fn active_run(&self, task_id: i64) -> Result<Option<Run>, ServerError> {
        self.conn
            .query_row(&format!("SELECT {RUN_COLS} FROM runs WHERE task_id=?1 AND status IN ('running','review')"), params![task_id], run_row)
            .optional()
            .map_err(db)
    }

    /// Close (or move to review) a run. `by` is `session` (the run's own session, via m4a)
    /// or `operator` (override). A session may not reopen or touch a closed run; the
    /// operator may override any non-cancelled run. The task follows the run.
    pub fn close_run(
        &self,
        run_id: i64,
        status: &str,
        by: &str,
        caller_session: Option<&str>,
        branch: Option<&str>,
        commit_id: Option<&str>,
        result_message_id: Option<&str>,
    ) -> Result<Result<Run, Refused>, ServerError> {
        if !matches!(status, "review" | "done" | "failed" | "cancelled") {
            return Ok(Err(Refused::Invalid("bad run status")));
        }
        let Some(run) = self.run(run_id)? else { return Ok(Err(Refused::NotFound)) };
        match by {
            "session" => {
                if caller_session != Some(run.session_id.as_str()) {
                    return Ok(Err(Refused::Conflict("not this run's session")));
                }
                if !matches!(run.status.as_str(), "running" | "review") {
                    return Ok(Err(Refused::Conflict("run is closed")));
                }
            }
            "operator" => {
                if run.status == "cancelled" && status != "cancelled" {
                    return Ok(Err(Refused::Conflict("run was cancelled")));
                }
            }
            _ => return Ok(Err(Refused::Invalid("bad closer"))),
        }
        self.conn
            .execute(
                "UPDATE runs SET status=?2, closed_by=?3, branch=COALESCE(?4,branch), commit_id=COALESCE(?5,commit_id),
                 result_message_id=COALESCE(?6,result_message_id), updated_at_ms=?7 WHERE id=?1",
                params![run_id, status, by, branch, commit_id, result_message_id, unix_ms()],
            )
            .map_err(db)?;
        // Task follows: review -> review, done/failed/cancelled -> same (operator may reopen via move).
        let task_state = status;
        let task = self.get(run.task_id)?.expect("task");
        if FINAL_TASK_STATES.contains(&task.status.as_str()) && by == "operator" {
            self.conn
                .execute("UPDATE tasks SET status=?2, revision=revision+1, updated_at_ms=?3 WHERE id=?1", params![task.id, task_state, unix_ms()])
                .map_err(db)?;
        } else {
            let _ = self.move_to(task.id, task_state, None)?;
        }
        Ok(Ok(self.run(run_id)?.expect("run")))
    }
}

#[derive(Clone, Debug, Serialize, Eq, PartialEq)]
pub struct SessionSignal {
    pub id: i64,
    pub node_id: String,
    pub workspace_id: String,
    pub instance_id: i64,
    pub provider: String,
    /// `limit` | `login_required` | `ready`
    pub kind: String,
    pub hint: Option<String>,
    pub at_ms: u64,
}

pub const SIGNAL_KINDS: &[&str] = &["limit", "login_required", "ready"];
const MAX_SIGNALS: i64 = 5000;

impl TaskStore {
    /// Record a short session signal from a node (state changes only; bounded table).
    pub fn add_signal(&self, node_id: &str, workspace_id: &str, instance_id: i64, provider: &str, kind: &str, hint: Option<&str>) -> Result<Result<SessionSignal, Refused>, ServerError> {
        if !SIGNAL_KINDS.contains(&kind) {
            return Ok(Err(Refused::Invalid("unknown signal kind")));
        }
        let hint = hint.map(|h| h.chars().filter(|c| !c.is_control()).take(160).collect::<String>());
        self.conn
            .execute(
                "INSERT INTO signals (node_id,workspace_id,instance_id,provider,kind,hint,at_ms) VALUES (?1,?2,?3,?4,?5,?6,?7)",
                params![node_id, workspace_id, instance_id, provider, kind, hint, unix_ms()],
            )
            .map_err(db)?;
        let id = self.conn.last_insert_rowid();
        self.conn.execute("DELETE FROM signals WHERE id <= ?1", params![id - MAX_SIGNALS]).map_err(db)?;
        Ok(Ok(self.signals(None, Some(id - 1), 1)?.remove(0)))
    }

    /// Signals after `after_id` (oldest first), optionally for one node.
    pub fn signals(&self, node: Option<&str>, after_id: Option<i64>, limit: i64) -> Result<Vec<SessionSignal>, ServerError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id,node_id,workspace_id,instance_id,provider,kind,hint,at_ms FROM signals WHERE (?1 IS NULL OR node_id=?1) AND id>?2 ORDER BY id LIMIT ?3")
            .map_err(db)?;
        let rows = stmt
            .query_map(params![node, after_id.unwrap_or(0), limit.clamp(1, 500)], |r| {
                Ok(SessionSignal {
                    id: r.get(0)?,
                    node_id: r.get(1)?,
                    workspace_id: r.get(2)?,
                    instance_id: r.get(3)?,
                    provider: r.get(4)?,
                    kind: r.get(5)?,
                    hint: r.get(6)?,
                    at_ms: r.get::<_, i64>(7)? as u64,
                })
            })
            .map_err(db)?;
        rows.collect::<Result<_, _>>().map_err(db)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sess(store: &TaskStore, id: &str) {
        store
            .upsert_session(&Session {
                session_id: id.into(),
                node_id: "alpha".into(),
                provider: "claude".into(),
                workspace_id: Some("primary".into()),
                role: None,
                state: "live".into(),
                created_at_ms: 0,
                updated_at_ms: 0,
            })
            .unwrap();
    }

    #[test]
    fn lifecycle_and_ownership() {
        let s = TaskStore::in_memory().unwrap();
        let t = s.create("build", "run it", None).unwrap();
        assert_eq!(t.status, "open");
        assert!(s.assign(t.id, "alpha").unwrap());
        assert_eq!(s.list(Some("alpha"), true).unwrap().len(), 1);
        assert!(s.list(Some("beta"), true).unwrap().is_empty());
        assert!(!s.set_status(t.id, "done", Some("beta")).unwrap());
        assert!(s.set_status(t.id, "in_progress", Some("alpha")).unwrap());
        assert!(s.set_status(t.id, "done", Some("alpha")).unwrap());
        assert!(!s.set_status(t.id, "open", None).unwrap());
        assert!(!s.assign(t.id, "beta").unwrap());
        assert!(s.list(Some("alpha"), true).unwrap().is_empty());
        assert_eq!(s.list(None, false).unwrap().len(), 1);
    }

    #[test]
    fn create_move_cancel_with_revisions() {
        let s = TaskStore::in_memory().unwrap();
        let t = s.create_full("port", "kernel", None, Some("dev"), None, Some("backlog")).unwrap();
        assert_eq!((t.status.as_str(), t.revision), ("backlog", 1));
        let t2 = s.move_to(t.id, "open", Some(1)).unwrap().unwrap();
        assert_eq!(t2.revision, 2);
        assert_eq!(s.move_to(t.id, "assigned", Some(1)).unwrap().unwrap_err(), Refused::Conflict("stale revision"));
        assert_eq!(s.move_to(t.id, "bogus", None).unwrap().unwrap_err(), Refused::Invalid("unknown status"));
        let t3 = s.move_to(t.id, "cancelled", Some(2)).unwrap().unwrap();
        assert_eq!(t3.status, "cancelled");
        assert_eq!(s.archive_task(t.id).unwrap().unwrap().status, "archived");
        assert!(s.list_filtered(None, None, false).unwrap().iter().all(|x| x.id != t.id));
        assert_eq!(s.move_to(t.id, "open", Some(3)).unwrap().unwrap_err(), Refused::Conflict("task is final"));
    }

    #[test]
    fn role_filter() {
        let s = TaskStore::in_memory().unwrap();
        s.create_full("a", "", None, Some("dev"), None, None).unwrap();
        s.create_full("b", "", None, Some("advisor"), None, None).unwrap();
        s.create_full("c", "", None, None, None, None).unwrap();
        let dev = s.list_filtered(None, Some(&["dev".into()]), false).unwrap();
        assert_eq!(dev.iter().map(|t| t.title.as_str()).collect::<Vec<_>>(), ["a", "c"]);
    }

    #[test]
    fn runs_close_by_session_and_operator_override() {
        let s = TaskStore::in_memory().unwrap();
        sess(&s, "conv-1");
        let t = s.create("t", "", Some("alpha")).unwrap();
        assert_eq!(s.start_run(t.id, "nope").unwrap().unwrap_err(), Refused::Invalid("unknown session"));
        let r = s.start_run(t.id, "conv-1").unwrap().unwrap();
        assert_eq!(s.get(t.id).unwrap().unwrap().status, "in_progress");
        assert_eq!(s.start_run(t.id, "conv-1").unwrap().unwrap_err(), Refused::Conflict("task has an active run"));
        // Another session cannot close it.
        assert!(s.close_run(r.id, "done", "session", Some("conv-2"), None, None, None).unwrap().is_err());
        let r = s.close_run(r.id, "review", "session", Some("conv-1"), Some("feat/x"), Some("abc123"), Some("m-1")).unwrap().unwrap();
        assert_eq!((r.status.as_str(), r.branch.as_deref()), ("review", Some("feat/x")));
        let r = s.close_run(r.id, "done", "session", Some("conv-1"), None, None, None).unwrap().unwrap();
        assert_eq!(s.get(t.id).unwrap().unwrap().status, "done");
        assert!(s.close_run(r.id, "failed", "session", Some("conv-1"), None, None, None).unwrap().is_err());
        // Operator override of a closed run.
        let r = s.close_run(r.id, "failed", "operator", None, None, None, None).unwrap().unwrap();
        assert_eq!((r.status.as_str(), r.closed_by.as_deref()), ("failed", Some("operator")));
        assert_eq!(s.get(t.id).unwrap().unwrap().status, "failed");
        assert_eq!(s.runs(t.id).unwrap().len(), 1);
    }

    #[test]
    fn signals_store_and_page() {
        let s = TaskStore::in_memory().unwrap();
        assert_eq!(s.add_signal("a", "w", 1, "claude", "bogus", None).unwrap().unwrap_err(), Refused::Invalid("unknown signal kind"));
        let x = s.add_signal("a", "w", 1, "claude", "limit", Some("resets 3pm\x1b")).unwrap().unwrap();
        assert_eq!(x.hint.as_deref(), Some("resets 3pm"));
        s.add_signal("b", "w", 2, "codex", "ready", None).unwrap().unwrap();
        assert_eq!(s.signals(Some("a"), None, 10).unwrap().len(), 1);
        assert_eq!(s.signals(None, Some(x.id), 10).unwrap()[0].node_id, "b");
    }

    #[test]
    fn op_ids_replay() {
        let s = TaskStore::in_memory().unwrap();
        assert!(s.replay(Some("op-1")).unwrap().is_none());
        s.record(Some("op-1"), &serde_json::json!({"task": 1})).unwrap();
        assert_eq!(s.replay(Some("op-1")).unwrap().unwrap()["task"], 1);
    }

    #[test]
    fn migrates_p6_table_and_survives_reopen() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("tasks")).unwrap();
        let old = Connection::open(dir.path().join("tasks/tasks.db")).unwrap();
        old.execute_batch("CREATE TABLE tasks (id INTEGER PRIMARY KEY AUTOINCREMENT, title TEXT NOT NULL, body TEXT NOT NULL, assignee TEXT, status TEXT NOT NULL, created_at_ms INTEGER NOT NULL, updated_at_ms INTEGER NOT NULL);
            INSERT INTO tasks (title,body,assignee,status,created_at_ms,updated_at_ms) VALUES ('old','b','alpha','assigned',1,1);").unwrap();
        drop(old);
        let s = TaskStore::open(dir.path()).unwrap();
        let t = &s.list(Some("alpha"), true).unwrap()[0];
        assert_eq!((t.title.as_str(), t.revision, t.role.clone()), ("old", 1, None));
        drop(s);
        assert_eq!(TaskStore::open(dir.path()).unwrap().list(None, false).unwrap().len(), 1);
    }
}
