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
const MAX_BODY_BYTES: usize = 128 * 1024;

pub async fn bind(addr: SocketAddr) -> std::io::Result<TcpListener> {
    assert!(addr.ip().is_loopback(), "control listener must be loopback");
    TcpListener::bind(addr).await
}

pub async fn serve(listener: TcpListener, supervisor: Supervisor, token: Option<String>) {
    serve_with_hub(listener, supervisor, token, None).await
}

/// Same as [`serve`], plus the P6 hub routes (`/v1/...`, bearer required): node calls forwarded by
/// C2, operator mail and tasks.
#[cfg(feature = "m4a")]
pub type HubHandle = Option<std::sync::Arc<crate::hub::Hub>>;
#[cfg(not(feature = "m4a"))]
pub type HubHandle = Option<()>;

pub async fn serve_with_hub(listener: TcpListener, supervisor: Supervisor, token: Option<String>, hub: HubHandle) {
    loop {
        let Ok((stream, peer)) = listener.accept().await else { continue };
        if !peer.ip().is_loopback() {
            continue;
        }
        let supervisor = supervisor.clone();
        let token = token.clone();
        let hub = hub.clone();
        tokio::spawn(async move {
            let _ = handle(stream, supervisor, token, hub).await;
        });
    }
}

async fn handle(
    mut stream: TcpStream,
    supervisor: Supervisor,
    token: Option<String>,
    hub: HubHandle,
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
    let head_end = buffer[..filled].windows(4).position(|w| w == b"\r\n\r\n").map(|p| p + 4).unwrap_or(filled);
    let head = String::from_utf8_lossy(&buffer[..head_end]).into_owned();
    let mut request_body = buffer[head_end..filled].to_vec();
    let length = head
        .lines()
        .find_map(|l| l.split_once(':').filter(|(n, _)| n.eq_ignore_ascii_case("content-length")).and_then(|(_, v)| v.trim().parse::<usize>().ok()))
        .unwrap_or(0);
    if length > MAX_BODY_BYTES {
        return respond(&mut stream, "413 Payload Too Large", json!({ "error": "too large" }).to_string()).await;
    }
    while request_body.len() < length {
        let mut chunk = [0u8; 4096];
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            break;
        }
        request_body.extend_from_slice(&chunk[..n]);
    }
    request_body.truncate(length);
    let first = head.lines().next().unwrap_or("");
    let mut fp = first.split_whitespace();
    let (method, path) = (fp.next().unwrap_or(""), fp.next().unwrap_or(""));
    let (status, body) = if path.starts_with("/v1/") {
        // The mailbox store blocks on its connection lock; never run it on a runtime thread.
        let (method, path, head, hub, token) = (method.to_owned(), path.to_owned(), head.clone(), hub.clone(), token.clone());
        tokio::task::spawn_blocking(move || v1(&method, &path, &head, &request_body, token.as_deref(), &hub))
            .await
            .unwrap_or(("500 Internal Server Error", json!({ "error": "internal" }).to_string()))
    } else {
        route(&head, &supervisor, token.as_deref())
    };
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(response.as_bytes()).await?;
    stream.shutdown().await
}

async fn respond(stream: &mut TcpStream, status: &str, body: String) -> std::io::Result<()> {
    let response = format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
    stream.write_all(response.as_bytes()).await?;
    stream.shutdown().await
}

fn bearer_of(head: &str) -> Option<String> {
    head.lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("authorization").then(|| value.trim().to_owned())
        })
        .and_then(|value| value.strip_prefix("Bearer ").map(str::to_owned))
}

fn status_line(code: u16) -> &'static str {
    match code {
        200 => "200 OK",
        400 => "400 Bad Request",
        403 => "403 Forbidden",
        404 => "404 Not Found",
        409 => "409 Conflict",
        _ => "500 Internal Server Error",
    }
}

#[cfg(feature = "m4a")]
fn v1(method: &str, path: &str, head: &str, body: &[u8], token: Option<&str>, hub: &HubHandle) -> (&'static str, String) {
    let authorized = match (token, bearer_of(head).as_deref()) {
        (Some(expected), Some(given)) => constant_time_eq(expected.as_bytes(), given.as_bytes()),
        _ => false,
    };
    if !authorized {
        return ("401 Unauthorized", json!({ "error": "unauthorized" }).to_string());
    }
    let Some(hub) = hub else {
        return ("404 Not Found", json!({ "error": "hub is not enabled" }).to_string());
    };
    let value: serde_json::Value = if body.is_empty() { json!({}) } else {
        match serde_json::from_slice(body) {
            Ok(v) => v,
            Err(_) => return ("400 Bad Request", json!({ "error": "malformed json" }).to_string()),
        }
    };
    let result = if method == "POST" && path == "/v1/node-call" {
        match serde_json::from_value::<hatchery_enroll::call::ForwardedCall>(value) {
            Ok(call) => hub.node_call(&call.node_id, &call.op, &call.payload),
            Err(_) => return ("400 Bad Request", json!({ "error": "malformed call" }).to_string()),
        }
    } else {
        hub.operator_call(method, path, &value)
    };
    match result {
        Ok(v) => ("200 OK", v.to_string()),
        Err(e) => (status_line(e.status), json!({ "error": e.message }).to_string()),
    }
}

#[cfg(not(feature = "m4a"))]
fn v1(_: &str, _: &str, _: &str, _: &[u8], _: Option<&str>, _: &HubHandle) -> (&'static str, String) {
    ("404 Not Found", json!({ "error": "hub is not built in" }).to_string())
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
