//! Global SharedMemory for cross-L2 coordination.
//!
//! Wraps the core data with parking_lot RwLock + file persistence.
//! L2 coordinators read/write namespaced knowledge ("L2.0.auth.method").

use super::types::*;
use chrono::Utc;
use parking_lot::RwLock;
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Thread-safe global shared memory.
#[derive(Clone)]
pub struct GlobalMemory {
    state: Arc<RwLock<GlobalState>>,
    persist_path: Option<PathBuf>,
}

impl GlobalMemory {
    pub fn new(persist_path: Option<PathBuf>, opus_model: &str) -> Self {
        let state = GlobalState {
            version: 1,
            knowledge: HashMap::new(),
            l2_status: HashMap::new(),
            messages: VecDeque::new(),
            metadata: GlobalMetadata {
                l2_count: 0,
                opus_model: opus_model.to_string(),
                created_at: Utc::now(),
                updated_at: Utc::now(),
            },
        };
        Self {
            state: Arc::new(RwLock::new(state)),
            persist_path,
        }
    }

    pub fn load(path: &Path, opus_model: &str) -> Self {
        let state = std::fs::read_to_string(path)
            .ok()
            .and_then(|json| serde_json::from_str::<GlobalState>(&json).ok())
            .unwrap_or_else(|| GlobalState {
                version: 1,
                knowledge: HashMap::new(),
                l2_status: HashMap::new(),
                messages: VecDeque::new(),
                metadata: GlobalMetadata {
                    l2_count: 0,
                    opus_model: opus_model.to_string(),
                    created_at: Utc::now(),
                    updated_at: Utc::now(),
                },
            });
        Self {
            state: Arc::new(RwLock::new(state)),
            persist_path: Some(path.to_path_buf()),
        }
    }

    // ── Knowledge ────────────────────────────────────────────────────

    /// Set knowledge. Key should be namespaced: "L2.0.auth.method"
    pub fn set_knowledge(&self, key: String, value: serde_json::Value) {
        let mut s = self.state.write();
        s.knowledge.insert(key, value);
        s.metadata.updated_at = Utc::now();
        drop(s);
        self.persist();
    }

    /// Get all knowledge matching prefix (e.g. "L2.0.*").
    pub fn get_knowledge(&self, prefix: &str) -> HashMap<String, serde_json::Value> {
        let s = self.state.read();
        let prefix = prefix.trim_end_matches('*');
        s.knowledge
            .iter()
            .filter(|(k, _)| k.starts_with(prefix))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    /// Get all knowledge as formatted summary.
    pub fn knowledge_summary(&self) -> String {
        let s = self.state.read();
        if s.knowledge.is_empty() {
            return "(none)".to_string();
        }
        s.knowledge
            .iter()
            .map(|(k, v)| format!("  {} = {}", k, v))
            .collect::<Vec<_>>()
            .join("\n")
    }

    // ── L2 Status ────────────────────────────────────────────────────

    pub fn update_l2_status(&self, status: L2Status) {
        let mut s = self.state.write();
        s.l2_status.insert(status.l2_id, status);
        s.metadata.updated_at = Utc::now();
        drop(s);
        self.persist();
    }

    pub fn get_l2_status(&self, l2_id: L2Id) -> Option<L2Status> {
        self.state.read().l2_status.get(&l2_id).cloned()
    }

    pub fn all_l2_status(&self) -> Vec<L2Status> {
        self.state.read().l2_status.values().cloned().collect()
    }

    pub fn set_l2_count(&self, count: usize) {
        let mut s = self.state.write();
        s.metadata.l2_count = count;
        s.metadata.updated_at = Utc::now();
    }

    // ── Messages ─────────────────────────────────────────────────────

    pub fn send_message(&self, from: L2Id, to: CrossTarget, text: String) {
        let mut s = self.state.write();
        s.messages.push_back(CrossMessage {
            from,
            to,
            text,
            timestamp: Utc::now(),
        });
        while s.messages.len() > 50 {
            s.messages.pop_front();
        }
        s.metadata.updated_at = Utc::now();
        drop(s);
        self.persist();
    }

    pub fn drain_messages_for_l2(&self, l2_id: L2Id) -> Vec<CrossMessage> {
        let mut s = self.state.write();
        let (mine, others): (VecDeque<_>, VecDeque<_>) =
            s.messages.drain(..).partition(|m| match m.to {
                CrossTarget::L2(id) => id == l2_id,
                CrossTarget::AllL2s => true,
                CrossTarget::OpusManager => false,
            });
        s.messages = others;
        mine.into_iter().collect()
    }

    // ── Progress ─────────────────────────────────────────────────────

    /// Aggregate: (total_completed, total_tasks) across all L2s.
    pub fn aggregate_progress(&self) -> (usize, usize) {
        let s = self.state.read();
        let done: usize = s.l2_status.values().map(|l| l.tasks_completed).sum();
        let total: usize = s.l2_status.values().map(|l| l.tasks_total).sum();
        (done, total)
    }

    /// Check if all L2s completed.
    pub fn all_complete(&self) -> bool {
        let s = self.state.read();
        !s.l2_status.is_empty()
            && s.l2_status.values().all(|l| l.state == L2State::Completed)
    }

    // ── Snapshot ─────────────────────────────────────────────────────

    pub fn snapshot(&self) -> GlobalState {
        self.state.read().clone()
    }

    // ── Persistence ──────────────────────────────────────────────────

    fn persist(&self) {
        let Some(path) = &self.persist_path else { return };
        let s = self.state.read();
        let Ok(json) = serde_json::to_string_pretty(&*s) else { return };
        drop(s);
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, &json).is_ok() {
            let _ = std::fs::rename(&tmp, path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_global_knowledge() {
        let gm = GlobalMemory::new(None, "opus");
        gm.set_knowledge("L2.0.auth.method".into(), serde_json::json!("bearer"));
        gm.set_knowledge("L2.1.api.url".into(), serde_json::json!("https://example.com"));

        let l2_0 = gm.get_knowledge("L2.0.*");
        assert_eq!(l2_0.len(), 1);
        assert_eq!(l2_0["L2.0.auth.method"], serde_json::json!("bearer"));

        let all = gm.get_knowledge("L2.*");
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn test_l2_status() {
        let gm = GlobalMemory::new(None, "opus");
        gm.update_l2_status(L2Status {
            l2_id: 0,
            name: "Auth".into(),
            tasks_completed: 3,
            tasks_total: 10,
            workers_active: 2,
            workers_total: 4,
            elapsed_secs: 120,
            last_update: Utc::now(),
            state: L2State::Running,
        });
        gm.update_l2_status(L2Status {
            l2_id: 1,
            name: "API".into(),
            tasks_completed: 5,
            tasks_total: 5,
            workers_active: 0,
            workers_total: 3,
            elapsed_secs: 200,
            last_update: Utc::now(),
            state: L2State::Completed,
        });

        let (done, total) = gm.aggregate_progress();
        assert_eq!(done, 8);
        assert_eq!(total, 15);
        assert!(!gm.all_complete());
    }

    #[test]
    fn test_cross_messages() {
        let gm = GlobalMemory::new(None, "opus");
        gm.send_message(0, CrossTarget::L2(1), "hello from L2.0".into());
        gm.send_message(1, CrossTarget::AllL2s, "broadcast".into());

        let msgs = gm.drain_messages_for_l2(1);
        assert_eq!(msgs.len(), 2); // direct + broadcast
    }
}
