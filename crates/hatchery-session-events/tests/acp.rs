//! ACP adapter tests with the update shapes from the Agent Client Protocol documentation
//! (https://agentclientprotocol.com): chunked messages and thoughts, tool call upserts, diffs, state.
use hatchery_session_events::acp::AcpAdapter;
use hatchery_session_events::*;
use serde_json::json;

fn upd(u: serde_json::Value) -> serde_json::Value { json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s","update":u}}) }

#[test]
fn chunks_accumulate_into_one_item_with_growing_revisions() {
    let mut a = AcpAdapter::new("kimi");
    let e1 = a.observe(&upd(json!({"sessionUpdate":"agent_message_chunk","messageId":"m1","content":{"type":"text","text":"Hel"}})), 1);
    let e2 = a.observe(&upd(json!({"sessionUpdate":"agent_message_chunk","messageId":"m1","content":{"type":"text","text":"lo"}})), 2);
    assert_eq!(e1[0].id, e2[0].id);
    assert_eq!(e2[0].rev, 2);
    assert_eq!(e2[0].event, EventKind::Answer { text: "Hello".into() });
    assert_eq!(e2[0].confidence, Confidence::Structured);
    assert_eq!(e2[0].source, "acp");
}

#[test]
fn thought_tool_call_diff_and_state() {
    let mut a = AcpAdapter::new("kimi");
    let t = a.observe(&upd(json!({"sessionUpdate":"agent_thought_chunk","messageId":"t1","content":{"type":"text","text":"check config"}})), 1);
    assert_eq!(t[0].event, EventKind::Thought { text: "check config".into() });
    let c1 = a.observe(&upd(json!({"sessionUpdate":"tool_call","toolCallId":"c1","title":"Edit config","kind":"edit","status":"pending"})), 2);
    let c2 = a.observe(&upd(json!({"sessionUpdate":"tool_call_update","toolCallId":"c1","status":"completed","content":[
        {"type":"diff","path":"/p/config.json","oldText":"{\"debug\":false}","newText":"{\"debug\":true}"},
        {"type":"content","content":{"type":"text","text":"ok"}}]})), 3);
    assert_eq!(c1[0].id, c2[0].id);
    assert!(matches!(&c2[0].event, EventKind::Action { name, args, result: Some(r), status: ActionStatus::Done } if name == "Edit config" && args == "edit" && r == "ok"));
    assert!(matches!(&c2[1].event, EventKind::Diff { files, patch } if files == &["/p/config.json"] && patch.contains("+{\"debug\":true}")));
    let s = a.observe(&upd(json!({"sessionUpdate":"state_update","state":"requires_action"})), 4);
    assert!(matches!(&s[0].event, EventKind::State { state: SessionState::NeedsInput, .. }));
}

#[test]
fn v2_diff_with_git_patch_and_unknown_updates_are_ignored() {
    let mut a = AcpAdapter::new("kimi");
    let e = a.observe(&upd(json!({"sessionUpdate":"tool_call_update","toolCallId":"c","title":"Edit","kind":"edit","status":"in_progress","content":[{"type":"diff","changes":[{"operation":"modify","path":"/abs/file.txt"}],"patch":{"format":"git_patch","text":"diff --git a/f b/f\n+x"}}]})), 1);
    assert!(matches!(&e[1].event, EventKind::Diff { files, patch } if files == &["/abs/file.txt"] && patch.starts_with("diff --git")));
    assert!(a.observe(&upd(json!({"sessionUpdate":"usage_update"})), 2).is_empty());
}
