//! Core operator control + OperatorLink request/reply (A2).
#![cfg(unix)]

use hatchery_c2_client::connect_core_operator;
use hatchery_core::{
    C2NodeResponse, C2NodeSnapshot, NodeId, NodeIncarnationId, NodeRequest, NodeRoute,
    NodeTransportState, RoutedNodeResponse,
};
use hatchery_server::node_controller::OperatorLink;
use hatchery_server::operator_control::{serve_local, NodeOperatorMesh};
use serde_json::json;
use std::os::unix::fs::DirBuilderExt;
use std::time::Duration;
use tokio::sync::{mpsc, watch};

fn incarnation() -> NodeIncarnationId {
    NodeIncarnationId::from_bytes([9u8; 16])
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn operator_request_round_trips_through_operator_link() {
    let root = tempfile::tempdir().unwrap();
    let sockdir = root.path().join("sock");
    std::fs::DirBuilder::new().mode(0o700).create(&sockdir).unwrap();
    let endpoint = sockdir.join("core.sock");

    let mesh = NodeOperatorMesh::new();
    let (events_tx, _events_rx) = mpsc::unbounded_channel();
    let (link, mut outbound) = OperatorLink::new(Some(events_tx));
    let answering = link.clone();
    tokio::spawn(async move {
        while let Some(bytes) = outbound.recv().await {
            let req: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            let op_id = req["op_id"].as_u64().unwrap();
            let response = RoutedNodeResponse {
                node_id: NodeId::new("node-a").unwrap(),
                incarnation_id: incarnation(),
                response: Ok(C2NodeResponse::Snapshot {
                    event_sequence: 0,
                    controller: None,
                    snapshot: C2NodeSnapshot {
                        node_id: NodeId::new("node-a").unwrap(),
                        enabled_providers: vec![],
                        provider_runtime_statuses: Default::default(),
                        workspaces: vec![],
                        session_records: vec![],
                        agent_progress: vec![],
                        managed_worktrees: vec![],
                        launch_inventory: None,

                    machine: None,},
                }),
            };
            let plain = serde_json::to_vec(&json!({ "op_id": op_id, "g4a": response })).unwrap();
            let h = answering.clone().handler();
            let _ = h("node-a", hatchery_core::ChannelKind::G4aControl, &plain);
        }
    });
    mesh.register("node-a", link);

    let (stop, rx) = watch::channel(false);
    let ep = endpoint.to_string_lossy().into_owned();
    let mesh2 = mesh.clone();
    tokio::spawn(async move { serve_local(ep, "tok-0123456789abcdef", mesh2, rx).await });
    tokio::time::sleep(Duration::from_millis(50)).await;

    let (control, _) = connect_core_operator(endpoint.to_str().unwrap(), "tok-0123456789abcdef")
        .await
        .unwrap();
    assert_eq!(control.current_topology().nodes[0].transport, NodeTransportState::Online);

    let routed = control
        .request(
            NodeRoute {
                node_id: NodeId::new("node-a").unwrap(),
                expected_incarnation_id: incarnation(),
            },
            NodeRequest::Snapshot,
        )
        .await
        .unwrap();
    assert_eq!(routed.node_id, NodeId::new("node-a").unwrap());
    assert!(matches!(routed.response, Ok(C2NodeResponse::Snapshot { .. })));

    let _ = stop.send(true);
}
