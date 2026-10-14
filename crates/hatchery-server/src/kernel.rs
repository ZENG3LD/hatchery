//! hq-core task kernel: the former harness task/run/delivery kernel and the observation
//! store, hosted in-process as libraries behind the hub (no separate `hatchery-harness`
//! process, no legacy C2 control socket).
//!
//! Increment 1 (this module): durable task kernel + observation store, operator task verbs
//! and read snapshots on the hub operator API under `/v1/kernel/...`. Run dispatch to nodes
//! (worktrees, scheduling, deliveries) goes over the core drain in later increments.

use hatchery_harness_protocol::{
    HarnessCancelTaskRequestV1, HarnessCreateTaskRequestV1, HarnessMoveTaskRequestV1,
    HarnessReplaceTaskRequestV1, HarnessRetryTaskRequestV1,
};
use hatchery_harness_service::HarnessService;
use hatchery_observation_service::ObservationService;
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use std::path::Path;

pub const KERNEL_DB: &str = "harness.db";
pub const OBSERVATION_DB: &str = "observation.db";

pub struct Kernel {
    harness: HarnessService,
    observation: ObservationService,
}

#[derive(Debug)]
pub struct KernelError {
    pub status: u16,
    pub message: String,
}

impl KernelError {
    fn new(status: u16, message: impl Into<String>) -> Self {
        Self { status, message: message.into() }
    }
}

fn parse<T: DeserializeOwned>(body: &Value) -> Result<T, KernelError> {
    serde_json::from_value(body.clone()).map_err(|e| KernelError::new(400, format!("bad request: {e}")))
}

impl Kernel {
    /// Open (or create) the kernel databases under the server data dir.
    pub fn open(data_dir: &Path) -> Result<Self, String> {
        let harness = HarnessService::open(data_dir.join(KERNEL_DB)).map_err(|e| format!("kernel db: {e}"))?;
        let observation =
            ObservationService::open(data_dir.join(OBSERVATION_DB)).map_err(|e| format!("observation db: {e}"))?;
        Ok(Self { harness, observation })
    }

    /// Operator verbs and reads. `parts` is the path after `/v1/kernel`.
    pub fn call(&mut self, method: &str, parts: &[&str], body: &Value) -> Result<Value, KernelError> {
        let applied = |r: Result<hatchery_harness_engine::HarnessApplyOutcome, hatchery_harness_service::HarnessServiceError>| {
            r.map(|o| json!({ "outcome": format!("{o:?}") })).map_err(|e| KernelError::new(409, e.to_string()))
        };
        match (method, parts) {
            ("GET", ["snapshot"]) => {
                let engine = self.harness.engine().checkpoint();
                let snap = self.harness.committed_snapshot();
                Ok(json!({
                    "engine": serde_json::to_value(&engine).map_err(|e| KernelError::new(500, e.to_string()))?,
                    "dispatch_contexts": serde_json::to_value(&snap.dispatch_contexts).map_err(|e| KernelError::new(500, e.to_string()))?,
                    "poisoned": self.harness.is_poisoned(),
                }))
            }
            ("GET", ["observation"]) => Ok(json!({
                "cursors": self.observation.durable_resume_cursors().iter()
                    .map(|(n, c)| json!({ "node": format!("{n:?}"), "cursor": format!("{c:?}") }))
                    .collect::<Vec<_>>(),
                "poisoned": self.observation.is_poisoned(),
            })),
            ("POST", ["tasks"]) => applied(self.harness.operator_create_task(parse::<HarnessCreateTaskRequestV1>(body)?)),
            ("POST", ["tasks", "replace"]) => applied(self.harness.operator_replace_task(parse::<HarnessReplaceTaskRequestV1>(body)?)),
            ("POST", ["tasks", "move"]) => applied(self.harness.operator_move_task(parse::<HarnessMoveTaskRequestV1>(body)?)),
            ("POST", ["tasks", "cancel"]) => applied(self.harness.operator_cancel_task(parse::<HarnessCancelTaskRequestV1>(body)?)),
            ("POST", ["tasks", "retry"]) => applied(self.harness.operator_retry_task(parse::<HarnessRetryTaskRequestV1>(body)?)),
            _ => Err(KernelError::new(404, "unknown kernel route")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn auth(hex: char, now: u64) -> Value {
        json!({
            "operation_id": format!("hop_{}", hex.to_string().repeat(24)),
            "idempotency_ref": format!("hidem_{}", hex.to_string().repeat(24)),
            "actor_id": "operator",
            "now_unix_ms": now,
        })
    }

    fn task<'a>(snap: &'a Value, id: &str) -> &'a Value {
        snap["engine"]["tasks"].as_array().unwrap().iter().find(|t| t["task_id"] == id).expect("task in snapshot")
    }

    #[test]
    fn task_create_move_cancel_over_the_api() {
        let dir = tempfile::tempdir().unwrap();
        let mut k = Kernel::open(dir.path()).unwrap();
        let id = format!("htask_{}", "b".repeat(24));
        let create = json!({
            "authority": auth('1', 10), "task_id": id, "title": "Port the kernel",
            "body": "hq-core owns tasks", "parent_task_id": null, "dependencies": [], "initial_state": "backlog",
        });
        k.call("POST", &["tasks"], &create).unwrap();
        // Same operation replays idempotently.
        k.call("POST", &["tasks"], &create).unwrap();
        let snap = k.call("GET", &["snapshot"], &Value::Null).unwrap();
        assert_eq!(task(&snap, &id)["state"], "backlog");
        let rev = task(&snap, &id)["revision"].clone();

        k.call("POST", &["tasks", "move"], &json!({
            "authority": auth('2', 20), "task_id": id, "expected_revision": rev, "state": "ready",
        })).unwrap();
        let snap = k.call("GET", &["snapshot"], &Value::Null).unwrap();
        assert_eq!(task(&snap, &id)["state"], "ready");
        let rev2 = task(&snap, &id)["revision"].clone();
        assert_ne!(rev, rev2);

        // Stale revision is refused.
        let stale = k.call("POST", &["tasks", "cancel"], &json!({
            "authority": auth('3', 30), "task_id": id, "expected_revision": rev,
        }));
        assert_eq!(stale.unwrap_err().status, 409);
        k.call("POST", &["tasks", "cancel"], &json!({
            "authority": auth('4', 40), "task_id": id, "expected_revision": rev2,
        })).unwrap();
        drop(k);
        // Durable across reopen.
        let mut k = Kernel::open(dir.path()).unwrap();
        let snap = k.call("GET", &["snapshot"], &Value::Null).unwrap();
        assert_eq!(task(&snap, &id)["state"], "cancelled");
    }

    #[test]
    fn opens_empty_kernel_and_reads_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let mut k = Kernel::open(dir.path()).unwrap();
        let snap = k.call("GET", &["snapshot"], &Value::Null).unwrap();
        assert_eq!(snap["poisoned"], false);
        assert!(snap["engine"].is_object());
        assert_eq!(k.call("GET", &["observation"], &Value::Null).unwrap()["poisoned"], false);
        assert_eq!(k.call("GET", &["nope"], &Value::Null).unwrap_err().status, 404);
        assert_eq!(k.call("POST", &["tasks"], &json!({"x":1})).unwrap_err().status, 400);
        drop(k);
        // Reopens the same durable files.
        Kernel::open(dir.path()).unwrap();
    }
}
