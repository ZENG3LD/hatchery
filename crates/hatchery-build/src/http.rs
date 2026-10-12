//! Minimal HTTP/1.1 helpers (one request per connection) shared by the host, C2 and the node.

use std::io;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;

pub const MAX_RESPONSE: usize = 64 * 1024 * 1024;

/// POST a body; returns (status, response body).
pub async fn post(addr: SocketAddr, path: &str, bearer: Option<&str>, body: &[u8], deadline: Duration) -> io::Result<(u16, Vec<u8>)> {
    let io = async {
        let mut stream = TcpStream::connect(addr).await?;
        let auth = bearer.map(|t| format!("Authorization: Bearer {t}\r\n")).unwrap_or_default();
        let head = format!(
            "POST {path} HTTP/1.1\r\nHost: {addr}\r\n{auth}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        stream.write_all(head.as_bytes()).await?;
        stream.write_all(body).await?;
        let mut raw = Vec::new();
        (&mut stream).take(MAX_RESPONSE as u64).read_to_end(&mut raw).await?;
        Ok::<_, io::Error>(raw)
    };
    let raw = timeout(deadline, io).await.map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "request timed out"))??;
    let split = raw.windows(4).position(|w| w == b"\r\n\r\n").ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "no header end"))?;
    let status = std::str::from_utf8(&raw[..split])
        .ok()
        .and_then(|h| h.split_whitespace().nth(1))
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "bad status line"))?;
    Ok((status, raw[split + 4..].to_vec()))
}

/// Read one request: (method, path, bearer, body). Body limited to `max_body`.
pub async fn read_request(stream: &mut TcpStream, max_body: usize) -> io::Result<(String, String, Option<String>, Vec<u8>)> {
    let mut bytes = Vec::with_capacity(2048);
    let mut chunk = [0_u8; 2048];
    let header_end = loop {
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        bytes.extend_from_slice(&chunk[..n]);
        if bytes.len() > max_body + 8192 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "too large"));
        }
        if let Some(pos) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
            break pos + 4;
        }
    };
    let head = std::str::from_utf8(&bytes[..header_end]).map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "utf8"))?;
    let mut lines = head.split("\r\n");
    let mut first = lines.next().unwrap_or_default().split_whitespace();
    let method = first.next().unwrap_or_default().to_owned();
    let path = first.next().unwrap_or_default().to_owned();
    let (mut length, mut bearer) = (0_usize, None);
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                length = value.trim().parse().map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "length"))?;
            } else if name.eq_ignore_ascii_case("authorization") {
                bearer = value.trim().strip_prefix("Bearer ").map(str::to_owned);
            }
        }
    }
    if length > max_body {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "body too large"));
    }
    let mut body = bytes[header_end..].to_vec();
    while body.len() < length {
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        body.extend_from_slice(&chunk[..n]);
    }
    body.truncate(length);
    Ok((method, path, bearer, body))
}

pub async fn respond(stream: &mut TcpStream, status: u16, body: &[u8]) -> io::Result<()> {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        422 => "Unprocessable Entity",
        429 => "Too Many Requests",
        502 => "Bad Gateway",
        504 => "Gateway Timeout",
        _ => "Internal Server Error",
    };
    let head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes()).await?;
    stream.write_all(body).await?;
    stream.shutdown().await
}

/// Constant-time comparison for bearer secrets.
pub fn secret_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let mut diff = (a.len() ^ b.len()) as u8;
    for i in 0..a.len().max(b.len()) {
        diff |= a.get(i).copied().unwrap_or(0) ^ b.get(i).copied().unwrap_or(0);
    }
    diff == 0
}
