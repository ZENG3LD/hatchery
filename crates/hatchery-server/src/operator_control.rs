//! Core operator control (A2): same client wire as the former C2 control socket,
//! but requests go through [`crate::node_controller::OperatorLink`] over the drain.
//! C2 never sees this socket and never parses g4a.

use crate::node_controller::{G4aMessage, OperatorLink};
use gate4agent_node_protocol::{
    read_json_frame_limited_body_timeout, write_json_frame_limited, FrameError,
};
use gate4agent_node_wire::{local_hmac_sha256, proofs_match, random_nonce, OwnerOnlyLocalListener};
use hatchery_core::{
    c2_auth_transcript, c2_bound_auth_transcript, ArchitectureId, BUILD_STAMP, BrowserClientCommand,
    BrowserStreamEvent, C2AuthDirection,
    C2ClientAuthentication, C2ClientFrame, C2ClientHello, C2ControlCompatibilitySupport, C2Hello,
    C2RelayFailure, C2RelayFailureCode, C2ReplyEnvelope, C2RequestEnvelope, C2ServerChallenge,
    C2ServerFrame, C2Topology, CapabilityId, ClientCompatibilityOffer, HostDescriptor,
    NegotiatedC2ControlCompatibility, NodeFreshness, NodeId, NodeTransportState, ObservedNode,
    OperatingSystemId, PathEncoding, PathSemantics, PathStyle, RoutedNodeRequest, RoutedNodeResponse,
    StatusResponse, C2_ACP_CONTROL_CAPABILITY, C2_AGENT_PROGRESS_SNAPSHOT_CAPABILITY,
    C2_AGENT_STREAM_EVENTS_CAPABILITY, C2_API_VERSION, C2_CHILD_ENVIRONMENT_PROFILE_CAPABILITY,
    C2_COMPATIBILITY_METADATA_CAPABILITY, C2_CONTROL_DETAIL_CAPABILITY,
    C2_DELIVERY_BUNDLE_V2_STAGE_COMMIT_CAPABILITY, C2_GIT_READ_CAPABILITY,
    C2_HARNESS_MCP_READ_PROXY_CAPABILITY, C2_HISTORY_CONTEXT_PACK_CAPABILITY,
    C2_HOST_DIRECTORY_BROWSE_CAPABILITY, C2_MANAGED_WORKTREE_LIFECYCLE_CAPABILITY,
    C2_MANAGED_WORKTREE_SPAWN_V2_CAPABILITY, C2_NATIVE_SESSION_CATALOG_CAPABILITY,
    C2_NATIVE_SESSION_CATALOG_PAGING_CAPABILITY, C2_NATIVE_SESSION_INDEX_CAPABILITY,
    C2_NATIVE_SESSION_PREVIEW_CAPABILITY, C2_OPAQUE_UNIX_PATH_CAPABILITY,
    C2_PROVIDER_CONTRACT_MANIFEST_CAPABILITY, C2_PROVIDER_ID_OPEN_CAPABILITY,
    C2_PROVIDER_RUNTIME_STATUS_CAPABILITY, C2_PROVIDER_SESSION_REFERENCE_INDEX_CAPABILITY,
    C2_REPOSITORY_PATH_CAPABILITY, C2_SESSION_BUNDLE_MATERIALIZATION_CAPABILITY,
    C2_SESSION_RECORD_CONTEXT_EXPORT_CAPABILITY, C2_SESSION_TASK_CORRELATION_CAPABILITY,
    C2_SPAWN_PROFILE_REVISION_CAPABILITY, C2_SPAWN_SPEC_DEFAULTS_OVERRIDES_CAPABILITY,
    C2_STANDALONE_WORKSPACE_LIFECYCLE_CAPABILITY, C2_TERMINAL_FRAME_EVENTS_CAPABILITY,
    C2_WORKSPACE_ENTRY_CREATE_CAPABILITY, C2_WORKSPACE_FILE_READ_CAPABILITY,
    C2_WORKSPACE_FILE_WRITE_CAPABILITY, C2_WORKTREE_SELECTION_CAPABILITY,
    MAX_C2_AUTH_FRAME_BYTES, MAX_C2_CLIENT_FRAME_BYTES, MAX_C2_HELLO_FRAME_BYTES,
    MAX_C2_SERVER_FRAME_BYTES,
};
use std::collections::BTreeMap;
use std::io;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{broadcast, watch, Semaphore};
#[cfg(test)]
use tokio::sync::mpsc;
use tokio::task::JoinSet;
use tokio::time::timeout;

const AUTH_DEADLINE: Duration = Duration::from_secs(5);
const REQUEST_DEADLINE: Duration = Duration::from_secs(60);

/// Live node control sessions on core (one OperatorLink per node).
#[derive(Clone)]
pub struct NodeOperatorMesh {
    inner: Arc<Mutex<BTreeMap<String, OperatorLink>>>,
    cursors: Arc<Mutex<BTreeMap<String, hatchery_core::NodeCursor>>>,
    machines: Arc<Mutex<BTreeMap<String, hatchery_core::MachineEnvironment>>>,
    topology: watch::Sender<Arc<StatusResponse>>,
    /// Fan-out of node-initiated g4a events (PTY frames, etc.) to every operator client.
    events: broadcast::Sender<hatchery_core::RoutedNodeEvent>,
    /// Fan-out of dig2browser frames / lifecycle events.
    browser: broadcast::Sender<BrowserStreamEvent>,
}

impl NodeOperatorMesh {
    pub fn new() -> Self {
        let initial = Arc::new(StatusResponse {
            api_version: C2_API_VERSION,
            ready: true,
            observed_at_unix_ms: unix_ms(),
            nodes: BTreeMap::new(),
        });
        let (topology, _) = watch::channel(initial);
        let (events, _) = broadcast::channel(256);
        let (browser, _) = broadcast::channel(64);
        Self {
            inner: Arc::new(Mutex::new(BTreeMap::new())),
            cursors: Arc::new(Mutex::new(BTreeMap::new())),
            machines: Arc::new(Mutex::new(BTreeMap::new())),
            topology,
            events,
            browser,
        }
    }

    pub fn subscribe(&self) -> watch::Receiver<Arc<StatusResponse>> {
        self.topology.subscribe()
    }

    pub fn register(&self, node_id: impl Into<String>, link: OperatorLink) {
        let id = node_id.into();
        let _ = NodeId::new(id.clone()).expect("operator mesh node id");
        self.inner.lock().expect("mesh").insert(id.clone(), link);
        self.publish_topology();
        tracing::info!(%id, "core operator mesh: node registered");
    }

    pub fn unregister(&self, node_id: &str) {
        self.inner.lock().expect("mesh").remove(node_id);
        self.cursors.lock().expect("cursors").remove(node_id);
        self.machines.lock().expect("machines").remove(node_id);
        self.publish_topology();
    }

    pub fn get(&self, node_id: &str) -> Option<OperatorLink> {
        self.inner.lock().expect("mesh").get(node_id).cloned()
    }

    /// Publish the node's incarnation (from Snapshot / bridge hello) into operator topology.
    pub fn set_cursor(&self, node_id: &str, cursor: hatchery_core::NodeCursor) {
        self.cursors.lock().expect("cursors").insert(node_id.to_owned(), cursor);
        self.publish_topology();
    }

    pub fn set_machine(&self, node_id: &str, machine: hatchery_core::MachineEnvironment) {
        self.machines.lock().expect("machines").insert(node_id.to_owned(), machine);
        self.publish_topology();
    }

    /// Publish a node-initiated event to every connected operator client.
    pub fn publish_event(&self, event: hatchery_core::RoutedNodeEvent) {
        // Keep topology cursor fresh from live traffic.
        self.set_cursor(
            event.node_id.as_str(),
            hatchery_core::NodeCursor {
                incarnation_id: event.cursor.incarnation_id,
                sequence: event.cursor.sequence,
            },
        );
        let _ = self.events.send(event);
    }

    pub fn subscribe_events(&self) -> broadcast::Receiver<hatchery_core::RoutedNodeEvent> {
        self.events.subscribe()
    }

    pub fn publish_browser(&self, event: BrowserStreamEvent) {
        let _ = self.browser.send(event);
    }

    pub fn subscribe_browser(&self) -> broadcast::Receiver<BrowserStreamEvent> {
        self.browser.subscribe()
    }

    fn publish_topology(&self) {
        let ids: Vec<String> = self.inner.lock().expect("mesh").keys().cloned().collect();
        let cursors = self.cursors.lock().expect("cursors").clone();
        let machines = self.machines.lock().expect("machines").clone();
        let mut nodes = BTreeMap::new();
        for id in &ids {
            let node_id = NodeId::new(id.clone()).expect("registered ids are valid");
            nodes.insert(
                node_id,
                ObservedNode {
                    endpoint: "drain".into(),
                    transport_label: "drain".into(),
                    transport: NodeTransportState::Online,
                    freshness: NodeFreshness::Fresh,
                    cursor: cursors.get(id).cloned(),
                    inventory: None,
                    machine: machines.get(id).cloned(),
                    last_attempt_unix_ms: None,
                    last_success_unix_ms: Some(unix_ms()),
                    consecutive_failures: 0,
                    last_error: None,
                    gaps: Vec::new(),
                    gaps_truncated: 0,
                },
            );
        }
        let status = Arc::new(StatusResponse {
            api_version: C2_API_VERSION,
            ready: true,
            observed_at_unix_ms: unix_ms(),
            nodes,
        });
        self.topology.send_replace(status);
    }
}

fn unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Serve the operator control wire on an owner-only local endpoint (core, not C2).
pub async fn serve_local(
    endpoint: impl Into<String>,
    token: impl Into<String>,
    mesh: NodeOperatorMesh,
    mut shutdown: watch::Receiver<bool>,
) -> io::Result<()> {
    let endpoint = endpoint.into();
    let token = token.into();
    let mut listener = OwnerOnlyLocalListener::bind(&endpoint).await?;
    tracing::info!(%endpoint, "core operator control listening");
    let preauth = Arc::new(Semaphore::new(4));
    let authenticated = Arc::new(Semaphore::new(8));
    let next_id = Arc::new(AtomicU64::new(1));
    let mut connections = JoinSet::new();
    loop {
        let permit = tokio::select! {
            p = Arc::clone(&preauth).acquire_owned() => p.map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "preauth closed"))?,
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() { break; }
                continue;
            }
        };
        let stream = tokio::select! {
            a = listener.accept() => a?,
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() { break; }
                continue;
            }
        };
        let token = token.clone();
        let mesh = mesh.clone();
        let authenticated = Arc::clone(&authenticated);
        let next_id = Arc::clone(&next_id);
        let sd = shutdown.clone();
        connections.spawn(async move {
            let _ = serve_connection(stream, permit, authenticated, next_id, &token, mesh, sd).await;
        });
        while connections.try_join_next().is_some() {}
    }
    connections.shutdown().await;
    Ok(())
}

async fn serve_connection<S>(
    mut pipe: S,
    preauth: tokio::sync::OwnedSemaphorePermit,
    authenticated: Arc<Semaphore>,
    connection_ids: Arc<AtomicU64>,
    token: &str,
    mesh: NodeOperatorMesh,
    mut shutdown: watch::Receiver<bool>,
) -> Result<(), FrameError>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let hello = timeout(AUTH_DEADLINE, read_client_frame(&mut pipe, MAX_C2_AUTH_FRAME_BYTES))
        .await.map_err(|_| FrameError::PrefixTimedOut)??;
    let C2ClientFrame::Hello(hello) = hello else { return Ok(()) };
    if hello.build_stamp != BUILD_STAMP {
        let _ = timeout(AUTH_DEADLINE, write_json_frame_limited(
            &mut pipe,
            &C2ServerFrame::Rejected(fail(C2RelayFailureCode::BuildStampMismatch, "build stamp mismatch")),
            MAX_C2_AUTH_FRAME_BYTES,
        )).await;
        return Ok(());
    }
    let negotiated = negotiate(&hello)?;
    let compatibility = hello.compatibility.as_ref().map(|_| negotiated.clone());
    let server_nonce = random_nonce().map_err(|e| FrameError::Io(io::Error::other(e)))?;
    let auth_compat = hello.compatibility.as_ref().zip(compatibility.as_ref());
    let server_proof = c2_proof(token, C2AuthDirection::Server, &hello.client_nonce, &server_nonce, auth_compat)?;
    timeout(AUTH_DEADLINE, write_json_frame_limited(
        &mut pipe,
        &C2ServerFrame::Challenge(C2ServerChallenge {
            build_stamp: BUILD_STAMP.to_owned(),
            server_nonce,
            server_proof,
            compatibility: compatibility.clone(),
        }),
        MAX_C2_AUTH_FRAME_BYTES,
    )).await.map_err(|_| FrameError::BodyTimedOut { length: 0 })??;

    let authentication = timeout(AUTH_DEADLINE, read_client_frame(&mut pipe, MAX_C2_AUTH_FRAME_BYTES))
        .await.map_err(|_| FrameError::PrefixTimedOut)??;
    let C2ClientFrame::Authenticate(C2ClientAuthentication { client_proof }) = authentication else {
        return Ok(());
    };
    let expected = c2_proof(token, C2AuthDirection::Client, &hello.client_nonce, &server_nonce, auth_compat)?;
    if !proofs_match(&client_proof, &expected) {
        return Ok(());
    }
    drop(preauth);

    let Ok(_op) = authenticated.try_acquire_owned() else {
        let _ = write_json_frame_limited(
            &mut pipe,
            &C2ServerFrame::Rejected(fail(C2RelayFailureCode::OperatorAlreadyConnected, "operator already connected")),
            MAX_C2_SERVER_FRAME_BYTES,
        ).await;
        return Ok(());
    };

    let connection_id = connection_ids.fetch_add(1, Ordering::AcqRel);
    let mut status_rx = mesh.subscribe();
    let mut events_rx = mesh.subscribe_events();
    let mut browser_rx = mesh.subscribe_browser();
    let hello_status = status_rx.borrow().as_ref().clone();
    timeout(AUTH_DEADLINE, write_json_frame_limited(
        &mut pipe,
        &C2ServerFrame::Hello(C2Hello {
            build_stamp: BUILD_STAMP.to_owned(),
            connection_id,
            status: hello_status,
            compatibility,
        }),
        MAX_C2_HELLO_FRAME_BYTES,
    )).await.map_err(|_| FrameError::BodyTimedOut { length: 0 })??;

    let (mut reader, mut writer) = tokio::io::split(pipe);
    // read_client_frame is not cancellation-safe: inside select! a busy event/browser
    // branch could drop a half-read frame, desync the stream and close the operator
    // connection. Read frames on their own task and hand complete ones over.
    let (frame_tx, mut frame_rx) = tokio::sync::mpsc::channel(64);
    // Requests and browser ops run on their own tasks so a slow node RPC never
    // stalls frame/event delivery on this connection; replies come back here.
    let (out_tx, mut out_rx) = tokio::sync::mpsc::unbounded_channel::<C2ServerFrame>();
    let reader_task = tokio::spawn(async move {
        loop {
            let frame = read_client_frame(&mut reader, MAX_C2_CLIENT_FRAME_BYTES).await;
            let stop = frame.is_err();
            if frame_tx.send(frame).await.is_err() || stop {
                break;
            }
        }
    });
    let mut last_topology = C2Topology::from_status(status_rx.borrow().as_ref());
    tracing::info!(connection_id, "core operator control opened");

    loop {
        tokio::select! {
            _ = shutdown.changed() => {
                if *shutdown.borrow() { break; }
            }
            changed = status_rx.changed() => {
                if changed.is_err() { break; }
                let status = status_rx.borrow().as_ref().clone();
                let topology = C2Topology::from_status(&status);
                if topology != last_topology {
                    last_topology = topology.clone();
                    if write_json_frame_limited(&mut writer, &C2ServerFrame::Topology(topology), MAX_C2_SERVER_FRAME_BYTES).await.is_err() {
                        break;
                    }
                }
            }
            event = events_rx.recv() => {
                match event {
                    Ok(routed) => {
                        if write_json_frame_limited(
                            &mut writer,
                            &C2ServerFrame::Event(routed),
                            MAX_C2_SERVER_FRAME_BYTES,
                        ).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            browser = browser_rx.recv() => {
                match browser {
                    Ok(ev) => {
                        if write_json_frame_limited(
                            &mut writer,
                            &C2ServerFrame::Browser(ev),
                            MAX_C2_SERVER_FRAME_BYTES,
                        ).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            out = out_rx.recv() => {
                let Some(out) = out else { break };
                if write_json_frame_limited(&mut writer, &out, MAX_C2_SERVER_FRAME_BYTES).await.is_err() {
                    break;
                }
            }
            frame = frame_rx.recv() => {
                let Some(frame) = frame else { break };
                match frame {
                    Ok(C2ClientFrame::Request(envelope)) => {
                        let (mesh, out) = (mesh.clone(), out_tx.clone());
                        tokio::spawn(async move {
                            let reply = dispatch_request(&mesh, envelope).await;
                            let _ = out.send(C2ServerFrame::Reply(reply));
                        });
                    }
                    Ok(C2ClientFrame::Browser(cmd)) => {
                        // Browser ops are acked via Browser stream events (Error on failure).
                        let (mesh, out) = (mesh.clone(), out_tx.clone());
                        tokio::spawn(async move {
                            if let Some(ev) = dispatch_browser(&mesh, cmd).await {
                                let _ = out.send(C2ServerFrame::Browser(ev));
                            }
                        });
                    }
                    Ok(_) | Err(_) => break,
                }
            }
        }
    }
    reader_task.abort();
    Ok(())
}


async fn dispatch_browser(
    mesh: &NodeOperatorMesh,
    cmd: BrowserClientCommand,
) -> Option<BrowserStreamEvent> {
    let link = {
        let guard = mesh.inner.lock().expect("mesh");
        guard.get(&cmd.node_id).cloned()
    };
    let Some(link) = link else {
        return Some(BrowserStreamEvent::Error {
            node_id: cmd.node_id,
            session: match &cmd.op {
                hatchery_core::BrowserClientOp::Start { session, .. }
                | hatchery_core::BrowserClientOp::Input { session, .. }
                | hatchery_core::BrowserClientOp::Stop { session } => session.clone(),
            },
            message: "node is not registered on core mesh".into(),
        });
    };
    match link.browser_op(&cmd.op, Duration::from_secs(60)).await {
        Ok(reply) if reply.ok => None,
        Ok(reply) => Some(BrowserStreamEvent::Error {
            node_id: cmd.node_id,
            session: reply.session,
            message: reply.message,
        }),
        Err(err) => Some(BrowserStreamEvent::Error {
            node_id: cmd.node_id,
            session: match &cmd.op {
                hatchery_core::BrowserClientOp::Start { session, .. }
                | hatchery_core::BrowserClientOp::Input { session, .. }
                | hatchery_core::BrowserClientOp::Stop { session } => session.clone(),
            },
            message: err.to_string(),
        }),
    }
}

async fn dispatch_request(mesh: &NodeOperatorMesh, envelope: C2RequestEnvelope) -> C2ReplyEnvelope {
    let node_id = envelope.request.route.node_id.as_str().to_owned();
    let Some(link) = mesh.get(&node_id) else {
        return C2ReplyEnvelope {
            request_id: envelope.request_id,
            result: Err(fail(C2RelayFailureCode::NodeOffline, "node not on drain")),
        };
    };
    let routed = RoutedNodeRequest {
        route: envelope.request.route.clone(),
        request: envelope.request.request.clone(),
    };
    match link.request(&routed, REQUEST_DEADLINE).await {
        Ok(G4aMessage::Response(response)) => {
            mesh.set_cursor(
                &node_id,
                hatchery_core::NodeCursor {
                    incarnation_id: response.incarnation_id,
                    sequence: 0,
                },
            );
            if let Ok(hatchery_core::C2NodeResponse::Snapshot { snapshot, .. }) = &response.response {
                if let Some(machine) = snapshot.machine.clone() {
                    mesh.set_machine(&node_id, machine);
                }
            }
            C2ReplyEnvelope {
                request_id: envelope.request_id,
                result: Ok(response),
            }
        }
        Ok(G4aMessage::Json(v)) => {
            // Forward-compatible: try as RoutedNodeResponse
            match serde_json::from_value::<RoutedNodeResponse>(v) {
                Ok(response) => {
                    mesh.set_cursor(
                        &node_id,
                        hatchery_core::NodeCursor {
                            incarnation_id: response.incarnation_id,
                            sequence: 0,
                        },
                    );
                    C2ReplyEnvelope { request_id: envelope.request_id, result: Ok(response) }
                }
                Err(_) => C2ReplyEnvelope {
                    request_id: envelope.request_id,
                    result: Err(fail(C2RelayFailureCode::RequestForbidden, "node reply was not a g4a response")),
                },
            }
        }
        Ok(_) => C2ReplyEnvelope {
            request_id: envelope.request_id,
            result: Err(fail(C2RelayFailureCode::RequestForbidden, "unexpected g4a reply kind")),
        },
        Err(_) => C2ReplyEnvelope {
            request_id: envelope.request_id,
            result: Err(fail(C2RelayFailureCode::RelayBusy, "node did not answer")),
        },
    }
}

fn fail(code: C2RelayFailureCode, message: &str) -> C2RelayFailure {
    C2RelayFailure { code, message: message.to_owned(), current_incarnation_id: None }
}

fn negotiate(hello: &C2ClientHello) -> Result<NegotiatedC2ControlCompatibility, FrameError> {
    let support = C2ControlCompatibilitySupport {
        build_stamp: BUILD_STAMP.to_owned(),
        capabilities: vec![
            CapabilityId::new(C2_COMPATIBILITY_METADATA_CAPABILITY).map_err(|e| FrameError::Io(io::Error::other(e.to_string())))?,
            CapabilityId::new(C2_OPAQUE_UNIX_PATH_CAPABILITY).map_err(|e| FrameError::Io(io::Error::other(e.to_string())))?,
            CapabilityId::new(C2_REPOSITORY_PATH_CAPABILITY).map_err(|e| FrameError::Io(io::Error::other(e.to_string())))?,
            CapabilityId::new(C2_WORKSPACE_FILE_READ_CAPABILITY).map_err(|e| FrameError::Io(io::Error::other(e.to_string())))?,
            CapabilityId::new(C2_WORKSPACE_FILE_WRITE_CAPABILITY).map_err(|e| FrameError::Io(io::Error::other(e.to_string())))?,
            CapabilityId::new(C2_WORKSPACE_ENTRY_CREATE_CAPABILITY).map_err(|e| FrameError::Io(io::Error::other(e.to_string())))?,
            CapabilityId::new(C2_GIT_READ_CAPABILITY).map_err(|e| FrameError::Io(io::Error::other(e.to_string())))?,
            CapabilityId::new(C2_PROVIDER_CONTRACT_MANIFEST_CAPABILITY).map_err(|e| FrameError::Io(io::Error::other(e.to_string())))?,
            CapabilityId::new(C2_PROVIDER_RUNTIME_STATUS_CAPABILITY).map_err(|e| FrameError::Io(io::Error::other(e.to_string())))?,
            CapabilityId::new(C2_PROVIDER_ID_OPEN_CAPABILITY).map_err(|e| FrameError::Io(io::Error::other(e.to_string())))?,
            CapabilityId::new(C2_SPAWN_SPEC_DEFAULTS_OVERRIDES_CAPABILITY).map_err(|e| FrameError::Io(io::Error::other(e.to_string())))?,
            CapabilityId::new(C2_SPAWN_PROFILE_REVISION_CAPABILITY).map_err(|e| FrameError::Io(io::Error::other(e.to_string())))?,
            CapabilityId::new(C2_TERMINAL_FRAME_EVENTS_CAPABILITY).map_err(|e| FrameError::Io(io::Error::other(e.to_string())))?,
            CapabilityId::new(C2_AGENT_STREAM_EVENTS_CAPABILITY).map_err(|e| FrameError::Io(io::Error::other(e.to_string())))?,
            CapabilityId::new(C2_WORKTREE_SELECTION_CAPABILITY).map_err(|e| FrameError::Io(io::Error::other(e.to_string())))?,
            CapabilityId::new(C2_MANAGED_WORKTREE_LIFECYCLE_CAPABILITY).map_err(|e| FrameError::Io(io::Error::other(e.to_string())))?,
            CapabilityId::new(C2_MANAGED_WORKTREE_SPAWN_V2_CAPABILITY).map_err(|e| FrameError::Io(io::Error::other(e.to_string())))?,
            CapabilityId::new(C2_CHILD_ENVIRONMENT_PROFILE_CAPABILITY).map_err(|e| FrameError::Io(io::Error::other(e.to_string())))?,
            CapabilityId::new(C2_SESSION_BUNDLE_MATERIALIZATION_CAPABILITY).map_err(|e| FrameError::Io(io::Error::other(e.to_string())))?,
            CapabilityId::new(C2_HISTORY_CONTEXT_PACK_CAPABILITY).map_err(|e| FrameError::Io(io::Error::other(e.to_string())))?,
            CapabilityId::new(C2_SESSION_RECORD_CONTEXT_EXPORT_CAPABILITY).map_err(|e| FrameError::Io(io::Error::other(e.to_string())))?,
            CapabilityId::new(C2_NATIVE_SESSION_CATALOG_CAPABILITY).map_err(|e| FrameError::Io(io::Error::other(e.to_string())))?,
            CapabilityId::new(C2_NATIVE_SESSION_CATALOG_PAGING_CAPABILITY).map_err(|e| FrameError::Io(io::Error::other(e.to_string())))?,
            CapabilityId::new(C2_NATIVE_SESSION_INDEX_CAPABILITY).map_err(|e| FrameError::Io(io::Error::other(e.to_string())))?,
            CapabilityId::new(C2_NATIVE_SESSION_PREVIEW_CAPABILITY).map_err(|e| FrameError::Io(io::Error::other(e.to_string())))?,
            CapabilityId::new(C2_HOST_DIRECTORY_BROWSE_CAPABILITY).map_err(|e| FrameError::Io(io::Error::other(e.to_string())))?,
            CapabilityId::new(C2_STANDALONE_WORKSPACE_LIFECYCLE_CAPABILITY).map_err(|e| FrameError::Io(io::Error::other(e.to_string())))?,
            CapabilityId::new(C2_PROVIDER_SESSION_REFERENCE_INDEX_CAPABILITY).map_err(|e| FrameError::Io(io::Error::other(e.to_string())))?,
            CapabilityId::new(C2_AGENT_PROGRESS_SNAPSHOT_CAPABILITY).map_err(|e| FrameError::Io(io::Error::other(e.to_string())))?,
            CapabilityId::new(C2_SESSION_TASK_CORRELATION_CAPABILITY).map_err(|e| FrameError::Io(io::Error::other(e.to_string())))?,
            CapabilityId::new(C2_CONTROL_DETAIL_CAPABILITY).map_err(|e| FrameError::Io(io::Error::other(e.to_string())))?,
            CapabilityId::new(C2_DELIVERY_BUNDLE_V2_STAGE_COMMIT_CAPABILITY).map_err(|e| FrameError::Io(io::Error::other(e.to_string())))?,
            CapabilityId::new(C2_HARNESS_MCP_READ_PROXY_CAPABILITY).map_err(|e| FrameError::Io(io::Error::other(e.to_string())))?,
            CapabilityId::new(C2_ACP_CONTROL_CAPABILITY).map_err(|e| FrameError::Io(io::Error::other(e.to_string())))?,
        ],
        host: HostDescriptor {
            operating_system: OperatingSystemId::new(std::env::consts::OS)
                .map_err(|e| FrameError::Io(io::Error::other(e.to_string())))?,
            architecture: ArchitectureId::new(std::env::consts::ARCH)
                .map_err(|e| FrameError::Io(io::Error::other(e.to_string())))?,
        },
        path_semantics: PathSemantics {
            style: if cfg!(windows) { PathStyle::Windows } else { PathStyle::Posix },
            encoding: if cfg!(windows) { PathEncoding::Utf8 } else { PathEncoding::UnixBytes },
        },
    };
    support.negotiate(hello).map_err(|e| FrameError::Io(io::Error::other(e.to_string())))
}

fn c2_proof(
    token: &str,
    direction: C2AuthDirection,
    client_nonce: &[u8; 32],
    server_nonce: &[u8; 32],
    compatibility: Option<(&ClientCompatibilityOffer, &NegotiatedC2ControlCompatibility)>,
) -> Result<[u8; 32], FrameError> {
    let transcript = match compatibility {
        Some((offer, selected)) => c2_bound_auth_transcript(direction, client_nonce, server_nonce, offer, selected)
            .map_err(|e| FrameError::Io(io::Error::other(e.to_string())))?,
        None => c2_auth_transcript(direction, client_nonce, server_nonce),
    };
    local_hmac_sha256(token.as_bytes(), &transcript).map_err(|e| FrameError::Io(io::Error::other(e)))
}

async fn read_client_frame<S: tokio::io::AsyncRead + Unpin>(
    stream: &mut S,
    max: usize,
) -> Result<C2ClientFrame, FrameError> {
    read_json_frame_limited_body_timeout(stream, max, AUTH_DEADLINE).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use hatchery_c2_client::connect_local;
        use std::time::Duration;

    #[tokio::test]
    async fn clients_connect_to_core_operator_not_c2() {
        let root = tempfile::tempdir().unwrap();
        let sockdir = root.path().join("sock");
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            std::fs::DirBuilder::new().mode(0o700).create(&sockdir).unwrap();
        }
        #[cfg(not(unix))]
        std::fs::create_dir_all(&sockdir).unwrap();
        let endpoint = sockdir.join("core.sock");
        let mesh = NodeOperatorMesh::new();
        let (events_tx, _events_rx) = mpsc::unbounded_channel();
        let (link, _outbound) = OperatorLink::new(Some(events_tx));
        // Fake node that answers inventory-ish JSON as a RoutedNodeResponse shape via G4aMessage::Json
        // Register link; dispatch will wait — spawn a responder on a drain-like loop using the outbound isn't
        // wired here. Instead register nothing and expect NodeOffline.
        mesh.register("unix-local", link);

        let (stop, rx) = watch::channel(false);
        let endpoint_s = endpoint.to_string_lossy().into_owned();
        let mesh2 = mesh.clone();
        tokio::spawn(async move { serve_local(endpoint_s, "core-op-token-0123456789", mesh2, rx).await });
        // Ensure topology is published after the server subscribed.
        mesh.register("unix-local", {
            let (events_tx, _) = mpsc::unbounded_channel();
            OperatorLink::new(Some(events_tx)).0
        });
        tokio::time::sleep(Duration::from_millis(80)).await;

        let (control, _ev) = tokio::time::timeout(
            Duration::from_secs(5),
            connect_local(endpoint.to_str().unwrap(), "core-op-token-0123456789"),
        )
        .await
        .expect("timeout")
        .expect("connect");
        assert_eq!(control.hello().build_stamp, BUILD_STAMP);
        let hello_nodes: Vec<_> = control.hello().status.nodes.keys().map(|k| k.as_str().to_owned()).collect();
        assert!(hello_nodes.iter().any(|n| n == "unix-local"), "hello status nodes={hello_nodes:?}");
        let topo = control.current_topology();
        assert!(!topo.nodes.is_empty() || !hello_nodes.is_empty(), "topology={:?} hello={hello_nodes:?}", topo.nodes);
        let _ = stop.send(true);
    }
}
