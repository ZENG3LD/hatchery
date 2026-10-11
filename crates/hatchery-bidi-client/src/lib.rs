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

/// CLOCK_MONOTONIC nanoseconds. Same clock in every netns on this box.
pub fn mono_ns() -> u64 {
    #[repr(C)]
    struct Timespec {
        tv_sec: i64,
        tv_nsec: i64,
    }
    extern "C" {
        fn clock_gettime(clk_id: i32, tp: *mut Timespec) -> i32;
    }
    const CLOCK_MONOTONIC: i32 = 1;
    let mut ts = Timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    let rc = unsafe { clock_gettime(CLOCK_MONOTONIC, &mut ts) };
    if rc != 0 {
        return 0;
    }
    (ts.tv_sec as u64)
        .saturating_mul(1_000_000_000)
        .saturating_add(ts.tv_nsec as u64)
}

/// `b"FRME"` + capture_ns little-endian. Not a PNG.
pub fn parse_frame_timing(bytes: &[u8]) -> Option<u64> {
    if bytes.len() == 12 && bytes.starts_with(b"FRME") {
        let mut buf = [0u8; 8];
        buf.copy_from_slice(&bytes[4..12]);
        Some(u64::from_le_bytes(buf))
    } else {
        None
    }
}

fn append_clock(path: &std::path::Path, line: &str) {
    use std::io::Write;
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{line}");
        let _ = file.flush();
    }
}

/// Read frames, then send every click, then keep reading until a later frame differs.
///
/// Frame latency is `recv_ns - capture_ns` where capture_ns was stamped on the
/// node into the FRME prefix and recv_ns is taken here when the PNG bytes arrive.
/// Click send timestamps are written for the node to pair by sequence.
pub async fn pull_login(
    c2_http: &str,
    session: &str,
    clicks: &[(f64, f64, MouseButton)],
    min_frames: usize,
    clock_path: &std::path::Path,
) -> Result<(Vec<u8>, Vec<u8>), ClientError> {
    if clicks.is_empty() {
        return Err(ClientError("at least one click is required".into()));
    }
    if min_frames == 0 {
        return Err(ClientError("min_frames must be at least 1".into()));
    }
    let _ = std::fs::remove_file(clock_path);
    let endpoints = c2_endpoints(c2_http, session)?;
    open_session(&endpoints.post_url).await?;
    let mut ws = connect_ws(&endpoints.ws_url).await?;
    let mut pending_capture: Option<u64> = None;
    let mut before: Option<Vec<u8>> = None;
    let mut count = 0usize;
    let pre_deadline = tokio::time::Instant::now() + Duration::from_secs(60);
    while count < min_frames {
        if tokio::time::Instant::now() >= pre_deadline {
            return Err(ClientError(format!(
                "only {count} frames arrived before clicks"
            )));
        }
        let (png, capture) = next_timed_png(&mut ws, &mut pending_capture).await?;
        note_frame(clock_path, &png, capture);
        count += 1;
        before = Some(png);
    }
    let before = before.ok_or_else(|| ClientError("no before frame".into()))?;
    for (i, (x, y, button)) in clicks.iter().enumerate() {
        let seq = i + 1;
        let bytes = encode_click(*x, *y, *button);
        let send_ns = mono_ns();
        ws.send(Message::binary(bytes.clone()))
            .await
            .map_err(|err| ClientError(format!("send click failed: {err}")))?;
        append_clock(
            clock_path,
            &format!("click seq={seq} send_ns={send_ns} bytes={}", bytes.len()),
        );
        eprintln!(
            "[hatchery-bidi] sent click seq={seq} bytes={}",
            bytes.len()
        );
    }
    let mut after = before.clone();
    let mut saw_diff = false;
    let post_deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    let mut extra = 0usize;
    // The relay now keeps every frame for a late peer, so frames that were
    // queued before the click may precede the first post-click frame: read
    // until a changed frame arrives or the deadline passes.
    while !saw_diff && tokio::time::Instant::now() < post_deadline {
        let timed = tokio::time::timeout(
            Duration::from_secs(8),
            next_timed_png(&mut ws, &mut pending_capture),
        )
        .await;
        let (png, capture) = match timed {
            Ok(Ok(pair)) => pair,
            Ok(Err(err)) => return Err(err),
            Err(_) => break,
        };
        note_frame(clock_path, &png, capture);
        extra += 1;
        if png != before {
            after = png;
            saw_diff = true;
        }
    }
    if !saw_diff {
        return Err(ClientError(
            "post-click frames did not differ from the before frame".into(),
        ));
    }
    Ok((before, after))
}

fn note_frame(clock_path: &std::path::Path, png: &[u8], capture: Option<u64>) {
    let recv = mono_ns();
    match capture {
        Some(capture_ns) => append_clock(
            clock_path,
            &format!(
                "frame capture_ns={capture_ns} recv_ns={recv} bytes={}",
                png.len()
            ),
        ),
        None => append_clock(
            clock_path,
            &format!("frame capture_ns=missing recv_ns={recv} bytes={}", png.len()),
        ),
    }
    eprintln!("[hatchery-bidi] frame bytes={}", png.len());
}

async fn next_timed_png(ws: &mut Ws, pending: &mut Option<u64>) -> Result<(Vec<u8>, Option<u64>), ClientError> {
    for _ in 0..16 {
        let msg = tokio::time::timeout(Duration::from_secs(30), ws.next())
            .await
            .map_err(|_| ClientError("timed out waiting for a frame".into()))?
            .ok_or_else(|| ClientError("websocket closed before a frame".into()))?
            .map_err(|err| ClientError(err.to_string()))?;
        match msg {
            Message::Binary(data) => {
                if let Some(ns) = parse_frame_timing(&data) {
                    *pending = Some(ns);
                    continue;
                }
                if data.len() >= 8 && data.starts_with(&[0x89, b'P', b'N', b'G']) {
                    let capture = pending.take();
                    return Ok((data.to_vec(), capture));
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

    #[test]
    fn frame_timing_header_is_not_a_png() {
        let mut bytes = b"FRME".to_vec();
        bytes.extend_from_slice(&1000u64.to_le_bytes());
        assert_eq!(parse_frame_timing(&bytes), Some(1000));
        assert!(parse_frame_timing(b"not-a-header").is_none());
        assert!(mono_ns() > 0);
    }

    fn hex_decode(hex: &str) -> Vec<u8> {
        (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect()
    }
}

const KIND_KEY: u8 = 2;
const KIND_SCROLL: u8 = 3;
const KIND_NAV: u8 = 4;
const KIND_TEXT: u8 = 5;
const KEY_PRESS: u8 = 3;

/// Press one key by DOM name (`Enter`, `Tab`, `Escape`, ...) or a single printable character.
pub fn encode_key(key: &str) -> Result<Vec<u8>, ClientError> {
    let bytes = key.as_bytes();
    if bytes.is_empty() || bytes.len() > 64 {
        return Err(ClientError("key length must be 1..=64".into()));
    }
    let mut out = vec![WIRE_VERSION, KIND_KEY, KEY_PRESS, bytes.len() as u8];
    out.extend_from_slice(bytes);
    Ok(out)
}

/// Wheel scroll at (x, y) with CSS-pixel deltas.
pub fn encode_scroll(x: f64, y: f64, dx: f64, dy: f64) -> Vec<u8> {
    let mut out = vec![WIRE_VERSION, KIND_SCROLL, 0];
    for v in [x, y, dx, dy] {
        out.extend_from_slice(&(v as f32).to_le_bytes());
    }
    out
}

fn encode_text_like(kind: u8, value: &str) -> Result<Vec<u8>, ClientError> {
    let bytes = value.as_bytes();
    if bytes.is_empty() || bytes.len() > 2048 {
        return Err(ClientError("value length must be 1..=2048".into()));
    }
    let mut out = vec![WIRE_VERSION, kind];
    out.extend_from_slice(&(bytes.len() as u16).to_le_bytes());
    out.extend_from_slice(bytes);
    Ok(out)
}

/// Navigate the node's page to a URL.
pub fn encode_navigate(url: &str) -> Result<Vec<u8>, ClientError> {
    encode_text_like(KIND_NAV, url)
}

/// Type text into the focused element, one character at a time.
pub fn encode_text(text: &str) -> Result<Vec<u8>, ClientError> {
    encode_text_like(KIND_TEXT, text)
}

/// One HQ action in a browse session.
#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    /// Keep receiving frames for this many seconds.
    Wait(f64),
    /// Write the newest received frame to `<out_dir>/<name>.png` and log its SHA-256.
    Shot(String),
    Click(f64, f64),
    Scroll { x: f64, y: f64, dx: f64, dy: f64 },
    Key(String),
    Text(String),
    Navigate(String),
}

/// Parse `wait:5;shot:01-name;click:640,360;scroll:640,400,0,500;key:Enter;text:abc;nav:https://...`.
pub fn parse_steps(raw: &str) -> Result<Vec<Step>, ClientError> {
    let mut steps = Vec::new();
    for item in raw.split(';').map(str::trim).filter(|s| !s.is_empty()) {
        let (kind, value) = item.split_once(':').unwrap_or((item, ""));
        let nums = |n: usize| -> Result<Vec<f64>, ClientError> {
            let v: Vec<f64> = value.split(',').filter_map(|p| p.trim().parse().ok()).collect();
            if v.len() == n { Ok(v) } else { Err(ClientError(format!("{kind} needs {n} numbers"))) }
        };
        steps.push(match kind {
            "wait" => Step::Wait(value.parse().map_err(|_| ClientError("wait needs seconds".into()))?),
            "shot" => Step::Shot(value.to_owned()),
            "click" => { let v = nums(2)?; Step::Click(v[0], v[1]) }
            "scroll" => { let v = nums(4)?; Step::Scroll { x: v[0], y: v[1], dx: v[2], dy: v[3] } }
            "key" => Step::Key(value.to_owned()),
            "text" => Step::Text(value.to_owned()),
            "nav" => Step::Navigate(value.to_owned()),
            other => return Err(ClientError(format!("unknown step {other}"))),
        });
    }
    Ok(steps)
}

/// Run steps against a live browse session. Frames that were queued while HQ was away are
/// skipped before every shot so the PNG shows the page as it is now. Returns one log line per
/// step (sent byte counts, frame sizes and SHA-256 only).
pub async fn run_steps(
    c2_http: &str,
    session: &str,
    steps: &[Step],
    out_dir: &std::path::Path,
) -> Result<Vec<String>, ClientError> {
    use sha2::{Digest, Sha256};
    std::fs::create_dir_all(out_dir).map_err(|e| ClientError(e.to_string()))?;
    let endpoints = c2_endpoints(c2_http, session)?;
    open_session(&endpoints.post_url).await?;
    let mut ws = connect_ws(&endpoints.ws_url).await?;
    let mut latest: Option<Vec<u8>> = None;
    let mut log = Vec::new();
    // Receive until `until`, keeping the newest PNG.
    async fn absorb(ws: &mut Ws, latest: &mut Option<Vec<u8>>, secs: f64) -> Result<(), ClientError> {
        let end = tokio::time::Instant::now() + Duration::from_millis((secs * 1000.0) as u64);
        while let Ok(Some(msg)) = tokio::time::timeout_at(end, ws.next()).await {
            match msg {
                Ok(Message::Binary(b)) if b.starts_with(&[0x89, b'P', b'N', b'G']) => *latest = Some(b.to_vec()),
                Ok(Message::Close(_)) | Err(_) => return Err(ClientError("stream closed".into())),
                Ok(_) => {}
            }
        }
        Ok(())
    }
    for step in steps {
        let line = match step {
            Step::Wait(secs) => {
                absorb(&mut ws, &mut latest, *secs).await?;
                format!("wait {secs}s")
            }
            Step::Shot(name) => {
                // Drain the backlog quickly, then take what arrives next as the current page.
                absorb(&mut ws, &mut latest, 1.2).await?;
                let png = latest.clone().ok_or_else(|| ClientError("no frame received yet".into()))?;
                let path = out_dir.join(format!("{name}.png"));
                std::fs::write(&path, &png).map_err(|e| ClientError(e.to_string()))?;
                let digest: String = Sha256::digest(&png).iter().map(|b| format!("{b:02x}")).collect();
                format!("shot {name} bytes={} sha256={digest}", png.len())
            }
            other => {
                let (wire, what) = match other {
                    Step::Click(x, y) => (encode_click(*x, *y, MouseButton::Left), format!("click {x},{y}")),
                    Step::Scroll { x, y, dx, dy } => (encode_scroll(*x, *y, *dx, *dy), format!("scroll at {x},{y} by {dx},{dy}")),
                    Step::Key(k) => (encode_key(k)?, format!("key {k}")),
                    Step::Text(t) => (encode_text(t)?, format!("text {t:?}")),
                    Step::Navigate(u) => (encode_navigate(u)?, format!("nav {u}")),
                    _ => unreachable!(),
                };
                ws.send(Message::binary(wire.clone()))
                    .await
                    .map_err(|e| ClientError(format!("send failed: {e}")))?;
                let digest: String = Sha256::digest(&wire).iter().map(|b| format!("{b:02x}")).collect();
                absorb(&mut ws, &mut latest, 0.1).await?;
                format!("send {what} bytes={} sha256={digest}", wire.len())
            }
        };
        eprintln!("[hatchery-bidi] {line}");
        log.push(line);
    }
    Ok(log)
}

#[cfg(test)]
mod browse_tests {
    use super::*;

    #[test]
    fn steps_parse_and_encode() {
        let steps = parse_steps("wait:2;shot:a;click:10,20;scroll:1,2,0,300;key:Enter;text:hi;nav:https://example.org/").unwrap();
        assert_eq!(steps.len(), 7);
        assert_eq!(encode_scroll(1.0, 2.0, 0.0, 300.0).len(), 19);
        assert_eq!(encode_key("Enter").unwrap()[3], 5);
        assert_eq!(encode_navigate("https://example.org/").unwrap()[1], 4);
        assert!(parse_steps("click:1").is_err());
    }
}
