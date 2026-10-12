//! Web HQ: a loopback server for the operator's browser. The browser talks only to this server;
//! the C2 operator token and the server control token live in this process's environment and are
//! never sent to the page. Requests are checked for a loopback Host and a same-origin Origin
//! (DNS-rebinding and cross-site protection).

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, Request, State};
use axum::http::{header, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get};
use axum::Router;
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::broadcast;

#[derive(Clone)]
pub struct Config {
    pub listen: SocketAddr,
    pub c2_api: SocketAddr,
    /// Origin of the browser byte relay (hatchery-bidi-relay); defaults to the C2 API origin.
    pub bidi: Option<SocketAddr>,
    pub c2_control: String,
    pub c2_token: String,
    pub server: Option<SocketAddr>,
    pub server_token: Option<String>,
    /// Optional read-only folder of PNG screenshots shown in the Library tab.
    pub library: Option<std::path::PathBuf>,
}

impl Config {
    /// Everything from the environment; tokens are never printed.
    pub fn from_env() -> Result<Self, String> {
        let var = |n: &str| std::env::var(n).ok().filter(|v| !v.is_empty());
        let listen: SocketAddr = var("HATCHERY_HQ_LISTEN").unwrap_or_else(|| "127.0.0.1:18500".into()).parse().map_err(|e| format!("HATCHERY_HQ_LISTEN: {e}"))?;
        if !listen.ip().is_loopback() {
            return Err("HATCHERY_HQ_LISTEN must be a loopback address (put a reverse proxy with its own auth in front for remote use)".into());
        }
        Ok(Self {
            listen,
            bidi: var("HATCHERY_HQ_BIDI").and_then(|v| v.parse().ok()),
            c2_api: var("HATCHERY_HQ_C2_API").ok_or("HATCHERY_HQ_C2_API is required")?.parse().map_err(|e| format!("HATCHERY_HQ_C2_API: {e}"))?,
            c2_control: var("HATCHERY_HQ_C2_CONTROL").ok_or("HATCHERY_HQ_C2_CONTROL is required (C2 control endpoint)")?,
            c2_token: var("HATCHERY_HQ_C2_TOKEN").or_else(|| var("GATE4AGENT_C2_TOKEN")).ok_or("HATCHERY_HQ_C2_TOKEN is required")?,
            server: var("HATCHERY_HQ_SERVER").map(|v| v.parse().map_err(|e| format!("HATCHERY_HQ_SERVER: {e}"))).transpose()?,
            server_token: var("HATCHERY_SERVER_CONTROL_TOKEN"),
            library: var("HATCHERY_HQ_LIBRARY").map(Into::into),
        })
    }
}

pub struct AppState {
    pub config: Config,
    pub events: broadcast::Sender<String>,
}

type Shared = Arc<AppState>;

pub fn router(config: Config) -> (Router, Shared) {
    let (events, _) = broadcast::channel(256);
    let state = Arc::new(AppState { config, events });
    let app = Router::new()
        .route("/", get(|| async { asset("text/html; charset=utf-8", include_str!("../assets/index.html").as_bytes()) }))
        .route("/app.js", get(|| async { asset("text/javascript; charset=utf-8", include_str!("../assets/app.js").as_bytes()) }))
        .route("/style.css", get(|| async { asset("text/css; charset=utf-8", include_str!("../assets/style.css").as_bytes()) }))
        .route("/vendor/xterm.js", get(|| async { asset("text/javascript; charset=utf-8", include_bytes!("../assets/vendor/xterm.js")) }))
        .route("/vendor/xterm.css", get(|| async { asset("text/css; charset=utf-8", include_bytes!("../assets/vendor/xterm.css")) }))
        .route("/api/config", get(api_config))
        .route("/api/nodes", get(api_nodes))
        .route("/api/c2/{*rest}", any(api_c2))
        .route("/api/s/{*rest}", any(api_server))
        .route("/api/library", get(api_library))
        .route("/api/library/{name}", get(api_library_file))
        .route("/api/events", get(api_events))
        .route("/api/term", get(term_ws))
        .route("/api/browser", get(browser_ws))
        .layer(middleware::from_fn_with_state(Arc::clone(&state), guard))
        .with_state(Arc::clone(&state));
    (app, state)
}

fn asset(content_type: &'static str, body: &'static [u8]) -> Response {
    ([(header::CONTENT_TYPE, content_type), (header::CACHE_CONTROL, "no-store")], body).into_response()
}

/// Loopback Host only; any Origin must be this very origin.
async fn guard(State(state): State<Shared>, request: Request, next: Next) -> Response {
    let headers = request.headers();
    let port = state.config.listen.port();
    let host_ok = headers
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .is_some_and(|h| [format!("127.0.0.1:{port}"), format!("localhost:{port}"), format!("[::1]:{port}")].iter().any(|ok| ok == h));
    if !host_ok {
        return (StatusCode::FORBIDDEN, "bad host").into_response();
    }
    if let Some(origin) = headers.get(header::ORIGIN).and_then(|o| o.to_str().ok()) {
        let host = headers.get(header::HOST).and_then(|h| h.to_str().ok()).unwrap_or_default();
        if origin != format!("http://{host}") {
            return (StatusCode::FORBIDDEN, "bad origin").into_response();
        }
    }
    next.run(request).await
}

// ---------------------------------------------------------------------------------- upstream

/// One HTTP/1.1 request to a loopback/private API; returns (status, body).
pub async fn upstream(addr: SocketAddr, method: &str, path: &str, bearer: Option<&str>, body: &[u8], deadline: Duration) -> Result<(u16, Vec<u8>), String> {
    let io = async {
        let mut stream = tokio::net::TcpStream::connect(addr).await?;
        let auth = bearer.map(|t| format!("Authorization: Bearer {t}\r\n")).unwrap_or_default();
        let head = format!("{method} {path} HTTP/1.1\r\nHost: {addr}\r\n{auth}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
        stream.write_all(head.as_bytes()).await?;
        stream.write_all(body).await?;
        let mut raw = Vec::new();
        (&mut stream).take(32 * 1024 * 1024).read_to_end(&mut raw).await?;
        Ok::<_, std::io::Error>(raw)
    };
    let raw = tokio::time::timeout(deadline, io).await.map_err(|_| "upstream timed out".to_owned())?.map_err(|e| e.to_string())?;
    let split = raw.windows(4).position(|w| w == b"\r\n\r\n").ok_or("bad upstream reply")?;
    let status = std::str::from_utf8(&raw[..split]).ok().and_then(|h| h.split_whitespace().nth(1)).and_then(|s| s.parse().ok()).ok_or("bad status line")?;
    Ok((status, raw[split + 4..].to_vec()))
}

fn json_reply(status: u16, body: Vec<u8>) -> Response {
    (StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY), [(header::CONTENT_TYPE, "application/json")], body).into_response()
}

fn err(status: StatusCode, msg: &str) -> Response {
    (status, [(header::CONTENT_TYPE, "application/json")], json!({ "error": msg }).to_string()).into_response()
}

fn safe_rest(rest: &str) -> bool {
    !rest.is_empty() && rest.len() < 128 && !rest.contains("..") && rest.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'-' | b'_' | b'.' | b'?' | b'=' | b'&'))
}

// ---------------------------------------------------------------------------------- handlers

async fn api_config(State(s): State<Shared>) -> Response {
    json_reply(200, json!({ "server": s.config.server.is_some(), "version": env!("CARGO_PKG_VERSION") }).to_string().into_bytes())
}

/// Allowlisted pass-through to the C2 operator API (`/v1/enroll/...`).
async fn api_c2(State(s): State<Shared>, Path(rest): Path<String>, method: Method, body: axum::body::Bytes) -> Response {
    let first = rest.split('/').next().unwrap_or_default();
    if !safe_rest(&rest) || !matches!(first, "codes" | "nodes" | "policy") || !matches!(method, Method::GET | Method::POST) {
        return err(StatusCode::NOT_FOUND, "not found");
    }
    match upstream(s.config.c2_api, method.as_str(), &format!("/v1/enroll/{rest}"), Some(&s.config.c2_token), &body, Duration::from_secs(15)).await {
        Ok((status, body)) => json_reply(status, body),
        Err(e) => err(StatusCode::BAD_GATEWAY, &e),
    }
}

/// Allowlisted pass-through to the hatchery-server control API (mail and tasks only).
async fn api_server(State(s): State<Shared>, Path(rest): Path<String>, method: Method, body: axum::body::Bytes) -> Response {
    let (Some(addr), Some(token)) = (s.config.server, s.config.server_token.as_deref()) else {
        return err(StatusCode::SERVICE_UNAVAILABLE, "server API is not configured");
    };
    let first = rest.split(['/', '?']).next().unwrap_or_default();
    if !safe_rest(&rest) || !matches!(first, "mail" | "tasks") || !matches!(method, Method::GET | Method::POST) {
        return err(StatusCode::NOT_FOUND, "not found");
    }
    let deadline = if rest == "mail/wait" { Duration::from_secs(40) } else { Duration::from_secs(15) };
    match upstream(addr, method.as_str(), &format!("/v1/{rest}"), Some(token), &body, deadline).await {
        Ok((status, body)) => json_reply(status, body),
        Err(e) => err(StatusCode::BAD_GATEWAY, &e),
    }
}

/// Enrollment records merged with live transport state from C2 `/status`.
async fn api_nodes(State(s): State<Shared>) -> Response {
    let cfg = &s.config;
    let enrolled = upstream(cfg.c2_api, "GET", "/v1/enroll/nodes", Some(&cfg.c2_token), b"", Duration::from_secs(10)).await.ok().filter(|r| r.0 == 200).and_then(|r| serde_json::from_slice::<Value>(&r.1).ok());
    let status = match hatchery_c2_client::C2Client::new(cfg.c2_api, cfg.c2_token.clone()) {
        Ok(c) => c.status().await.ok().and_then(|st| serde_json::to_value(st).ok()),
        Err(_) => None,
    };
    let live = |id: &str| -> String {
        status
            .as_ref()
            .and_then(|st| st["nodes"][id]["transport"].as_str())
            .map(str::to_lowercase)
            .unwrap_or_else(|| "unknown".into())
    };
    let mut out = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for n in enrolled.as_ref().and_then(|v| v["nodes"].as_array()).cloned().unwrap_or_default() {
        let id = n["node_id"].as_str().unwrap_or_default().to_owned();
        seen.insert(id.clone());
        out.push(json!({
            "node_id": id, "kind": n["kind"], "status": n["status"], "tunnel_ip": n["tunnel_ip"],
            "grants": n["grants"], "online": live(&id) == "online", "transport": live(&id), "enrolled": true,
        }));
    }
    if let Some(nodes) = status.as_ref().and_then(|st| st["nodes"].as_object()) {
        for id in nodes.keys().filter(|id| !seen.contains(*id)) {
            out.push(json!({ "node_id": id, "kind": "token", "status": "static", "online": live(id) == "online", "transport": live(id), "enrolled": false }));
        }
    }
    json_reply(200, json!({ "ready": status.as_ref().map(|st| st["ready"].clone()), "nodes": out }).to_string().into_bytes())
}

async fn api_library(State(s): State<Shared>) -> Response {
    let mut files: Vec<String> = s.config.library.as_ref().and_then(|d| std::fs::read_dir(d).ok()).map(|rd| rd.flatten().filter_map(|e| e.file_name().into_string().ok()).filter(|n| n.ends_with(".png") && !n.starts_with('.')).collect()).unwrap_or_default();
    files.sort();
    json_reply(200, json!({ "files": files }).to_string().into_bytes())
}

async fn api_library_file(State(s): State<Shared>, Path(name): Path<String>) -> Response {
    let ok = name.ends_with(".png") && name.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.')) && !name.contains("..");
    let Some(dir) = s.config.library.as_ref().filter(|_| ok) else { return err(StatusCode::NOT_FOUND, "not found") };
    match std::fs::read(dir.join(&name)) {
        Ok(bytes) => ([(header::CONTENT_TYPE, "image/png")], bytes).into_response(),
        Err(_) => err(StatusCode::NOT_FOUND, "not found"),
    }
}

async fn api_events(State(s): State<Shared>) -> Sse<impl futures_util::Stream<Item = Result<Event, std::convert::Infallible>>> {
    let rx = s.events.subscribe();
    let stream = tokio_stream::wrappers::BroadcastStream::new(rx).filter_map(|m| async move { m.ok().map(|data| Ok(Event::default().data(data))) });
    Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
}

/// Background wake-up: long-polls the server for operator mail and pushes an event to every
/// open page the moment a message lands (no blind polling in the browser).
pub fn spawn_mail_watcher(state: Shared) {
    let (Some(addr), Some(token)) = (state.config.server, state.config.server_token.clone()) else { return };
    tokio::spawn(async move {
        let mut since = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as u64;
        loop {
            let body = json!({ "since_ms": since, "timeout_secs": 25 }).to_string();
            match upstream(addr, "POST", "/v1/mail/wait", Some(&token), body.as_bytes(), Duration::from_secs(40)).await {
                Ok((200, reply)) => {
                    let woke_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as u64;
                    if let Some(msgs) = serde_json::from_slice::<Value>(&reply).ok().and_then(|v| v["messages"].as_array().cloned()) {
                        for m in msgs {
                            since = since.max(m["created_at_ms"].as_u64().unwrap_or(0));
                            tracing::info!(from = %m["from"], subject = %m["subject"], latency_ms = woke_ms.saturating_sub(m["created_at_ms"].as_u64().unwrap_or(woke_ms)), "HQ woken by mail");
                            let _ = state.events.send(json!({ "type": "mail", "woke_at_ms": woke_ms, "message": m }).to_string());
                        }
                    }
                }
                _ => tokio::time::sleep(Duration::from_secs(3)).await,
            }
        }
    });
}

// ---------------------------------------------------------------------------------- terminals

#[derive(Deserialize)]
struct TermQuery {
    node: String,
    provider: String,
    /// Event parser to use when it differs from the provider id (test fixtures).
    parser: Option<String>,
    #[serde(default = "d_rows")]
    rows: u16,
    #[serde(default = "d_cols")]
    cols: u16,
    #[serde(default = "d_ws")]
    workspace: String,
    /// Leave the session running when the page closes.
    #[serde(default)]
    keep: bool,
}
fn d_rows() -> u16 { 30 }
fn d_cols() -> u16 { 110 }
fn d_ws() -> String { "primary".into() }

async fn term_ws(State(s): State<Shared>, Query(q): Query<TermQuery>, ws: WebSocketUpgrade) -> Response {
    ws.on_upgrade(move |socket| async move {
        if let Err(message) = term_session(socket, s, q).await {
            tracing::warn!(%message, "terminal session ended with error");
        }
    })
}

fn collect_frames(v: &Value, out: &mut Vec<Value>) {
    match v {
        Value::Object(m) => {
            if m.contains_key("contents") && m.contains_key("cursor_row") {
                out.push(v.clone());
            }
            m.values().for_each(|x| collect_frames(x, out));
        }
        Value::Array(a) => a.iter().for_each(|x| collect_frames(x, out)),
        _ => {}
    }
}

async fn term_session(mut socket: WebSocket, s: Shared, q: TermQuery) -> Result<(), String> {
    use gate4agent_node_protocol::{AgentId, NodeRequest, SessionMode, WorkspaceId};
    use gate4agent_types::TerminalSize;
    use hatchery_c2_protocol::{C2NodeResponse, NodeId, NodeRoute, NodeTransportState};
    let cfg = &s.config;
    let say = |kind: &str, text: String| json!({ "type": kind, "text": text }).to_string();
    let node = NodeId::new(q.node.clone()).map_err(|e| e.to_string())?;
    let http = hatchery_c2_client::C2Client::new(cfg.c2_api, cfg.c2_token.clone()).map_err(|e| e.to_string())?;
    let status = http.status().await.map_err(|e| e.to_string())?;
    let observed = status.nodes.get(&node).filter(|n| n.transport == NodeTransportState::Online);
    let Some(observed) = observed else {
        let _ = socket.send(Message::text(say("error", format!("node {} is not online", q.node)))).await;
        return Err("node not online".into());
    };
    let route = NodeRoute { node_id: node.clone(), expected_incarnation_id: observed.cursor.clone().ok_or("no cursor")?.incarnation_id };
    let (control, mut events) = hatchery_c2_client::connect_local(&cfg.c2_control, &cfg.c2_token).await.map_err(|e| e.to_string())?;
    let spawned = control
        .request(
            route.clone(),
            NodeRequest::Spawn {
                workspace_id: WorkspaceId::new(q.workspace.clone()).map_err(|e| e.to_string())?,
                provider: AgentId::new(q.provider.clone()).map_err(|e| e.to_string())?,
                mode: SessionMode::Pty,
                terminal_size: TerminalSize { rows: q.rows, columns: q.cols },
                initial_prompt: None,
            },
        )
        .await
        .map_err(|e| e.to_string())?;
    let session = match spawned.response {
        Ok(C2NodeResponse::SpawnAccepted { session }) => session,
        other => {
            let _ = socket.send(Message::text(say("error", format!("spawn refused: {other:?}")))).await;
            return Err("spawn refused".into());
        }
    };
    let session_key = serde_json::to_value(&session).map(|v| v.to_string()).unwrap_or_default();
    let session_key = session_key.trim_matches('"').to_owned();
    tracing::info!(node = %q.node, provider = %q.provider, "terminal session spawned from web HQ");
    let _ = socket.send(Message::text(json!({ "type": "spawned", "node": q.node, "provider": q.provider, "session": session_key }).to_string())).await;
    let mut pending: Option<Value> = None;
    let mut tick = tokio::time::interval(Duration::from_millis(40));
    let mut settle = tokio::time::interval(Duration::from_millis(250));
    // The browser reads only this typed event layer; it never parses terminal text itself.
    let mut events_layer = hatchery_session_events::EventStream::new(q.parser.as_deref().unwrap_or(&q.provider));
    events_layer.set_size(q.rows, q.cols);
    let started = std::time::Instant::now();
    let now_ms = move || started.elapsed().as_millis() as u64;
    loop {
        tokio::select! {
            event = events.recv() => {
                let Some(event) = event else { break };
                let Ok(value) = serde_json::to_value(&event) else { continue };
                let mut frames = Vec::new();
                collect_frames(&value, &mut frames);
                if !frames.is_empty() && (session_key.is_empty() || value.to_string().contains(&session_key)) {
                    pending = frames.pop();
                }
            }
            _ = settle.tick() => {
                let evs = events_layer.tick(now_ms());
                if !evs.is_empty() && socket.send(Message::text(json!({ "type": "events", "events": evs }).to_string())).await.is_err() { break; }
            }
            _ = tick.tick() => {
                if let Some(f) = pending.take() {
                    if let Some(text) = f["contents"].as_str() {
                        let evs = events_layer.observe_screen(text, now_ms());
                        if !evs.is_empty() && socket.send(Message::text(json!({ "type": "events", "events": evs }).to_string())).await.is_err() { break; }
                    }
                    let bytes: Vec<u8> = f["formatted"].as_array().map(|a| a.iter().map(|x| x.as_u64().unwrap_or(0) as u8).collect()).unwrap_or_default();
                    let msg = json!({
                        "type": "frame", "rows": f["size"]["rows"], "cols": f["size"]["columns"],
                        "cursor": [f["cursor_row"], f["cursor_col"]], "ansi": B64.encode(&bytes),
                        "text": f["contents"],
                    });
                    if socket.send(Message::text(msg.to_string())).await.is_err() { break; }
                }
            }
            incoming = socket.recv() => {
                let Some(Ok(Message::Text(text))) = incoming else { if matches!(incoming, Some(Ok(Message::Close(_))) | None | Some(Err(_))) { break } else { continue } };
                let Ok(cmd) = serde_json::from_str::<Value>(&text) else { continue };
                // The node accepts at most 64 terminal bytes per request: split longer input.
                let chunks = |bytes: Vec<u8>| -> Vec<NodeRequest> { bytes.chunks(64).map(|c| NodeRequest::TerminalBytes { session: session.clone(), bytes: c.to_vec() }).collect() };
                let mut requests: Vec<NodeRequest> = match cmd["t"].as_str() {
                    Some("data") => cmd["data"].as_str().map(|d| chunks(d.as_bytes().to_vec())).unwrap_or_default(),
                    Some("prompt") => cmd["text"].as_str().map(|d| chunks(format!("{d}\r").into_bytes())).unwrap_or_default(),
                    Some("resize") => vec![NodeRequest::Resize { session: session.clone(), size: TerminalSize { rows: cmd["rows"].as_u64().unwrap_or(30) as u16, columns: cmd["cols"].as_u64().unwrap_or(110) as u16 } }],
                    Some("stop") => vec![NodeRequest::Stop { session: session.clone(), force: true }],
                    _ => Vec::new(),
                };
                if !requests.is_empty() {
                    let mut last = None;
                    let stopping = matches!(requests[0], NodeRequest::Stop { .. });
                    for request in requests.drain(..) {
                        let r = control.request(route.clone(), request).await;
                        let failed = r.as_ref().map(|x| x.response.is_err()).unwrap_or(true);
                        last = Some(r);
                        if failed { break; }
                    }
                    let result = last.expect("at least one request");

                    let ok = result.as_ref().map(|r| r.response.is_ok()).unwrap_or(false);
                    tracing::info!(op = %cmd["t"], ok, detail = %match &result { Ok(r) => format!("{:?}", r.response).chars().take(160).collect::<String>(), Err(e) => e.to_string() }, "terminal command from web HQ");
                    let _ = socket.send(Message::text(json!({ "type": "ack", "op": cmd["t"], "ok": ok, "detail": if ok { String::new() } else { match &result { Ok(r) => format!("{:?}", r.response), Err(e) => e.to_string() } } }).to_string())).await;
                    if stopping { break; }
                }
            }
        }
    }
    if !q.keep {
        let _ = control.request(route, NodeRequest::Stop { session, force: true }).await;
    }
    Ok(())
}

// ---------------------------------------------------------------------------------- browser frames

#[derive(Deserialize)]
struct BrowserQuery {
    session: String,
}

async fn browser_ws(State(s): State<Shared>, Query(q): Query<BrowserQuery>, ws: WebSocketUpgrade) -> Response {
    ws.on_upgrade(move |socket| async move {
        if let Err(e) = browser_session(socket, s, q.session).await {
            tracing::warn!(error = %e, "browser stream ended with error");
        }
    })
}

async fn browser_session(mut socket: WebSocket, s: Shared, session: String) -> Result<(), String> {
    use hatchery_bidi_client as bidi;
    use tokio_tungstenite::tungstenite::Message as Tm;
    if !session.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.')) {
        return Err("bad session id".into());
    }
    let endpoints = bidi::c2_endpoints(&format!("http://{}", s.config.bidi.unwrap_or(s.config.c2_api)), &session).map_err(|e| e.to_string())?;
    bidi::open_session(&endpoints.post_url).await.map_err(|e| e.to_string())?;
    let upstream_ws = bidi::connect_ws(&endpoints.ws_url).await.map_err(|e| e.to_string())?;
    let (mut up_tx, mut up_rx) = upstream_ws.split();
    loop {
        tokio::select! {
            frame = up_rx.next() => match frame {
                Some(Ok(Tm::Binary(b))) => {
                    use sha2::{Digest, Sha256};
                    let sha: String = Sha256::digest(&b).iter().take(8).map(|x| format!("{x:02x}")).collect();
                    let _ = socket.send(Message::text(json!({ "type": "frame_info", "bytes": b.len(), "sha256_8": sha }).to_string())).await;
                    if socket.send(Message::binary(b.to_vec())).await.is_err() { break; }
                }
                Some(Ok(Tm::Ping(p))) => { let _ = up_tx.send(Tm::Pong(p)).await; }
                Some(Ok(Tm::Close(_))) | None | Some(Err(_)) => break,
                _ => {}
            },
            incoming = socket.recv() => {
                let Some(Ok(Message::Text(text))) = incoming else { if matches!(incoming, Some(Ok(Message::Close(_))) | None | Some(Err(_))) { break } else { continue } };
                let Ok(cmd) = serde_json::from_str::<Value>(&text) else { continue };
                let f = |k: &str| cmd[k].as_f64().unwrap_or(0.0);
                let wire = match cmd["t"].as_str() {
                    Some("click") => Ok(bidi::encode_click(f("x"), f("y"), match cmd["button"].as_str() { Some("right") => bidi::MouseButton::Right, Some("middle") => bidi::MouseButton::Middle, _ => bidi::MouseButton::Left })),
                    Some("scroll") => Ok(bidi::encode_scroll(f("x"), f("y"), f("dx"), f("dy"))),
                    Some("key") => bidi::encode_key(cmd["key"].as_str().unwrap_or_default()),
                    Some("text") => bidi::encode_text(cmd["text"].as_str().unwrap_or_default()),
                    Some("nav") => bidi::encode_navigate(cmd["url"].as_str().unwrap_or_default()),
                    _ => continue,
                };
                match wire {
                    Ok(bytes) => {
                        tracing::info!(op = %cmd["t"], bytes = bytes.len(), x = f("x"), y = f("y"), "browser input from web HQ");
                        let _ = socket.send(Message::text(json!({ "type": "sent", "op": cmd["t"], "x": cmd["x"], "y": cmd["y"], "bytes": bytes.len() }).to_string())).await;
                        if up_tx.send(Tm::binary(bytes)).await.is_err() { break; }
                    }
                    Err(e) => { let _ = socket.send(Message::text(json!({ "type": "error", "text": e.to_string() }).to_string())).await; }
                }
            }
        }
    }
    Ok(())
}

