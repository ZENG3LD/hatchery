#![allow(dead_code)]
use base64::{engine::general_purpose::STANDARD, Engine};
use hatchery_session_events::{EventKind, EventStream, SessionEvent};
use std::io::BufRead;

/// Replays a recorded real PTY capture (jsonl: meta line, then {t, out|in} records) through the event layer.
pub fn replay(provider: &str, file: &str) -> Vec<SessionEvent> {
    let path = format!("{}/tests/fixtures/real/{file}", env!("CARGO_MANIFEST_DIR"));
    let mut lines = std::io::BufReader::new(std::fs::File::open(path).unwrap()).lines();
    let meta: serde_json::Value = serde_json::from_str(&lines.next().unwrap().unwrap()).unwrap();
    let mut s = EventStream::new(provider);
    s.set_size(meta["rows"].as_u64().unwrap() as u16, meta["cols"].as_u64().unwrap() as u16);
    let mut events = Vec::new();
    let mut last = 0u64;
    for l in lines {
        let v: serde_json::Value = serde_json::from_str(&l.unwrap()).unwrap();
        let t = (v["t"].as_f64().unwrap() * 1000.0) as u64;
        last = t;
        if let Some(o) = v["out"].as_str() {
            events.extend(s.feed_bytes(&STANDARD.decode(o).unwrap(), t));
        }
    }
    for k in 1..=6 { events.extend(s.tick(last + k * 500)); }
    events
}

pub fn kinds(events: &[SessionEvent]) -> Vec<&EventKind> { events.iter().map(|e| &e.event).collect() }

/// Last revision of every item (consumers upsert by id).
pub fn latest(events: &[SessionEvent]) -> Vec<SessionEvent> {
    let mut m: std::collections::BTreeMap<u64, SessionEvent> = Default::default();
    for e in events { m.insert(e.id, e.clone()); }
    m.into_values().collect()
}
