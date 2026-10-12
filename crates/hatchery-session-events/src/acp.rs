//! ACP adapter: maps `session/update` notifications of the Agent Client Protocol (v1 and v2 names)
//! onto the same [`SessionEvent`] type. Confidence is `Structured`: these fields are what the agent said.
//! Used as an extra scenario next to PTY parsing, e.g. to run a provider headless and get typed
//! replies into a turn.

use crate::event::*;
use serde_json::Value;
use std::collections::HashMap;

#[derive(Default)]
pub struct AcpAdapter {
    provider: String,
    ids: HashMap<String, (u64, u32)>,
    next_id: u64,
    texts: HashMap<String, String>,
    tools: HashMap<String, (String, String, Option<String>, ActionStatus)>,
}

impl AcpAdapter {
    pub fn new(provider: &str) -> Self {
        Self { provider: provider.to_owned(), next_id: 1, ..Default::default() }
    }

    fn id(&mut self, key: &str) -> (u64, u32) {
        let next = self.next_id;
        let e = self.ids.entry(key.to_owned()).or_insert((next, 0));
        if e.1 == 0 && e.0 == next { self.next_id += 1; }
        e.1 += 1;
        *e
    }

    fn ev(&mut self, key: &str, ts_ms: u64, event: EventKind) -> SessionEvent {
        let (id, rev) = self.id(key);
        SessionEvent { id, rev, provider: self.provider.clone(), source: "acp".into(), confidence: Confidence::Structured, ts_ms, event }
    }

    /// Feed one JSON-RPC notification (`{"method":"session/update","params":{...}}`) or just its `params`.
    pub fn observe(&mut self, msg: &Value, ts_ms: u64) -> Vec<SessionEvent> {
        let params = if msg.get("method").is_some() { &msg["params"] } else { msg };
        let u = &params["update"];
        let Some(kind) = u["sessionUpdate"].as_str() else { return vec![] };
        let text_of = |u: &Value| u["content"]["text"].as_str().map(str::to_owned);
        let msg_id = u["messageId"].as_str().unwrap_or("0").to_owned();
        match kind {
            "agent_message_chunk" | "agent_message" | "agent_thought_chunk" | "agent_thought" | "user_message_chunk" | "user_message" => {
                let k = format!("{}:{msg_id}", kind.split('_').take(2).collect::<Vec<_>>().join("_"));
                let t = self.texts.entry(k.clone()).or_default();
                let piece = text_of(u).unwrap_or_default();
                if kind.ends_with("chunk") { t.push_str(&piece) } else { *t = piece }
                let text = t.clone();
                let event = if kind.starts_with("agent_thought") { EventKind::Thought { text } } else if kind.starts_with("user") { EventKind::UserMessage { text } } else { EventKind::Answer { text } };
                vec![self.ev(&k, ts_ms, event)]
            }
            "tool_call" | "tool_call_update" => {
                let tid = u["toolCallId"].as_str().unwrap_or("?").to_owned();
                let entry = self.tools.entry(tid.clone()).or_insert((String::new(), String::new(), None, ActionStatus::Running));
                if let Some(t) = u["title"].as_str() { entry.0 = t.to_owned(); }
                if let Some(k) = u["kind"].as_str() { entry.1 = k.to_owned(); }
                match u["status"].as_str() { Some("completed") => entry.3 = ActionStatus::Done, Some("failed") => entry.3 = ActionStatus::Failed, Some(_) => entry.3 = ActionStatus::Running, None => {} }
                let mut out_text = None;
                let mut diffs = Vec::new();
                for c in u["content"].as_array().cloned().unwrap_or_default() {
                    match c["type"].as_str() {
                        Some("content") => out_text = c["content"]["text"].as_str().map(str::to_owned),
                        Some("diff") => {
                            let patch = c["patch"]["text"].as_str().map(str::to_owned).unwrap_or_else(|| format!("--- {}\n+++ {}\n-{}\n+{}", c["path"].as_str().unwrap_or(""), c["path"].as_str().unwrap_or(""), c["oldText"].as_str().unwrap_or(""), c["newText"].as_str().unwrap_or("")));
                            let files: Vec<String> = c["changes"].as_array().map(|a| a.iter().filter_map(|x| x["path"].as_str().map(str::to_owned)).collect()).unwrap_or_else(|| c["path"].as_str().map(|p| vec![p.to_owned()]).unwrap_or_default());
                            diffs.push((files, patch));
                        }
                        _ => {}
                    }
                }
                if out_text.is_some() { entry.2 = out_text; }
                let (name, args, result, status) = (entry.0.clone(), entry.1.clone(), entry.2.clone(), entry.3);
                let mut out = vec![self.ev(&format!("tool:{tid}"), ts_ms, EventKind::Action { name, args, result, status })];
                for (i, (files, patch)) in diffs.into_iter().enumerate() {
                    out.push(self.ev(&format!("diff:{tid}:{i}"), ts_ms, EventKind::Diff { files, patch }));
                }
                out
            }
            "state_update" => {
                let s = u["state"].as_str().unwrap_or("idle");
                let state = match s { "running" => SessionState::Working, "requires_action" => SessionState::NeedsInput, _ => SessionState::Idle };
                vec![self.ev("state", ts_ms, EventKind::State { state, detail: s.to_owned() })]
            }
            _ => vec![],
        }
    }
}
