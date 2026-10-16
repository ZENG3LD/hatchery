//! Node letter pipeline end to end with fake providers: node mailbox letter -> policy -> g4a
//! routine POST (fake routine on loopback; fake credential command), and the refusals.

#![cfg(unix)]

use std::io::{Read, Write};
use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

use hatchery_node::wake_pipeline::{node_mail_tick, route, MailSeen, Route, RoutineTarget, SessionShape, WakePolicy};
use serde_json::json;

fn temp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("hn-wake-e2e-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn fake_routine(status: &'static str) -> (String, mpsc::Receiver<String>) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/routine", l.local_addr().unwrap());
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(mut s) = s else { break };
            s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            let mut buf = Vec::new();
            let mut chunk = [0u8; 4096];
            loop {
                let n = s.read(&mut chunk).unwrap_or(0);
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&chunk[..n]);
                let t = String::from_utf8_lossy(&buf);
                if let Some((head, body)) = t.split_once("\r\n\r\n") {
                    let len = head
                        .lines()
                        .find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap()))
                        .unwrap_or(0);
                    if body.len() >= len {
                        break;
                    }
                }
            }
            let _ = write!(s, "HTTP/1.1 {status}\r\ncontent-length: 0\r\nconnection: close\r\n\r\n");
            let _ = tx.send(String::from_utf8_lossy(&buf).into_owned());
        }
    });
    (url, rx)
}

fn credential_script(dir: &PathBuf, url: &str, key: &str) -> Vec<String> {
    let p = dir.join("cred.sh");
    std::fs::write(&p, format!("#!/bin/sh\nprintf '{{\"url\":\"{url}\",\"key\":\"{key}\"}}'\n")).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o700)).unwrap();
    vec![p.display().to_string()]
}

fn policy(cmd: Vec<String>) -> WakePolicy {
    WakePolicy {
        routines: [("web-bot".to_string(), RoutineTarget { credential_command: cmd })].into(),
        node_mail_to: Some("web-bot".into()),
        ..WakePolicy::default()
    }
}

#[test]
fn node_mail_reaches_the_routine_once_through_g4a_and_history_is_not_replayed() {
    let dir = temp("deliver");
    let (url, rx) = fake_routine("200 OK");
    let policy = policy(credential_script(&dir, &url, "k-fake-1"));
    let mut seen = MailSeen::load(dir.join("seen.json"));
    let old = json!({"messages": [{"message_id": "m-old", "from": "operator", "subject": "old", "body": "history"}]});
    assert!(node_mail_tick(&policy, &mut seen, &old, None).is_empty(), "first read only records");
    assert!(rx.recv_timeout(Duration::from_millis(300)).is_err(), "history was replayed");

    let new = json!({"messages": [
        {"message_id": "m-old", "from": "operator", "subject": "old", "body": "history"},
        {"message_id": "m-1", "from": "operator", "subject": "ping", "body": "wake up"}
    ]});
    let out = node_mail_tick(&policy, &mut seen, &new, Some("http://127.0.0.1:1/door"));
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].message_id, "m-1");
    assert_eq!(out[0].result, Ok(200));
    let req = rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let lower = req.to_ascii_lowercase();
    assert!(lower.contains("authorization: bearer k-fake-1") && lower.contains("x-automation-key: k-fake-1"));
    let body: serde_json::Value = serde_json::from_str(req.split_once("\r\n\r\n").unwrap().1).unwrap();
    assert_eq!(body["body"], "wake up");
    assert_eq!(body["from"], "operator");
    assert_eq!(body["to"], "web-bot");
    assert_eq!(body["event_id"], "m-1");
    assert!(body["reply"].as_str().unwrap().contains("m4a-send"));

    // Same inbox again (and after a restart): nothing new.
    assert!(node_mail_tick(&policy, &mut seen, &new, None).is_empty());
    let mut reopened = MailSeen::load(dir.join("seen.json"));
    assert!(node_mail_tick(&policy, &mut reopened, &new, None).is_empty(), "seen ledger survives a restart");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_failing_routine_or_credential_is_reported_without_secrets() {
    let dir = temp("fail");
    let (url, _rx) = fake_routine("403 Forbidden");
    let policy = policy(credential_script(&dir, &url, "k-fake-2"));
    let mut seen = MailSeen::load(dir.join("seen.json"));
    node_mail_tick(&policy, &mut seen, &json!({"messages": []}), None);
    let out = node_mail_tick(&policy, &mut seen, &json!({"messages": [{"message_id": "m-2", "body": "x"}]}), None);
    let err = out[0].result.clone().unwrap_err();
    assert!(err.contains("403") && !err.contains("k-fake-2") && !err.contains(&url), "{err}");

    let broken = WakePolicy { routines: [("web-bot".to_string(), RoutineTarget { credential_command: vec!["/nonexistent/cred".into()] })].into(), node_mail_to: Some("web-bot".into()), ..WakePolicy::default() };
    let mut seen = MailSeen::load(dir.join("seen2.json"));
    node_mail_tick(&broken, &mut seen, &json!({"messages": []}), None);
    let out = node_mail_tick(&broken, &mut seen, &json!({"messages": [{"message_id": "m-3", "body": "x"}]}), None);
    assert!(out[0].result.as_ref().unwrap_err().starts_with("credential command"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn without_a_policy_nothing_is_delivered_and_typing_is_refused() {
    let dir = temp("default");
    let p = WakePolicy::default();
    let mut seen = MailSeen::load(dir.join("seen.json"));
    node_mail_tick(&p, &mut seen, &json!({"messages": []}), None);
    assert!(node_mail_tick(&p, &mut seen, &json!({"messages": [{"message_id": "m", "body": "x"}]}), None).is_empty());
    assert!(matches!(route(&p, "anyone", SessionShape::RawTerminal), Route::Refuse(_)));
    assert_eq!(route(&p, "anyone", SessionShape::Semantic), Route::Deliver);
    let _ = std::fs::remove_dir_all(&dir);
}
