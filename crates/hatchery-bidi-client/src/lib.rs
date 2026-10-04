//! HQ side of the bidirectional browser stream.
//!
//! This client dials C2 and only C2. It has no node address. It receives
//! opaque frame bytes and can send one click (x, y, button) back. It does
//! not launch a browser and does not apply input.

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;

type Ws = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

const WIRE_VERSION: u8 = 1;
const KIND_MOUSE: u8 = 1;
const MOUSE_CLICK: u8 = 4;
const BUTTON_LEFT: u8 = 0;
const BUTTON_MIDDLE: u8 = 1;
const BUTTON_RIGHT: u8 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
}

#[derive(Debug)]
pub struct ClientError(pub String);

impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ClientError {}

pub struct Endpoints {
    pub post_url: String,
    pub ws_url: String,
}

/// Build the C2 HTTP and WebSocket URLs. The only host is the C2 origin.
pub fn c2_endpoints(c2_http: &str, session: &str) -> Result<Endpoints, ClientError> {
    let (scheme, rest) = c2_http
        .split_once("://")
        .ok_or_else(|| ClientError("c2 url needs a scheme".into()))?;
    let ws_scheme = match scheme {
        "http" => "ws",
        "https" => "wss",
        other => {
            return Err(ClientError(format!(
                "c2 scheme must be http or https, got {other}"
            )))
        }
    };
    if rest.is_empty() || rest.contains('/') || rest.contains('@') || rest.contains(' ') {
        return Err(ClientError(
            "c2 url must be scheme://host:port with no path".into(),
        ));
    }
    if session.is_empty()
        || session.len() > 64
        || !session
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return Err(ClientError("bad session id".into()));
    }
    Ok(Endpoints {
        post_url: format!("{scheme}://{rest}/session/{session}"),
        ws_url: format!("{ws_scheme}://{rest}/session/{session}/hq"),
    })
}

pub fn encode_click(x: f64, y: f64, button: MouseButton) -> Vec<u8> {
    let mut out = Vec::with_capacity(20);
    out.push(WIRE_VERSION);
    out.push(KIND_MOUSE);
    out.push(MOUSE_CLICK);
    out.push(match button {
        MouseButton::Left => BUTTON_LEFT,
        MouseButton::Middle => BUTTON_MIDDLE,
        MouseButton::Right => BUTTON_RIGHT,
    });
    out.extend_from_slice(&x.to_le_bytes());
    out.extend_from_slice(&y.to_le_bytes());
    out
}

/// Open the session on C2, read one PNG frame, send one click, return the PNG.
pub async fn pull_frame_and_click(
    c2_http: &str,
    session: &str,
    x: f64,
    y: f64,
    button: MouseButton,
) -> Result<Vec<u8>, ClientError> {
    let endpoints = c2_endpoints(c2_http, session)?;
    open_session(&endpoints.post_url).await?;
    let mut ws = connect_ws(&endpoints.ws_url).await?;
    let png = next_png(&mut ws).await?;
    let click = encode_click(x, y, button);
    let n = click.len();
    ws.send(Message::binary(click))
        .await
        .map_err(|err| ClientError(format!("send click failed: {err}")))?;
    eprintln!("[hatchery-bidi] sent click bytes={n}");
    Ok(png)
}

async fn open_session(post_url: &str) -> Result<(), ClientError> {
    let rest = post_url
        .strip_prefix("http://")
        .ok_or_else(|| ClientError("HQ client opens sessions over http:// only".into()))?;
    let (host, path) = rest
        .split_once('/')
        .ok_or_else(|| ClientError("session url missing path".into()))?;
    let mut last = String::from("not attempted");
    for _ in 0..40 {
        match TcpStream::connect(host).await {
            Ok(mut sock) => {
                let req = format!(
                    "POST /{path} HTTP/1.1\r\nHost: {host}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                );
                sock.write_all(req.as_bytes())
                    .await
                    .map_err(|err| ClientError(err.to_string()))?;
                let mut buf = Vec::new();
                sock.read_to_end(&mut buf)
                    .await
                    .map_err(|err| ClientError(err.to_string()))?;
                let text = String::from_utf8_lossy(&buf);
                if text.starts_with("HTTP/1.1 200") {
                    eprintln!("[hatchery-bidi] session open {post_url}");
                    return Ok(());
                }
                last = text.chars().take(120).collect();
            }
            Err(err) => last = err.to_string(),
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    Err(ClientError(format!("session open failed: {last}")))
}

async fn connect_ws(ws_url: &str) -> Result<Ws, ClientError> {
    let mut last = String::from("not attempted");
    for attempt in 1..=40 {
        match tokio_tungstenite::connect_async(ws_url).await {
            Ok((ws, _)) => {
                eprintln!("[hatchery-bidi] websocket {ws_url} attempt={attempt}");
                return Ok(ws);
            }
            Err(err) => {
                last = err.to_string();
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
        }
    }
    Err(ClientError(format!("websocket {ws_url} failed: {last}")))
}

async fn next_png(ws: &mut Ws) -> Result<Vec<u8>, ClientError> {
    for _ in 0..8 {
        let msg = tokio::time::timeout(Duration::from_secs(30), ws.next())
            .await
            .map_err(|_| ClientError("timed out waiting for a frame".into()))?
            .ok_or_else(|| ClientError("websocket closed before a frame".into()))?
            .map_err(|err| ClientError(err.to_string()))?;
        match msg {
            Message::Binary(data) => {
                let n = data.len();
                eprintln!("[hatchery-bidi] frame bytes={n}");
                if data.len() >= 8 && data.starts_with(&[0x89, b'P', b'N', b'G']) {
                    return Ok(data.to_vec());
                }
            }
            Message::Ping(payload) => {
                ws.send(Message::Pong(payload))
                    .await
                    .map_err(|err| ClientError(err.to_string()))?;
            }
            Message::Close(_) => break,
            _ => {}
        }
    }
    Err(ClientError("no PNG frame arrived from C2".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn click_bytes_match_the_node_vector() {
        let bytes = encode_click(120.0, 90.0, MouseButton::Left);
        assert_eq!(
            bytes,
            hex_decode("010104000000000000005e400000000000805640")
        );
    }

    #[test]
    fn endpoints_dial_c2_hq_role_only() {
        let ends = c2_endpoints("http://10.88.1.2:18443", "proof1").unwrap();
        assert_eq!(ends.post_url, "http://10.88.1.2:18443/session/proof1");
        assert_eq!(ends.ws_url, "ws://10.88.1.2:18443/session/proof1/hq");
        assert!(!ends.ws_url.contains("/node"));
        assert!(c2_endpoints("http://10.88.1.2:18443/node", "proof1").is_err());
    }

    fn hex_decode(hex: &str) -> Vec<u8> {
        (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect()
    }
}
