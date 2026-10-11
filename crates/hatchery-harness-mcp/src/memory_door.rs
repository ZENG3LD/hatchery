//! Agent-facing door that starts the on-demand code-memory service.
//!
//! The agent sees one ordinary tool, `g4a_memory_start`. The adapter forwards it
//! to the node-side service host over loopback with a bearer taken from the
//! environment of the node (never from the agent's arguments) and returns only
//! the state and the loopback MCP URL. The door exists only when both variables
//! are set; otherwise the tool is not listed.

use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

pub const MEMORY_START_TOOL: &str = "g4a_memory_start";
pub const CONTROL_ADDR_ENV: &str = "HATCHERY_SERVER_CONTROL_LISTEN";
pub const CONTROL_TOKEN_ENV: &str = "HATCHERY_SERVER_CONTROL_TOKEN";
const IO_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone)]
pub struct MemoryDoor {
    addr: SocketAddr,
    token: String,
}

impl MemoryDoor {
    pub fn new(addr: SocketAddr, token: String) -> Option<Self> {
        (addr.ip().is_loopback() && !token.is_empty()).then_some(Self { addr, token })
    }

    pub fn from_env() -> Option<Self> {
        let addr = std::env::var(CONTROL_ADDR_ENV).ok()?.parse().ok()?;
        Self::new(addr, std::env::var(CONTROL_TOKEN_ENV).ok()?)
    }

    pub fn tool_definition() -> Value {
        json!({
            "name": MEMORY_START_TOOL,
            "description": "Start the code-memory index service for this machine (if it is not running) and return its local MCP URL.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false },
            "annotations": {
                "readOnlyHint": false,
                "destructiveHint": false,
                "idempotentHint": true,
                "openWorldHint": false
            }
        })
    }

    /// Ask the host to start memory. Returns `{state, url}` or an opaque error.
    pub fn start(&self) -> Result<Value, ()> {
        let mut stream = TcpStream::connect_timeout(&self.addr, IO_TIMEOUT).map_err(|_| ())?;
        stream.set_read_timeout(Some(IO_TIMEOUT)).map_err(|_| ())?;
        stream.set_write_timeout(Some(IO_TIMEOUT)).map_err(|_| ())?;
        let request = format!(
            "POST /sub/memory/start HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            self.token
        );
        stream.write_all(request.as_bytes()).map_err(|_| ())?;
        let mut raw = Vec::new();
        stream.take(64 * 1024).read_to_end(&mut raw).map_err(|_| ())?;
        let text = String::from_utf8_lossy(&raw);
        let (head, body) = text.split_once("\r\n\r\n").ok_or(())?;
        if !head.starts_with("HTTP/1.1 200") {
            return Err(());
        }
        let status: Value = serde_json::from_str(body).map_err(|_| ())?;
        let port = status.get("port").and_then(Value::as_u64).filter(|p| (18200..=18299).contains(p)).ok_or(())?;
        Ok(json!({
            "state": status.get("state").and_then(Value::as_str).unwrap_or("running"),
            "url": format!("http://127.0.0.1:{port}/mcp"),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    fn fake_host(response: &'static str) -> (SocketAddr, std::thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 2048];
            let n = stream.read(&mut buf).unwrap();
            stream.write_all(response.as_bytes()).unwrap();
            String::from_utf8_lossy(&buf[..n]).into_owned()
        });
        (addr, handle)
    }

    #[test]
    fn posts_with_bearer_and_returns_only_state_and_url() {
        let (addr, handle) = fake_host(
            "HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n{\"detail\":null,\"name\":\"memory\",\"port\":18204,\"state\":\"running\"}",
        );
        let door = MemoryDoor::new(addr, "node-side-token".into()).unwrap();
        let out = door.start().unwrap();
        assert_eq!(out, json!({"state":"running","url":"http://127.0.0.1:18204/mcp"}));
        let request = handle.join().unwrap();
        assert!(request.starts_with("POST /sub/memory/start HTTP/1.1"));
        assert!(request.contains("Authorization: Bearer node-side-token"));
    }

    #[test]
    fn refuses_port_outside_range_and_non_200() {
        let (addr, _h) = fake_host("HTTP/1.1 200 OK\r\n\r\n{\"port\":9000,\"state\":\"running\"}");
        assert!(MemoryDoor::new(addr, "t".into()).unwrap().start().is_err());
        let (addr, _h) = fake_host("HTTP/1.1 401 Unauthorized\r\n\r\n{}");
        assert!(MemoryDoor::new(addr, "t".into()).unwrap().start().is_err());
    }

    #[test]
    fn door_requires_loopback_and_token() {
        assert!(MemoryDoor::new("10.0.0.1:1".parse().unwrap(), "t".into()).is_none());
        assert!(MemoryDoor::new("127.0.0.1:1".parse().unwrap(), String::new()).is_none());
    }
}
