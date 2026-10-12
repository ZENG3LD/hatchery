//! SYNTHETIC screens in the publicly documented / observed output formats of each provider
//! (answer bullets, thinking lines, tool results, diffs, permission prompts). These are NOT real
//! captures: logged-in sessions cannot be recorded here. They pin the parser contract, and real
//! recordings should replace them as soon as logged-in captures exist.
use hatchery_session_events::*;

fn run(provider: &str, screens: &[&str]) -> Vec<SessionEvent> {
    let mut s = EventStream::new(provider);
    let mut ev = Vec::new();
    let mut t = 0;
    for sc in screens { ev.extend(s.observe_screen(sc, t)); t += 400; ev.extend(s.observe_screen(sc, t)); t += 400; }
    ev.extend(s.tick(t + 2000));
    ev
}
fn latest(ev: &[SessionEvent]) -> Vec<SessionEvent> {
    let mut m: std::collections::BTreeMap<u64, SessionEvent> = Default::default();
    for e in ev { m.insert(e.id, e.clone()); }
    m.into_values().collect()
}

const CLAUDE_TURN: &str = "\
> please edit the readme

∴ Thinking… the heading needs the project name

● Update(README.md)
  ⎿ Updated README.md with 2 additions and 1 removal
diff --git a/README.md b/README.md
@@ -1,3 +1,4 @@
-# Project
+# Project (managed)
 intro
+status: ok

● I updated the heading and added a status line.
  Tell me if you want more.

╭──────────────────────────────────────────╮
│ >                                        │
╰──────────────────────────────────────────╯
";

#[test]
fn claude_turn_yields_message_thought_action_diff_answer() {
    let ev = latest(&run("claude", &[CLAUDE_TURN]));
    let k: Vec<&EventKind> = ev.iter().map(|e| &e.event).collect();
    assert!(k.iter().any(|e| matches!(e, EventKind::UserMessage { text } if text == "please edit the readme")), "{k:#?}");
    assert!(k.iter().any(|e| matches!(e, EventKind::Thought { text } if text.contains("heading needs"))));
    assert!(k.iter().any(|e| matches!(e, EventKind::Action { name, args, result: Some(r), status: ActionStatus::Done } if name == "Update" && args == "README.md" && r.contains("2 additions"))));
    assert!(k.iter().any(|e| matches!(e, EventKind::Diff { files, patch } if files == &["README.md"] && patch.contains("+status: ok") && patch.contains("-# Project"))));
    assert!(k.iter().any(|e| matches!(e, EventKind::Answer { text } if text == "I updated the heading and added a status line. Tell me if you want more.")));
    assert!(!k.iter().any(|e| matches!(e, EventKind::UserMessage { text } if text.is_empty())), "input box is not a message");
    assert!(ev.iter().all(|e| e.confidence == Confidence::Heuristic && e.source == "pty:claude"));
    assert_eq!(ev.iter().find_map(|e| match &e.event { EventKind::State { state, .. } => Some(*state), _ => None }), Some(SessionState::Idle));
}

#[test]
fn tool_call_gets_result_as_a_new_revision_of_the_same_id() {
    let running = "● Bash(cargo test)\n\n";
    let done = "● Bash(cargo test)\n  ⎿ 12 passed\n\n";
    let ev = run("claude", &[running, done]);
    let actions: Vec<_> = ev.iter().filter(|e| matches!(e.event, EventKind::Action { .. })).collect();
    assert_eq!(actions.len(), 2, "{actions:#?}");
    assert_eq!(actions[0].id, actions[1].id);
    assert_eq!((actions[0].rev, actions[1].rev), (1, 2));
    assert!(matches!(&actions[0].event, EventKind::Action { status: ActionStatus::Running, .. }));
    assert!(matches!(&actions[1].event, EventKind::Action { status: ActionStatus::Done, result: Some(r), .. } if r == "12 passed"));
}

#[test]
fn items_that_scroll_off_screen_are_not_lost_or_repeated() {
    let first = "● first answer\n\n";
    let second = "● second answer\n\n";
    let ev = run("claude", &[first, second, second]);
    let answers: Vec<_> = ev.iter().filter_map(|e| match &e.event { EventKind::Answer { text } => Some(text.as_str()), _ => None }).collect();
    assert_eq!(answers, ["first answer", "second answer"]);
}

#[test]
fn streaming_answer_is_debounced_into_few_events() {
    let mut s = EventStream::new("claude");
    let mut ev = Vec::new();
    for (i, t) in ["● Hel", "● Hello wor", "● Hello world, this is a long answer"].iter().enumerate() {
        ev.extend(s.observe_screen(t, i as u64 * 100));
    }
    ev.extend(s.observe_screen("● Hello world, this is a long answer", 1000));
    let answers: Vec<_> = ev.iter().filter(|e| matches!(e.event, EventKind::Answer { .. })).collect();
    assert_eq!(answers.len(), 1, "{answers:#?}");
}

#[test]
fn permission_prompt_blocks_and_lists_options() {
    let screen = "Bash command\n  rm -rf build\n\nDo you want to proceed?\n❯ 1. Yes\n  2. Yes, and don't ask again\n  3. No, and tell Claude what to do differently\n";
    let ev = latest(&run("claude", &[screen]));
    let p = ev.iter().find_map(|e| match &e.event { EventKind::PromptNeeded { prompt, options, .. } => Some((*prompt, options.clone())), _ => None }).unwrap();
    assert_eq!(p.0, PromptKind::Permission);
    assert_eq!(p.1.len(), 3);
    assert!(ev.iter().any(|e| matches!(e.event, EventKind::State { state: SessionState::NeedsInput, .. })));
}

#[test]
fn working_marker_sets_working_and_quiet_goes_idle() {
    let mut s = EventStream::new("claude");
    let ev = s.observe_screen("✻ Cogitating… (4s · esc to interrupt)\n", 0);
    assert!(ev.iter().any(|e| matches!(&e.event, EventKind::State { state: SessionState::Working, .. })));
    let ev = s.observe_screen("● done\n", 5000);
    assert!(ev.iter().all(|e| !matches!(&e.event, EventKind::State { state: SessionState::Working, detail } if detail.contains("interrupt"))));
    let ev = s.tick(8000);
    assert!(ev.iter().any(|e| matches!(&e.event, EventKind::State { state: SessionState::Idle, .. })));
}

#[test]
fn codex_bullets_ran_edited_and_answer() {
    let screen = "• Ran cargo fmt\n  └ finished\n• Edited src/lib.rs\n  └ +3 -1\n• Done. Formatting is clean.\n";
    let ev = latest(&run("codex", &[screen]));
    let names: Vec<_> = ev.iter().filter_map(|e| match &e.event { EventKind::Action { name, args, .. } => Some(format!("{name} {args}")), _ => None }).collect();
    assert_eq!(names, ["Ran cargo fmt", "Edited src/lib.rs"]);
    assert!(ev.iter().any(|e| matches!(&e.event, EventKind::Answer { text } if text.starts_with("Done."))));
}

#[test]
fn fixture_agent_screen_from_web_hq_runs() {
    // the p7 end-to-end fixture prints Claude-style markers
    let screen = "Fixture agent (stand-in for an MCP-using provider)\n\n> please reply to hatchery.operator with hi\n\n∴ Thinking: answer through the messenger\n● g4a_mail_send(to=hatchery.operator)\n  ⎿ {\"message_id\":\"m4a_1\"}\n● Sent. I replied through the messenger.\n> \n";
    let ev = latest(&run("claude", &[screen]));
    assert!(ev.iter().any(|e| matches!(&e.event, EventKind::Action { name, .. } if name == "g4a_mail_send")));
    assert!(ev.iter().any(|e| matches!(&e.event, EventKind::Answer { text } if text.starts_with("Sent."))));
}

#[test]
fn unknown_provider_uses_generic_fallback() {
    let ev = latest(&run("someagent", &["● hello from an unknown agent\n"]));
    assert!(ev.iter().any(|e| matches!(&e.event, EventKind::Answer { text } if text.contains("unknown agent")) && e.source == "pty:someagent"));
}

#[test]
fn events_serialize_for_the_wire() {
    let ev = run("claude", &["● hi\n"]);
    let j = serde_json::to_value(ev.iter().find(|e| matches!(e.event, EventKind::Answer { .. })).unwrap()).unwrap();
    assert_eq!(j["kind"], "answer");
    assert_eq!(j["confidence"], "heuristic");
    assert_eq!(j["source"], "pty:claude");
}
