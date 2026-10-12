//! P6: the hub behind the server control API: node calls forwarded by C2 and operator (HQ)
//! calls for mail and tasks. All state lives under the data dir; admission is always read from
//! the P2 ledger, never from the request.

use crate::messenger::{Messenger, MessengerError, OPERATOR};
use crate::provision::{read_ledger, LedgerEntry, NodeStatus};
use crate::tasks::TaskStore;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

#[derive(Debug)]
pub struct HubError {
    pub status: u16,
    pub message: String,
}

impl HubError {
    fn new(status: u16, message: impl Into<String>) -> Self {
        Self { status, message: message.into() }
    }
}

impl From<MessengerError> for HubError {
    fn from(e: MessengerError) -> Self {
        match e {
            MessengerError::Forbidden => Self::new(403, "refused"),
            MessengerError::Bad(m) => Self::new(400, m),
            MessengerError::Internal(m) => Self::new(500, m),
        }
    }
}

pub struct Hub {
    data_dir: PathBuf,
    messenger: Mutex<Messenger>,
    tasks: Mutex<TaskStore>,
    /// Bumped on every message sent; lets HQ long-poll for new mail instead of polling blindly.
    mail_signal: (Mutex<u64>, Condvar),
}

fn str_field<'a>(v: &'a Value, name: &str) -> Result<&'a str, HubError> {
    v.get(name).and_then(Value::as_str).ok_or_else(|| HubError::new(400, format!("missing field: {name}")))
}

impl Hub {
    pub fn open(data_dir: &Path) -> Result<Self, crate::error::ServerError> {
        Ok(Self {
            data_dir: data_dir.to_owned(),
            messenger: Mutex::new(Messenger::open(data_dir)?),
            tasks: Mutex::new(TaskStore::open(data_dir)?),
            mail_signal: (Mutex::new(0), Condvar::new()),
        })
    }

    fn bump_mail(&self) {
        *self.mail_signal.0.lock().expect("signal") += 1;
        self.mail_signal.1.notify_all();
    }

    fn entry(&self, node_id: &str) -> Option<LedgerEntry> {
        read_ledger(&self.data_dir).ok()?.into_iter().find(|e| e.node_id == node_id)
    }

    /// Active (member/trusted) node holding `capability`, per the ledger.
    fn admitted(&self, node_id: &str, capability: &str) -> bool {
        node_id != OPERATOR
            && self.entry(node_id).is_some_and(|e| {
                matches!(e.status, NodeStatus::Member | NodeStatus::Trusted) && e.grants.capabilities.iter().any(|c| c == capability)
            })
    }

    /// A call made on behalf of a node. The caller (C2) has authenticated the node and checked its
    /// policy; the hub checks the ledger again, independently.
    pub fn node_call(&self, node_id: &str, op: &str, payload: &Value) -> Result<Value, HubError> {
        let capability = match op.split_once('.').map(|p| p.0) {
            Some("mail") => "mail",
            Some("task") => "tasks",
            _ => return Err(HubError::new(404, "unknown operation")),
        };
        if !self.admitted(node_id, capability) {
            return Err(HubError::new(403, "refused"));
        }
        match op {
            "mail.send" => {
                let key = payload.get("idempotency_key").and_then(Value::as_str).map(str::to_owned);
                let id = self.messenger.lock().expect("messenger").send(node_id, str_field(payload, "to")?, str_field(payload, "subject")?, str_field(payload, "body")?, key)?;
                self.bump_mail();
                Ok(json!({ "message_id": id }))
            }
            "mail.inbox" => {
                let since = payload.get("since_ms").and_then(Value::as_u64).unwrap_or(0);
                let limit = payload.get("limit").and_then(Value::as_u64).unwrap_or(50).min(256) as u16;
                let mail = self.messenger.lock().expect("messenger").inbox(node_id, since, limit)?;
                Ok(json!({ "messages": mail }))
            }
            "mail.ack" => {
                self.messenger.lock().expect("messenger").ack(node_id, str_field(payload, "message_id")?)?;
                Ok(json!({ "ok": true }))
            }
            "task.list" => {
                let tasks = self.tasks.lock().expect("tasks").list(Some(node_id), true).map_err(|e| HubError::new(500, e.to_string()))?;
                Ok(json!({ "tasks": tasks }))
            }
            "task.update" => {
                let id = payload.get("id").and_then(Value::as_i64).ok_or_else(|| HubError::new(400, "missing field: id"))?;
                let status = str_field(payload, "status")?;
                if !matches!(status, "in_progress" | "done") {
                    return Err(HubError::new(400, "status must be in_progress or done"));
                }
                let changed = self.tasks.lock().expect("tasks").set_status(id, status, Some(node_id)).map_err(|e| HubError::new(500, e.to_string()))?;
                if changed { Ok(json!({ "ok": true })) } else { Err(HubError::new(403, "refused")) }
            }
            _ => Err(HubError::new(404, "unknown operation")),
        }
    }

    /// Operator (HQ) calls, authenticated by the control bearer in the HTTP layer.
    pub fn operator_call(&self, method: &str, path: &str, body: &Value) -> Result<Value, HubError> {
        let (path, query) = path.split_once('?').unwrap_or((path, ""));
        let parts: Vec<&str> = path.trim_matches('/').split('/').collect();
        match (method, parts.as_slice()) {
            ("POST", ["v1", "tasks"]) => {
                let assignee = body.get("assignee").and_then(Value::as_str);
                if let Some(node) = assignee {
                    if !self.admitted(node, "tasks") {
                        return Err(HubError::new(403, "assignee does not hold the tasks capability"));
                    }
                }
                let task = self.tasks.lock().expect("tasks").create(str_field(body, "title")?, body.get("body").and_then(Value::as_str).unwrap_or(""), assignee).map_err(|e| HubError::new(500, e.to_string()))?;
                Ok(json!({ "task": task }))
            }
            ("GET", ["v1", "tasks"]) => {
                let assignee = query.split('&').find_map(|kv| kv.strip_prefix("assignee="));
                let tasks = self.tasks.lock().expect("tasks").list(assignee, false).map_err(|e| HubError::new(500, e.to_string()))?;
                Ok(json!({ "tasks": tasks }))
            }
            ("POST", ["v1", "tasks", id, "assign"]) => {
                let id: i64 = id.parse().map_err(|_| HubError::new(400, "bad id"))?;
                let node = str_field(body, "assignee")?;
                if !self.admitted(node, "tasks") {
                    return Err(HubError::new(403, "assignee does not hold the tasks capability"));
                }
                if self.tasks.lock().expect("tasks").assign(id, node).map_err(|e| HubError::new(500, e.to_string()))? { Ok(json!({ "ok": true })) } else { Err(HubError::new(409, "task cannot be assigned")) }
            }
            ("POST", ["v1", "tasks", id, "cancel"]) => {
                let id: i64 = id.parse().map_err(|_| HubError::new(400, "bad id"))?;
                if self.tasks.lock().expect("tasks").set_status(id, "cancelled", None).map_err(|e| HubError::new(500, e.to_string()))? { Ok(json!({ "ok": true })) } else { Err(HubError::new(409, "task is final")) }
            }
            ("POST", ["v1", "mail", "send"]) => {
                let id = self.messenger.lock().expect("messenger").send(OPERATOR, str_field(body, "to")?, str_field(body, "subject")?, str_field(body, "body")?, None)?;
                self.bump_mail();
                Ok(json!({ "message_id": id }))
            }
            // Long-poll for HQ: returns as soon as the operator inbox holds a message newer than
            // `since_ms`, or after `timeout_secs` (max 30) with an empty list.
            ("POST", ["v1", "mail", "wait"]) => {
                let since = body.get("since_ms").and_then(Value::as_u64).unwrap_or(0);
                let deadline = Instant::now() + Duration::from_secs(body.get("timeout_secs").and_then(Value::as_u64).unwrap_or(25).min(30));
                loop {
                    let seen = *self.mail_signal.0.lock().expect("signal");
                    let mail: Vec<_> = self.messenger.lock().expect("messenger").inbox(OPERATOR, since, 100)?.into_iter().filter(|m| m.created_at_ms > since).collect();
                    if !mail.is_empty() {
                        return Ok(json!({ "messages": mail }));
                    }
                    let now = Instant::now();
                    if now >= deadline {
                        return Ok(json!({ "messages": [] }));
                    }
                    let guard = self.mail_signal.0.lock().expect("signal");
                    if *guard == seen {
                        let _ = self.mail_signal.1.wait_timeout(guard, deadline - now).expect("signal wait");
                    }
                }
            }
            ("POST", ["v1", "mail", "inbox"]) => {
                let mail = self.messenger.lock().expect("messenger").inbox(OPERATOR, 0, 100)?;
                Ok(json!({ "messages": mail }))
            }
            _ => Err(HubError::new(404, "not found")),
        }
    }
}
