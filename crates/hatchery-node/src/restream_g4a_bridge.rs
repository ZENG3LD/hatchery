//! Node restream g4a bridge (A2).
//!
//! Tunnels the local g4a `NodeServer` through a [`crate::restream_session::NodeSession`].
//! Core speaks the OperatorLink wire (`{"op_id","g4a"}`); C2 only ever sees opaque
//! Noise ciphertext. The node never learns what sits behind C2.

use crate::restream_session::{NodeSession, UpstreamKeys};
use crate::uplink_journal::UplinkJournal;
use gate4agent_node_protocol::{
    ClientRole, NodeEventEnvelope, NodeFailureCode, NodeId, NodeIncarnationId, NodeRequest,
    NodeResponse, ServerFrame,
};
use gate4agent_node_wire::{LocalNodeClient, NodeClientError};
use hatchery_core::{
    BrowserClientOp, BrowserOpReply, BrowserStreamEvent, C2NodeEvent, C2NodeFailure,
    C2NodeResponse, ChannelKind, NodeCursor, RoutedNodeEvent, RoutedNodeRequest,
    RoutedNodeResponse,
};
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, watch};

const HANDSHAKE: Duration = Duration::from_secs(10);
const CONTROL_CHANNEL: u32 = 1;

/// Run until `shutdown` is true: local g4a operator client ↔ restream session.
pub async fn run(
    local_endpoint: impl Into<String>,
    node_id: impl Into<String>,
    local_token: impl Into<String>,
    uplink: SocketAddr,
    keys: UpstreamKeys,
    journal: Option<Arc<UplinkJournal>>,
    browser_profile_root: Option<PathBuf>,
    mut shutdown: watch::Receiver<bool>,
) -> Result<(), String> {
    let local_endpoint = local_endpoint.into();
    let node_id_str = node_id.into();
    let local_token = local_token.into();
    let node_id = NodeId::new(node_id_str.clone()).map_err(|e| e.to_string())?;
    let browser_profile_root = browser_profile_root.unwrap_or_else(|| {
        std::env::temp_dir().join("hatchery-browser-profiles")
    });

    let mut client =
        LocalNodeClient::connect(&local_endpoint, &node_id, ClientRole::Operator, &local_token)
            .await
            .map_err(|e| e.to_string())?;

    let mut session = NodeSession::connect(
        uplink,
        &node_id_str,
        CONTROL_CHANNEL,
        ChannelKind::G4aControl,
        keys,
        journal,
        HANDSHAKE,
    )
    .await?;

    let hello = client.hello().clone();
    let incarnation_id = hello.incarnation_id;
    let connection_id = hello.connection_id;
    tracing::info!(%node_id_str, connection_id, "restream g4a bridge up");

    // Requests run on their own local connection in a worker task. A request that
    // waits on the node (input settle, spawn readiness, snapshot) used to hold this
    // loop, and with it every PTY frame; now this connection only carries events.
    let worker_client =
        LocalNodeClient::connect(&local_endpoint, &node_id, ClientRole::Operator, &local_token)
            .await
            .map_err(|e| e.to_string())?;
    let (req_tx, req_rx) = mpsc::unbounded_channel::<Vec<u8>>();
    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<Vec<u8>>();
    let (browser_ev_tx, mut browser_ev_rx) = mpsc::unbounded_channel::<BrowserStreamEvent>();
    let (worker_stop_tx, worker_stop) = watch::channel(false);
    let mut worker = tokio::spawn(request_worker(
        worker_client,
        node_id.clone(),
        req_rx,
        out_tx,
        browser_profile_root.clone(),
        browser_ev_tx,
        worker_stop,
    ));
    let mut last_pty_input: Option<std::time::Instant> = None;
    let result = loop {
        if *shutdown.borrow() {
            break Ok(());
        }
        tokio::select! {
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() { break Ok(()); }
            }
            done = &mut worker => {
                break match done { Ok(r) => r.and(Err("request worker ended".into())), Err(e) => Err(e.to_string()) };
            }
            frame = client.recv() => {
                match frame {
                    Ok(ServerFrame::Event(envelope)) => {
                        if matches!(envelope.event, gate4agent_node_protocol::NodeEvent::TerminalFrame { .. }) {
                            if let Some(at) = last_pty_input.take() {
                                tracing::debug!(target: "pty_lat", bridge_us = at.elapsed().as_micros() as u64, "bridge: pty input received to echo frame forwarded");
                            }
                        }
                        if envelope.event.requires_harness_mcp_proxy_capability() { continue; }
                        let plain = event_bytes(&node_id, incarnation_id, envelope)?;
                        if let Err(e) = session.send(&plain).await { break Err(e); }
                    }
                    Ok(_) => break Err("local g4a sent an unexpected frame on the event connection".into()),
                    Err(err) => break Err(err.to_string()),
                }
            }
            out = out_rx.recv() => {
                let Some(out) = out else { break Err("request worker closed".into()) };
                if let Err(e) = session.send(&out).await { break Err(e); }
            }
            plain = session.recv(Duration::from_secs(120)) => {
                match plain {
                    Ok(bytes) => {
                        if last_pty_input.is_none() && is_pty_input(&bytes) { last_pty_input = Some(std::time::Instant::now()); }
                        if req_tx.send(bytes).is_err() { break Err("request worker closed".into()); }
                    }
                    Err(err) if err.contains("timeout") => continue,
                    Err(err) => break Err(err),
                }
            }
            ev = browser_ev_rx.recv() => {
                let Some(ev) = ev else { continue };
                let plain = serde_json::to_vec(&json!({ "browser": ev })).map_err(|e| e.to_string())?;
                if let Err(e) = session.send(&plain).await { break Err(e); }
            }
        }
    };
    // The worker releases the controller lease it holds before it exits.
    let _ = worker_stop_tx.send(true);
    let _ = tokio::time::timeout(Duration::from_secs(3), worker).await;
    result
}

fn is_pty_input(plain: &[u8]) -> bool {
    // Cheap probe (timing only); the worker does the real parsing.
    let s = String::from_utf8_lossy(&plain[..plain.len().min(512)]);
    s.contains("TerminalBytes") || s.contains("terminal_bytes")
}

type Sink = mpsc::UnboundedSender<Vec<u8>>;

/// All node requests, in arrival order, on a dedicated local connection.
/// PTY input is pipelined (sent at once, answered when its reply arrives);
/// anything else first settles in-flight input, then runs request/reply.
async fn request_worker(
    mut client: LocalNodeClient,
    node_id: NodeId,
    mut rx: mpsc::UnboundedReceiver<Vec<u8>>,
    out: Sink,
    browser_profile_root: PathBuf,
    browser_ev_tx: mpsc::UnboundedSender<BrowserStreamEvent>,
    mut stop: watch::Receiver<bool>,
) -> Result<(), String> {
    #[cfg(feature = "d2b")]
    let browsers = crate::browser_capture::shared_map();
    let hello = client.hello().clone();
    let mut incarnation_id = hello.incarnation_id;
    let connection_id = hello.connection_id;
    let mut controller_owned = hello.controller.as_ref().is_some_and(|c| c.connection_id == connection_id);
    let mut inflight: std::collections::HashMap<u64, (u64, NodeRequest)> = std::collections::HashMap::new();
    let mut retry: Vec<(u64, NodeRequest)> = Vec::new();
    // Events on this connection: only the controller-scoped harness MCP proxy
    // events are forwarded (the event connection does not receive them).
    let fwd = |out: &Sink, incarnation_id: NodeIncarnationId, envelope: NodeEventEnvelope| -> Result<(), String> {
        if envelope.event.requires_harness_mcp_proxy_capability() {
            out.send(event_bytes(&node_id, incarnation_id, envelope)?).map_err(|_| "bridge closed".to_string())?;
        }
        Ok(())
    };
    loop {
        tokio::select! {
            _ = stop.changed() => {
                let _ = release_controller(&mut client, &mut controller_owned).await;
                return Ok(());
            }
            frame = client.recv() => {
                match frame.map_err(|e| e.to_string())? {
                    ServerFrame::Event(envelope) => fwd(&out, incarnation_id, envelope)?,
                    ServerFrame::Reply(reply) => {
                        on_pipelined_reply(&out, &node_id, &mut incarnation_id, &mut controller_owned, &mut inflight, &mut retry, reply)?;
                    }
                    _ => return Err("local g4a sent an unexpected idle frame".into()),
                }
            }
            plain = rx.recv() => {
                let Some(bytes) = plain else {
                    let _ = release_controller(&mut client, &mut controller_owned).await;
                    return Ok(());
                };
                if controller_owned && retry.is_empty() {
                    if let Some((op_id, request)) = pipelinable(&bytes, &node_id, incarnation_id) {
                        let request_id = client.send(request.clone()).await.map_err(|e| e.to_string())?;
                        inflight.insert(request_id, (op_id, request));
                        continue;
                    }
                }
                while !inflight.is_empty() {
                    match client.recv().await.map_err(|e| e.to_string())? {
                        ServerFrame::Event(envelope) => fwd(&out, incarnation_id, envelope)?,
                        ServerFrame::Reply(reply) => on_pipelined_reply(&out, &node_id, &mut incarnation_id, &mut controller_owned, &mut inflight, &mut retry, reply)?,
                        _ => return Err("local g4a sent an unexpected idle frame".into()),
                    }
                }
                let mut queue: Vec<Vec<u8>> = Vec::new();
                for (op_id, request) in std::mem::take(&mut retry) {
                    // Controller lapsed under pipelined input: reacquire and replay in order.
                    let routed = RoutedNodeRequest {
                        route: hatchery_core::NodeRoute {
                            node_id: hatchery_core::NodeId::new(node_id.as_str().to_owned()).map_err(|e| e.to_string())?,
                            expected_incarnation_id: NodeIncarnationId::from_bytes([0; 16]),
                        },
                        request,
                    };
                    queue.push(serde_json::to_vec(&json!({ "op_id": op_id, "g4a": routed })).map_err(|e| e.to_string())?);
                }
                queue.push(bytes);
                for plain in queue {
                    let started = std::time::Instant::now();
                    let kind: String = serde_json::from_slice::<Value>(&plain).ok().and_then(|v| v.pointer("/g4a/request").and_then(|r| r.as_object().and_then(|o| o.keys().next().cloned()).or_else(|| r.as_str().map(str::to_owned)))).unwrap_or_else(|| "browser".into());
                    handle_control(&out, &mut client, &node_id, &mut incarnation_id, connection_id, &mut controller_owned, &plain, &browser_profile_root, browser_ev_tx.clone(), #[cfg(feature = "d2b")] &browsers).await?;
                    while let Some(envelope) = client.take_event() {
                        fwd(&out, incarnation_id, envelope)?;
                    }
                    if started.elapsed() > Duration::from_millis(500) {
                        tracing::warn!(request = %kind, elapsed_ms = started.elapsed().as_millis() as u64, "bridge: slow node request held the request queue");
                    }
                }
            }
        }
    }
}

/// PTY input that may be pipelined (no ordering dependency on replies).
fn pipelinable(plain: &[u8], node_id: &NodeId, incarnation_id: NodeIncarnationId) -> Option<(u64, NodeRequest)> {
    let v: Value = serde_json::from_slice(plain).ok()?;
    let op_id = v.get("op_id")?.as_u64()?;
    let routed: RoutedNodeRequest = serde_json::from_value(v.get("g4a")?.clone()).ok()?;
    if routed.route.node_id.as_str() != node_id.as_str() { return None; }
    let expected = routed.route.expected_incarnation_id;
    if expected != NodeIncarnationId::from_bytes([0; 16]) && expected != incarnation_id { return None; }
    match routed.request {
        NodeRequest::TerminalBytes { .. } | NodeRequest::TerminalControl { .. } | NodeRequest::Interrupt { .. } => Some((op_id, routed.request)),
        _ => None,
    }
}

fn on_pipelined_reply(
    out: &Sink,
    node_id: &NodeId,
    incarnation_id: &mut NodeIncarnationId,
    controller_owned: &mut bool,
    inflight: &mut std::collections::HashMap<u64, (u64, NodeRequest)>,
    retry: &mut Vec<(u64, NodeRequest)>,
    reply: gate4agent_node_protocol::ResponseEnvelope,
) -> Result<(), String> {
    let Some((op_id, request)) = inflight.remove(&reply.request_id) else {
        return Err(format!("unexpected local reply id {}", reply.request_id));
    };
    if let Err(f) = &reply.result {
        if f.code == NodeFailureCode::ControllerRequired {
            *controller_owned = false;
            retry.push((op_id, request));
            return Ok(());
        }
    }
    if !retry.is_empty() {
        // Keep order: everything after a lapsed one replays behind it.
        retry.push((op_id, request));
        return Ok(());
    }
    let mapped = reply
        .result
        .map(|r| C2NodeResponse::from_node_response_with_control_detail(&r))
        .map_err(|f| C2NodeFailure::from(&f));
    let plain = serde_json::to_vec(&json!({ "op_id": op_id, "g4a": RoutedNodeResponse { node_id: node_id.clone(), incarnation_id: *incarnation_id, response: mapped } })).map_err(|e| e.to_string())?;
    out.send(plain).map_err(|_| "bridge closed".to_string())
}

async fn handle_control(
    session: &Sink,
    client: &mut LocalNodeClient,
    node_id: &NodeId,
    incarnation_id: &mut NodeIncarnationId,
    connection_id: u64,
    controller_owned: &mut bool,
    plain: &[u8],
    browser_profile_root: &std::path::Path,
    browser_ev_tx: mpsc::UnboundedSender<BrowserStreamEvent>,
    #[cfg(feature = "d2b")] browsers: &std::sync::Arc<crate::browser_capture::BrowserSessionMap>,
) -> Result<(), String> {
    let v: Value = serde_json::from_slice(plain).map_err(|e| e.to_string())?;
    let Some(op_id) = v.get("op_id").and_then(Value::as_u64) else {
        return Ok(());
    };
    if let Some(browser) = v.get("browser") {
        let op: BrowserClientOp = serde_json::from_value(browser.clone()).map_err(|e| e.to_string())?;
        let reply = handle_browser_op(
            &node_id_str_from(node_id),
            op,
            browser_profile_root,
            browser_ev_tx,
            #[cfg(feature = "d2b")]
            browsers,
        )
        .await;
        let plain = serde_json::to_vec(&json!({ "op_id": op_id, "browser": reply })).map_err(|e| e.to_string())?;
        return session.send(plain).map_err(|_| "bridge closed".to_string());
    }
    let g4a = v.get("g4a").cloned().unwrap_or(Value::Null);
    let routed: RoutedNodeRequest = serde_json::from_value(g4a).map_err(|e| e.to_string())?;
    if routed.route.node_id.as_str() != node_id.as_str() {
        return send_reply(
            session,
            op_id,
            &failure_response(node_id, *incarnation_id, NodeFailureCode::InvalidRequest, "route node id does not match bridge"),
        )
        .await;
    }
    // All-zero expected incarnation is the core cursor-bootstrap wildcard.
    let expected = routed.route.expected_incarnation_id;
    if expected != NodeIncarnationId::from_bytes([0; 16]) && expected != *incarnation_id {
        return send_reply(
            session,
            op_id,
            &failure_response(
                node_id,
                *incarnation_id,
                NodeFailureCode::InvalidRequest,
                "node incarnation changed",
            ),
        )
        .await;
    }

    let request = routed.request;
    let controller_gated = !is_read_only(&request);
    if controller_gated && !*controller_owned {
        match acquire_controller(client, connection_id).await {
            Ok(true) => *controller_owned = true,
            Ok(false) => {
                return send_reply(
                    session,
                    op_id,
                    &failure_response(
                        node_id,
                        *incarnation_id,
                        NodeFailureCode::ControllerBusy,
                        "node controller lease is unavailable",
                    ),
                )
                .await;
            }
            Err(err) => return Err(err.to_string()),
        }
    }

    let deadline = request_deadline(&request);
    // The node controller lease is short-lived and the node clears it on
    // expiry, but `controller_owned` stays set on this long-lived bridge. So a
    // controller-gated request can come back `ControllerRequired` even though
    // we believe we hold it (lease lapsed, or the local client reconnected
    // under a new connection id). Re-acquire once and retry so live PTY input
    // and fresh spawns keep working on a held stream.
    let mut remaining_retries = 1u8;
    let response = loop {
        match tokio::time::timeout(deadline, client.request(request.clone())).await {
            Ok(Ok(response)) => break Ok(response),
            Ok(Err(NodeClientError::Node(failure))) => {
                if controller_gated
                    && failure.code == NodeFailureCode::ControllerRequired
                    && remaining_retries > 0
                {
                    remaining_retries -= 1;
                    *controller_owned = false;
                    match acquire_controller(client, connection_id).await {
                        Ok(true) => {
                            *controller_owned = true;
                            continue;
                        }
                        Ok(false) => break Err(failure),
                        Err(err) => return Err(err.to_string()),
                    }
                }
                break Err(failure);
            }
            Ok(Err(err)) => return Err(err.to_string()),
            Err(_) => {
                return send_reply(
                    session,
                    op_id,
                    &failure_response(
                        node_id,
                        *incarnation_id,
                        NodeFailureCode::HostDirectoryReadTimedOut,
                        "node request deadline exceeded",
                    ),
                )
                .await;
            }
        }
    };

    *incarnation_id = client.hello().incarnation_id;

    let mut mapped = response
        .map(|r| C2NodeResponse::from_node_response_with_control_detail(&r))
        .map_err(|f| C2NodeFailure::from(&f));
    if let Ok(C2NodeResponse::Snapshot { snapshot, .. }) = &mut mapped {
        snapshot.machine = Some(crate::collect_machine_environment());
    }
    send_reply(
        session,
        op_id,
        &RoutedNodeResponse {
            node_id: node_id.clone(),
            incarnation_id: *incarnation_id,
            response: mapped,
        },
    )
    .await
}

fn failure_response(
    node_id: &NodeId,
    incarnation_id: NodeIncarnationId,
    code: NodeFailureCode,
    message: &str,
) -> RoutedNodeResponse {
    RoutedNodeResponse {
        node_id: node_id.clone(),
        incarnation_id,
        response: Err(C2NodeFailure {
            code,
            message: message.into(),
        }),
    }
}

async fn send_reply(
    session: &Sink,
    op_id: u64,
    response: &RoutedNodeResponse,
) -> Result<(), String> {
    let plain =
        serde_json::to_vec(&json!({ "op_id": op_id, "g4a": response })).map_err(|e| e.to_string())?;
    session.send(plain).map_err(|_| "bridge closed".to_string())
}

fn event_bytes(
    node_id: &NodeId,
    incarnation_id: NodeIncarnationId,
    envelope: NodeEventEnvelope,
) -> Result<Vec<u8>, String> {
    let event = RoutedNodeEvent {
        node_id: node_id.clone(),
        cursor: NodeCursor {
            incarnation_id,
            sequence: envelope.sequence,
        },
        event: C2NodeEvent::from_node_event_with_control_detail(&envelope.event),
    };
    serde_json::to_vec(&json!({ "g4a": event })).map_err(|e| e.to_string())
}

async fn acquire_controller(
    client: &mut LocalNodeClient,
    connection_id: u64,
) -> Result<bool, NodeClientError> {
    match client
        .request(NodeRequest::AcquireController {
            lease_ms: gate4agent_node_protocol::MAX_CONTROLLER_LEASE_MS,
        })
        .await?
    {
        NodeResponse::Controller { controller } => Ok(controller
            .as_ref()
            .is_some_and(|state| state.connection_id == connection_id)),
        _ => Err(NodeClientError::Protocol(
            "controller acquisition returned a different response".into(),
        )),
    }
}

async fn release_controller(
    client: &mut LocalNodeClient,
    controller_owned: &mut bool,
) -> Result<(), NodeClientError> {
    if !*controller_owned {
        return Ok(());
    }
    let _ = client.request(NodeRequest::ReleaseController).await?;
    *controller_owned = false;
    Ok(())
}

fn is_read_only(request: &NodeRequest) -> bool {
    matches!(
        request,
        NodeRequest::Snapshot
            | NodeRequest::Resync { .. }
            | NodeRequest::BrowseHostDirectories { .. }
            | NodeRequest::InspectWorkspace { .. }
            | NodeRequest::ReadWorkspaceFile { .. }
            | NodeRequest::ReadGitHistory { .. }
            | NodeRequest::ReadGitDiff { .. }
            | NodeRequest::CatalogNativeSessions { .. }
            | NodeRequest::PageNativeSessions { .. }
            | NodeRequest::PreviewNativeSession { .. }
            | NodeRequest::PreviewSessionRecord { .. }
    )
}

fn request_deadline(request: &NodeRequest) -> Duration {
    match request {
        NodeRequest::CreateStandaloneWorkspace { .. }
        | NodeRequest::CreateWorktree { .. }
        | NodeRequest::RemoveWorktree { .. }
        | NodeRequest::CleanupManagedWorktree { .. } => Duration::from_secs(240),
        NodeRequest::Spawn { .. }
        | NodeRequest::Resume { .. }
        | NodeRequest::Stop { .. }
        | NodeRequest::SpawnSpec { .. }
        | NodeRequest::SpawnSpecWithHarnessMcp { .. }
        | NodeRequest::SpawnManagedWorktree { .. } => Duration::from_secs(60),
        NodeRequest::InspectWorkspace { .. } => Duration::from_secs(15),
        _ => Duration::from_secs(30),
    }
}


fn node_id_str_from(node_id: &NodeId) -> String {
    node_id.as_str().to_owned()
}

async fn handle_browser_op(
    node_id: &str,
    op: BrowserClientOp,
    browser_profile_root: &std::path::Path,
    _browser_ev_tx: mpsc::UnboundedSender<BrowserStreamEvent>,
    #[cfg(feature = "d2b")] browsers: &std::sync::Arc<crate::browser_capture::BrowserSessionMap>,
) -> BrowserOpReply {
    #[cfg(not(feature = "d2b"))]
    {
        let _ = (node_id, op, browser_profile_root, _browser_ev_tx);
        return BrowserOpReply {
            ok: false,
            session: String::new(),
            message: "dig2browser feature (d2b) is not enabled in this node build".into(),
        };
    }
    #[cfg(feature = "d2b")]
    {
        match op {
            BrowserClientOp::Start {
                session,
                url,
                profile_dir,
            } => {
                let dir = match profile_dir {
                    Some(p) if !p.is_empty() => PathBuf::from(p),
                    _ => browser_profile_root.join(&session),
                };
                match browsers
                    .start(node_id.to_owned(), session.clone(), url, dir, _browser_ev_tx)
                    .await
                {
                    Ok(()) => BrowserOpReply {
                        ok: true,
                        session,
                        message: String::new(),
                    },
                    Err(message) => BrowserOpReply {
                        ok: false,
                        session,
                        message,
                    },
                }
            }
            BrowserClientOp::Input { session, bytes } => match browsers.input(&session, &bytes).await
            {
                Ok(()) => BrowserOpReply {
                    ok: true,
                    session,
                    message: String::new(),
                },
                Err(message) => BrowserOpReply {
                    ok: false,
                    session,
                    message,
                },
            },
            BrowserClientOp::Stop { session } => match browsers.stop(&session).await {
                Ok(()) => BrowserOpReply {
                    ok: true,
                    session,
                    message: String::new(),
                },
                Err(message) => BrowserOpReply {
                    ok: false,
                    session,
                    message,
                },
            },
        }
    }
}
