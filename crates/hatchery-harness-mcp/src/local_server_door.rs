//! Agent-facing `local_server.*` tools → node's loopback door.
//!
//! Present only when `HATCHERY_NODE_DOOR` is a loopback address. The session id
//! comes from `HATCHERY_LOCAL_SERVER_SESSION` (injected by the node at MCP
//! overlay install); the agent never picks another session's pool.

use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

pub const DOOR_ADDR_ENV: &str = "HATCHERY_NODE_DOOR";
pub const SESSION_ENV: &str = "HATCHERY_LOCAL_SERVER_SESSION";
const IO_TIMEOUT: Duration = Duration::from_secs(60);

fn tools() -> Vec<(&'static str, &'static str, &'static str, Value)> {
    let s = |props: Value, req: &[&str]| {
        json!({"type":"object","properties":props,"required":req,"additionalProperties":false})
    };
    vec![
        (
            "local_server.list",
            "local_server.list",
            "List shipped local-server kinds and running instances for this session.",
            s(json!({}), &[]),
        ),
        (
            "local_server.start",
            "local_server.start",
            "Start a session-local HTTP server on loopback. kind=static needs root; kind=http needs bin (or cwd). Set open=true to navigate the session browser.",
            s(
                json!({
                    "kind":{"type":"string","enum":["static","http"]},
                    "root":{"type":"string"},
                    "cwd":{"type":"string"},
                    "bin":{"type":"string"},
                    "open":{"type":"boolean"},
                    "spa":{"type":"boolean"}
                }),
                &["kind"],
            ),
        ),
        (
            "local_server.stop",
            "local_server.stop",
            "Stop one session-local server by id.",
            s(json!({"id":{"type":"string"}}), &["id"]),
        ),
        (
            "local_server.logs",
            "local_server.logs",
            "Tail the log ring for one session-local server.",
            s(json!({"id":{"type":"string"},"limit":{"type":"integer"}}), &["id"]),
        ),
    ]
}

#[derive(Clone)]
pub struct LocalServerDoor {
    addr: SocketAddr,
    session: String,
}

impl LocalServerDoor {
    pub fn new(addr: SocketAddr, session: String) -> Option<Self> {
        (addr.ip().is_loopback() && !session.is_empty()).then_some(Self { addr, session })
    }

    pub fn from_env() -> Option<Self> {
        let addr = std::env::var(DOOR_ADDR_ENV).ok()?.parse().ok()?;
        let session = std::env::var(SESSION_ENV).ok()?;
        Self::new(addr, session)
    }

    pub fn tool_definitions() -> Vec<Value> {
        tools()
            .into_iter()
            .map(|(name, _, description, schema)| {
                json!({
                    "name": name,
                    "description": description,
                    "inputSchema": schema,
                    "annotations": {
                        "readOnlyHint": name.ends_with(".list") || name.ends_with(".logs"),
                        "destructiveHint": name.ends_with(".stop"),
                        "openWorldHint": false
                    }
                })
            })
            .collect()
    }

    pub fn owns(name: &str) -> bool {
        tools().iter().any(|t| t.0 == name)
    }

    pub fn call(&self, tool: &str, mut arguments: Value) -> Result<String, ()> {
        let op = tools()
            .into_iter()
            .find(|t| t.0 == tool)
            .map(|t| t.1)
            .ok_or(())?;
        if !arguments.is_object() {
            arguments = json!({});
        }
        if let Some(obj) = arguments.as_object_mut() {
            obj.insert("session".into(), json!(self.session));
        }
        let body = json!({"op": op, "payload": arguments}).to_string();
        let mut stream = TcpStream::connect_timeout(&self.addr, IO_TIMEOUT).map_err(|_| ())?;
        stream.set_read_timeout(Some(IO_TIMEOUT)).map_err(|_| ())?;
        stream.set_write_timeout(Some(IO_TIMEOUT)).map_err(|_| ())?;
        let request = format!(
            "POST /door HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(request.as_bytes()).map_err(|_| ())?;
        let mut raw = Vec::new();
        stream.take(256 * 1024).read_to_end(&mut raw).map_err(|_| ())?;
        let text = String::from_utf8_lossy(&raw);
        let (head, reply) = text.split_once("\r\n\r\n").ok_or(())?;
        if !head.starts_with("HTTP/1.1 200") {
            return Err(());
        }
        serde_json::from_str::<Value>(reply).map_err(|_| ())?;
        Ok(reply.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn forwards_session_and_op() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let host = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut buf = [0u8; 4096];
            let n = s.read(&mut buf).unwrap();
            s.write_all(b"HTTP/1.1 200 OK\r\n\r\n{\"instances\":[]}").unwrap();
            String::from_utf8_lossy(&buf[..n]).into_owned()
        });
        let door = LocalServerDoor::new(addr, "pty-primary-1".into()).unwrap();
        assert_eq!(
            door.call("local_server.list", json!({})).unwrap(),
            "{\"instances\":[]}"
        );
        let seen = host.join().unwrap();
        assert!(seen.contains(r#""op":"local_server.list""#));
        assert!(seen.contains(r#""session":"pty-primary-1""#));
    }
}
