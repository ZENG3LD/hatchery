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
    C2NodeEvent, C2NodeFailure, C2NodeResponse, ChannelKind, NodeCursor, RoutedNodeEvent,
    RoutedNodeRequest, RoutedNodeResponse,
};
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;

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
    mut shutdown: watch::Receiver<bool>,
) -> Result<(), String> {
    let local_endpoint = local_endpoint.into();
    let node_id_str = node_id.into();
    let local_token = local_token.into();
    let node_id = NodeId::new(node_id_str.clone()).map_err(|e| e.to_string())?;

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
    let mut incarnation_id = hello.incarnation_id;
    let connection_id = hello.connection_id;
    let mut controller_owned = hello
        .controller
        .as_ref()
        .is_some_and(|c| c.connection_id == connection_id);

    tracing::info!(%node_id_str, connection_id, "restream g4a bridge up");

    loop {
        if *shutdown.borrow() {
            let _ = release_controller(&mut client, &mut controller_owned).await;
            return Ok(());
        }
        tokio::select! {
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    let _ = release_controller(&mut client, &mut controller_owned).await;
                    return Ok(());
                }
            }
            frame = client.recv() => {
                match frame {
                    Ok(ServerFrame::Event(envelope)) => {
                        forward_event(&mut session, &node_id, incarnation_id, envelope).await?;
                    }
                    Ok(_) => return Err("local g4a sent an unexpected idle frame".into()),
                    Err(err) => return Err(err.to_string()),
                }
            }
            plain = session.recv(Duration::from_secs(120)) => {
                match plain {
                    Ok(bytes) => {
                        handle_control(
                            &mut session,
                            &mut client,
                            &node_id,
                            &mut incarnation_id,
                            connection_id,
                            &mut controller_owned,
                            &bytes,
                        )
                        .await?;
                        while let Some(envelope) = client.take_event() {
                            forward_event(&mut session, &node_id, incarnation_id, envelope).await?;
                        }
                    }
                    Err(err) if err.contains("timeout") => continue,
                    Err(err) => return Err(err),
                }
            }
        }
    }
}

async fn handle_control(
    session: &mut NodeSession,
    client: &mut LocalNodeClient,
    node_id: &NodeId,
    incarnation_id: &mut NodeIncarnationId,
    connection_id: u64,
    controller_owned: &mut bool,
    plain: &[u8],
) -> Result<(), String> {
    let v: Value = serde_json::from_slice(plain).map_err(|e| e.to_string())?;
    let Some(op_id) = v.get("op_id").and_then(Value::as_u64) else {
        return Ok(());
    };
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
    if !is_read_only(&request) && !*controller_owned {
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
    let response = match tokio::time::timeout(deadline, client.request(request)).await {
        Ok(Ok(response)) => Ok(response),
        Ok(Err(NodeClientError::Node(failure))) => Err(failure),
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
    session: &mut NodeSession,
    op_id: u64,
    response: &RoutedNodeResponse,
) -> Result<(), String> {
    let plain =
        serde_json::to_vec(&json!({ "op_id": op_id, "g4a": response })).map_err(|e| e.to_string())?;
    session.send(&plain).await
}

async fn forward_event(
    session: &mut NodeSession,
    node_id: &NodeId,
    incarnation_id: NodeIncarnationId,
    envelope: NodeEventEnvelope,
) -> Result<(), String> {
    let event = RoutedNodeEvent {
        node_id: node_id.clone(),
        cursor: NodeCursor {
            incarnation_id,
            sequence: envelope.sequence,
        },
        event: C2NodeEvent::from_node_event_with_control_detail(&envelope.event),
    };
    let plain = serde_json::to_vec(&json!({ "g4a": event })).map_err(|e| e.to_string())?;
    session.send(&plain).await
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
