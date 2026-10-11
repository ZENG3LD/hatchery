//! Loopback control endpoint (minimal HTTP/1.1, no framework).
//!
//! - `GET /health`            : open on loopback, lists subservice status.
//! - `POST /sub/<name>/start` : bearer required (`HATCHERY_SERVER_CONTROL_TOKEN`).
//! - `POST /sub/<name>/stop`  : bearer required.
//! The bearer is a node-side credential; agents never receive it.

use crate::supervisor::Supervisor;
use serde_json::json;
use std::net::SocketAddr;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

const MAX_REQUEST_BYTES: usize = 8 * 1024;

pub async fn bind(addr: SocketAddr) -> std::io::Result<TcpListener> {
    assert!(addr.ip().is_loopback(), "control listener must be loopback");
    TcpListener::bind(addr).await
}

pub async fn serve(listener: TcpListener, supervisor: Supervisor, token: Option<String>) {
    loop {
        let Ok((stream, peer)) = listener.accept().await else { continue };
        if !peer.ip().is_loopback() {
            continue;
        }
        let supervisor = supervisor.clone();
        let token = token.clone();
        tokio::spawn(async move {
            let _ = handle(stream, supervisor, token).await;
        });
    }
}

async fn handle(
    mut stream: TcpStream,
    supervisor: Supervisor,
    token: Option<String>,
) -> std::io::Result<()> {
    let mut buffer = vec![0u8; MAX_REQUEST_BYTES];
    let mut filled = 0;
    loop {
        let read = stream.read(&mut buffer[filled..]).await?;
        if read == 0 {
            return Ok(());
        }
        filled += read;
        if buffer[..filled].windows(4).any(|w| w == b"\r\n\r\n") || filled == buffer.len() {
            break;
        }
    }
    let head = String::from_utf8_lossy(&buffer[..filled]).into_owned();
    let (status, body) = route(&head, &supervisor, token.as_deref());
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(response.as_bytes()).await?;
    stream.shutdown().await
}

pub fn route(head: &str, supervisor: &Supervisor, token: Option<&str>) -> (&'static str, String) {
    let mut lines = head.lines();
    let request_line = lines.next().unwrap_or("");
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let path = parts.next().unwrap_or("");
    if method == "GET" && path == "/health" {
        return ("200 OK", json!({ "status": "ok", "subservices": supervisor.statuses() }).to_string());
    }
    let segments: Vec<&str> = path.trim_matches('/').split('/').collect();
    if method == "POST" && segments.len() == 3 && segments[0] == "sub" {
        let presented = lines
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("authorization").then(|| value.trim().to_owned())
            })
            .and_then(|value| value.strip_prefix("Bearer ").map(str::to_owned));
        let authorized = match (token, presented.as_deref()) {
            (Some(expected), Some(given)) => constant_time_eq(expected.as_bytes(), given.as_bytes()),
            _ => false,
        };
        if !authorized {
            return ("401 Unauthorized", json!({ "error": "unauthorized" }).to_string());
        }
        let result = match segments[2] {
            "start" => supervisor.start_named(segments[1]),
            "stop" => supervisor.stop_named(segments[1]),
            _ => return ("404 Not Found", json!({ "error": "not found" }).to_string()),
        };
        return match result {
            Ok(status) => ("200 OK", json!(status).to_string()),
            Err(error) => ("409 Conflict", json!({ "error": error.to_string() }).to_string()),
        };
    }
    ("404 Not Found", json!({ "error": "not found" }).to_string())
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter().zip(right).fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0
}
