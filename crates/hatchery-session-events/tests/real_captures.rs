//! Parser tests on RECORDED REAL PTY captures of the provider CLIs, started with a fresh HOME and no
//! login (tests/fixtures/real/*.jsonl, produced by recording the pty byte stream with timings).
//! Without a login the CLIs only show onboarding screens, so these captures prove prompt-needed,
//! banner, working and idle detection on real bytes. Answer/thought/action/diff parsing is covered by
//! tests/synthetic.rs on documented formats (clearly labelled synthetic).
mod common;
use common::*;
use hatchery_session_events::{EventKind, PromptKind, SessionState};

fn has_prompt(ev: &[hatchery_session_events::SessionEvent], k: PromptKind) -> bool {
    ev.iter().any(|e| matches!(&e.event, EventKind::PromptNeeded { prompt, .. } if *prompt == k))
}
fn final_state(ev: &[hatchery_session_events::SessionEvent]) -> Option<SessionState> {
    ev.iter().rev().find_map(|e| match &e.event { EventKind::State { state, .. } => Some(*state), _ => None })
}

#[test]
fn claude_real_startup_login_menu() {
    let ev = replay("claude", "claude-startup.jsonl");
    assert!(has_prompt(&ev, PromptKind::Login), "{ev:#?}");
    let opts = latest(&ev).iter().find_map(|e| match &e.event { EventKind::PromptNeeded { prompt: PromptKind::Login, options, .. } => Some(options.clone()), _ => None }).unwrap();
    assert_eq!(opts.len(), 3, "{opts:?}");
    assert!(opts[0].contains("Claude account with subscription"));
    assert_eq!(final_state(&ev), Some(SessionState::NeedsInput));
    assert!(ev.iter().all(|e| e.confidence == hatchery_session_events::Confidence::Heuristic));
}

#[test]
fn codex_real_startup_menu_then_api_key_entry() {
    let ev = replay("codex", "codex-startup.jsonl");
    assert!(has_prompt(&ev, PromptKind::ApiKey), "{ev:#?}");
    assert_eq!(final_state(&ev), Some(SessionState::NeedsInput));
    assert!(ev.iter().any(|e| matches!(&e.event, EventKind::Banner { text } if text.contains("Codex"))) || has_prompt(&ev, PromptKind::Login));
}

#[test]
fn kimi_real_startup_idle_with_login_hint() {
    let ev = replay("kimi", "kimi-startup.jsonl");
    assert!(has_prompt(&ev, PromptKind::Trust), "real kimi first asks to trust the folder: {ev:#?}");
    assert!(has_prompt(&ev, PromptKind::Login), "{ev:#?}");
    assert_eq!(final_state(&ev), Some(SessionState::Idle), "{ev:#?}");
    assert!(!ev.iter().any(|e| matches!(&e.event, EventKind::UserMessage { .. })), "input box must not become a user message");
}

#[test]
fn grok_real_startup_device_code_waiting() {
    let ev = replay("grok", "grok-startup.jsonl");
    assert!(has_prompt(&ev, PromptKind::Login), "{ev:#?}");
    assert!(final_state(&ev).is_some());
}

#[test]
fn gemini_real_startup_auth_menu() {
    let ev = replay("gemini", "gemini-startup.jsonl");
    assert!(has_prompt(&ev, PromptKind::Login), "{ev:#?}");
    let opts = latest(&ev).iter().find_map(|e| match &e.event { EventKind::PromptNeeded { prompt: PromptKind::Login, options, .. } => Some(options.clone()), _ => None }).unwrap();
    assert_eq!(opts.len(), 3, "{opts:?}");
    assert!(opts[1].contains("Gemini API Key"));
    // the capture presses Enter on the menu: the CLI then asks to trust the folder
    assert!(has_prompt(&ev, PromptKind::Trust));
}

#[test]
fn replay_is_deterministic() {
    let a = replay("claude", "claude-startup.jsonl");
    let b = replay("claude", "claude-startup.jsonl");
    assert_eq!(a, b);
}

#[test]
#[ignore]
fn print_all() {
    for (p, f) in [("kimi", "kimi-startup.jsonl")] {
        for e in replay(p, f) { println!("{} r{} {:?}", e.id, e.rev, e.event); }
    }
}
