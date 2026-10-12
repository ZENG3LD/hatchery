//! Agent-facing tools for mail and tasks, served through the node's loopback door.
//!
//! Agents see ordinary tools (`g4a_mail_send`, `g4a_mail_inbox`, `g4a_mail_ack`, `g4a_tasks`,
//! `g4a_task_update`). The adapter forwards to the node over loopback; the node signs the call
//! with an identity key the agent never sees, and the hub applies the node's status. The tools
//! exist only when `HATCHERY_NODE_DOOR` names a loopback address.

use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

pub const DOOR_ADDR_ENV: &str = "HATCHERY_NODE_DOOR";
const IO_TIMEOUT: Duration = Duration::from_secs(60);

/// (tool name, door operation, description, schema)
fn tools() -> Vec<(&'static str, &'static str, &'static str, Value)> {
    let s = |props: Value, req: &[&str]| json!({"type":"object","properties":props,"required":req,"additionalProperties":false});
    vec![
        ("g4a_mail_send", "mail.send", "Send a message to another participant (a node name) or to a room (`room:NAME`).",
            s(json!({"to":{"type":"string"},"subject":{"type":"string"},"body":{"type":"string"},"idempotency_key":{"type":"string"}}), &["to","subject","body"])),
        ("g4a_mail_inbox", "mail.inbox", "List messages addressed to this machine.",
            s(json!({"since_ms":{"type":"integer"},"limit":{"type":"integer"}}), &[])),
        ("g4a_mail_ack", "mail.ack", "Mark a message as read.", s(json!({"message_id":{"type":"string"}}), &["message_id"])),
        ("g4a_tasks", "task.list", "List open tasks assigned to this machine.", s(json!({}), &[])),
        ("g4a_task_update", "task.update", "Set the state of one of your tasks (`in_progress` or `done`).",
            s(json!({"id":{"type":"integer"},"status":{"type":"string","enum":["in_progress","done"]}}), &["id","status"])),
    ]
}

#[derive(Clone)]
pub struct WorkDoor {
    addr: SocketAddr,
}

impl WorkDoor {
    pub fn new(addr: SocketAddr) -> Option<Self> {
        addr.ip().is_loopback().then_some(Self { addr })
    }

    pub fn from_env() -> Option<Self> {
        Self::new(std::env::var(DOOR_ADDR_ENV).ok()?.parse().ok()?)
    }

    pub fn tool_definitions() -> Vec<Value> {
        tools()
            .into_iter()
            .map(|(name, _, description, schema)| {
                json!({"name": name, "description": description, "inputSchema": schema,
                       "annotations": {"readOnlyHint": name == "g4a_mail_inbox" || name == "g4a_tasks", "destructiveHint": false, "openWorldHint": false}})
            })
            .collect()
    }

    pub fn owns(name: &str) -> bool {
        tools().iter().any(|t| t.0 == name)
    }

    /// Returns the reply JSON text or an opaque error.
    pub fn call(&self, tool: &str, arguments: Value) -> Result<String, ()> {
        let op = tools().into_iter().find(|t| t.0 == tool).map(|t| t.1).ok_or(())?;
        let body = json!({"op": op, "payload": arguments}).to_string();
        let mut stream = TcpStream::connect_timeout(&self.addr, IO_TIMEOUT).map_err(|_| ())?;
        stream.set_read_timeout(Some(IO_TIMEOUT)).map_err(|_| ())?;
        stream.set_write_timeout(Some(IO_TIMEOUT)).map_err(|_| ())?;
        let request = format!("POST /door HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
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
    fn forwards_op_and_payload_and_hides_nothing_but_the_transport() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let host = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut buf = [0u8; 2048];
            let n = s.read(&mut buf).unwrap();
            s.write_all(b"HTTP/1.1 200 OK\r\n\r\n{\"tasks\":[]}").unwrap();
            String::from_utf8_lossy(&buf[..n]).into_owned()
        });
        let door = WorkDoor::new(addr).unwrap();
        assert_eq!(door.call("g4a_tasks", json!({})).unwrap(), "{\"tasks\":[]}");
        let seen = host.join().unwrap();
        assert!(seen.starts_with("POST /door HTTP/1.1") && seen.contains(r#""op":"task.list""#));
    }

    #[test]
    fn refusal_and_unknown_tools_are_opaque_errors() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut buf = [0u8; 512];
            let _ = s.read(&mut buf);
            s.write_all(b"HTTP/1.1 403 Forbidden\r\n\r\n{\"error\":\"refused\"}").unwrap();
        });
        let door = WorkDoor::new(addr).unwrap();
        assert!(door.call("g4a_mail_inbox", json!({})).is_err());
        assert!(door.call("g4a_nothing", json!({})).is_err());
        assert!(WorkDoor::new("10.0.0.1:1".parse().unwrap()).is_none());
    }
}
