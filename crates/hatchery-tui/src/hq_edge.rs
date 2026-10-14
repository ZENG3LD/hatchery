//! hq-edge wrapper: the TUI as a thin client of the hq-edge HTTP API (hq-web), which
//! fronts hq-core (`/api/s/{tasks,runs,sessions,signals,routing}`). No core credentials
//! live here; the edge holds them. Plain HTTP/1.1 to a loopback/tunnelled edge address.

use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

pub const DEFAULT_EDGE: &str = "127.0.0.1:18500";

pub struct Edge {
    addr: SocketAddr,
}

impl Edge {
    pub fn new(addr: &str) -> Result<Self, String> {
        Ok(Self { addr: addr.parse().map_err(|_| format!("bad edge address: {addr}"))? })
    }

    pub fn call(&self, method: &str, path: &str, body: Option<&Value>) -> Result<Value, String> {
        let mut s = TcpStream::connect_timeout(&self.addr, Duration::from_secs(5)).map_err(|e| format!("edge: {e}"))?;
        s.set_read_timeout(Some(Duration::from_secs(30))).ok();
        let body = body.map(Value::to_string).unwrap_or_default();
        let req = format!(
            "{method} {path} HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            self.addr,
            body.len()
        );
        s.write_all(req.as_bytes()).map_err(|e| e.to_string())?;
        let mut raw = Vec::new();
        s.read_to_end(&mut raw).map_err(|e| e.to_string())?;
        parse_response(&raw)
    }
}

pub fn parse_response(raw: &[u8]) -> Result<Value, String> {
    let text = String::from_utf8_lossy(raw);
    let (head, body) = text.split_once("\r\n\r\n").ok_or("edge: malformed reply")?;
    let status: u16 = head.split_whitespace().nth(1).and_then(|s| s.parse().ok()).ok_or("edge: no status")?;
    let body = if head.to_ascii_lowercase().contains("transfer-encoding: chunked") { dechunk(body) } else { body.to_owned() };
    let v: Value = serde_json::from_str(body.trim()).unwrap_or(Value::String(body));
    if (200..300).contains(&status) {
        Ok(v)
    } else {
        Err(format!("{status}: {}", v.get("error").and_then(Value::as_str).unwrap_or("request failed")))
    }
}

fn dechunk(mut s: &str) -> String {
    let mut out = String::new();
    while let Some((size, rest)) = s.split_once("\r\n") {
        let Ok(n) = usize::from_str_radix(size.trim(), 16) else { break };
        if n == 0 || rest.len() < n {
            break;
        }
        out.push_str(&rest[..n]);
        s = rest[n..].trim_start_matches("\r\n");
    }
    out
}

fn s(v: &Value, k: &str) -> String {
    match &v[k] {
        Value::Null => "-".into(),
        Value::String(x) => x.clone(),
        other => other.to_string(),
    }
}

pub fn render_tasks(v: &Value) -> String {
    let mut out = String::from("ID   REV  STATUS       ROLE       ASSIGNEE        TITLE\n");
    for t in v["tasks"].as_array().into_iter().flatten() {
        out += &format!("{:<4} {:<4} {:<12} {:<10} {:<15} {}\n", s(t, "id"), s(t, "revision"), s(t, "status"), s(t, "role"), s(t, "assignee"), s(t, "title"));
    }
    out
}

pub fn render_routing(v: &Value) -> String {
    let mut out = String::from("ROLES\n");
    for (role, r) in v["roles"].as_object().into_iter().flatten() {
        out += &format!("  {role:<10} auto={:<5} {}\n", r["auto"].as_bool().unwrap_or(false), r["preset"].as_array().map(|p| p.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(" > ")).unwrap_or_default());
    }
    out += "SEATS\n";
    for seat in v["seats"].as_array().into_iter().flatten() {
        let until = seat["state"]["limited_until_ms"].as_u64();
        out += &format!("  {:<16} {:<14} {:<8} {}\n", s(&seat["seat"], "id"), s(&seat["seat"], "node"), s(&seat["seat"], "provider"), until.map(|u| format!("limited until {u}")).unwrap_or_else(|| "free".into()));
    }
    out
}

pub fn render_signals(v: &Value) -> String {
    let mut out = String::from("ID   NODE           PROVIDER  KIND            HINT\n");
    for x in v["signals"].as_array().into_iter().flatten() {
        out += &format!("{:<4} {:<14} {:<9} {:<15} {}\n", s(x, "id"), s(x, "node_id"), s(x, "provider"), s(x, "kind"), s(x, "hint"));
    }
    out
}

pub fn render_sessions(v: &Value) -> String {
    let mut out = String::from("SESSION                          NODE           PROVIDER  ROLE       STATE\n");
    for x in v["sessions"].as_array().into_iter().flatten() {
        out += &format!("{:<32} {:<14} {:<9} {:<10} {}\n", s(x, "session_id"), s(x, "node_id"), s(x, "provider"), s(x, "role"), s(x, "state"));
    }
    out
}

pub const USAGE: &str = "usage: hatchery-tui hq [--edge ADDR] <tasks|sessions|signals|routing|route ROLE|task-new TITLE [ROLE]|task-move ID STATUS [REV]|watch>";

/// Run one `hq` command; returns the text to print.
pub fn run(args: &[String]) -> Result<String, String> {
    let mut edge = DEFAULT_EDGE.to_owned();
    let mut rest = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--edge" {
            edge = it.next().ok_or("--edge needs ADDR")?.clone();
        } else {
            rest.push(a.as_str());
        }
    }
    let e = Edge::new(&edge)?;
    match rest.as_slice() {
        ["tasks"] => Ok(render_tasks(&e.call("GET", "/api/s/tasks", None)?)),
        ["sessions"] => Ok(render_sessions(&e.call("GET", "/api/s/sessions", None)?)),
        ["signals"] => Ok(render_signals(&e.call("GET", "/api/s/signals?limit=50", None)?)),
        ["routing"] => Ok(render_routing(&e.call("GET", "/api/s/routing", None)?)),
        ["route", role] => Ok(serde_json::to_string_pretty(&e.call("POST", "/api/s/routing/recommend", Some(&json!({ "role": role })))?["recommendation"]).unwrap_or_default()),
        ["task-new", title, more @ ..] => {
            let mut b = json!({ "title": title });
            if let Some(role) = more.first() {
                b["role"] = json!(role);
            }
            Ok(render_tasks(&json!({ "tasks": [e.call("POST", "/api/s/tasks", Some(&b))?["task"]] })))
        }
        ["task-move", id, status, more @ ..] => {
            let mut b = json!({ "status": status });
            if let Some(rev) = more.first() {
                b["expected_revision"] = json!(rev.parse::<i64>().map_err(|_| "REV must be a number")?);
            }
            Ok(render_tasks(&json!({ "tasks": [e.call("POST", &format!("/api/s/tasks/{id}/move"), Some(&b))?["task"]] })))
        }
        ["watch"] => loop {
            let mut screen = String::from("\x1b[2J\x1b[H");
            for (title, path, f) in [
                ("TASKS", "/api/s/tasks", render_tasks as fn(&Value) -> String),
                ("ROUTING", "/api/s/routing", render_routing),
                ("SIGNALS", "/api/s/signals?limit=10", render_signals),
            ] {
                screen += &format!("== {title}\n");
                screen += &e.call("GET", path, None).map(|v| f(&v)).unwrap_or_else(|err| format!("({err})\n"));
            }
            print!("{screen}");
            let _ = std::io::stdout().flush();
            std::thread::sleep(Duration::from_secs(3));
        },
        _ => Err(USAGE.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_and_chunked_replies() {
        let v = parse_response(b"HTTP/1.1 200 OK\r\ncontent-type: application/json\r\n\r\n{\"tasks\":[]}").unwrap();
        assert_eq!(v["tasks"], json!([]));
        let v = parse_response(b"HTTP/1.1 200 OK\r\ntransfer-encoding: chunked\r\n\r\n5\r\n{\"a\":\r\n2\r\n1}\r\n0\r\n\r\n").unwrap();
        assert_eq!(v["a"], 1);
        assert_eq!(parse_response(b"HTTP/1.1 409 Conflict\r\n\r\n{\"error\":\"stale revision\"}").unwrap_err(), "409: stale revision");
    }

    #[test]
    fn renders_tables() {
        let t = render_tasks(&json!({ "tasks": [{ "id": 1, "revision": 2, "status": "open", "role": "dev", "assignee": null, "title": "x" }] }));
        assert!(t.contains("open") && t.contains("dev") && t.lines().count() == 2);
        let r = render_routing(&json!({ "roles": { "dev": { "preset": ["claude/opus:high"], "auto": true } }, "seats": [{ "seat": { "id": "s", "node": "n", "provider": "claude" }, "state": { "limited_until_ms": 5 } }] }));
        assert!(r.contains("claude/opus:high") && r.contains("limited until 5"));
    }

    #[test]
    fn usage_on_unknown() {
        assert_eq!(run(&["bogus".into()]).unwrap_err(), USAGE);
    }
}
