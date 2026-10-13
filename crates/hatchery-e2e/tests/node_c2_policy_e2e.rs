//! A2: core operator mesh refuses requests for nodes not on the drain.
#![cfg(unix)]

use hatchery_c2_client::{connect_core_operator, C2ControlError};
use hatchery_core::{NodeId, NodeIncarnationId, NodeRequest, NodeRoute};
use hatchery_server::operator_control::{serve_local, NodeOperatorMesh};
use std::os::unix::fs::DirBuilderExt;
use std::time::Duration;
use tokio::sync::watch;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn core_operator_reports_node_offline_when_absent_from_mesh() {
    let root = tempfile::tempdir().unwrap();
    let sockdir = root.path().join("sock");
    std::fs::DirBuilder::new().mode(0o700).create(&sockdir).unwrap();
    let endpoint = sockdir.join("core.sock");
    let mesh = NodeOperatorMesh::new();
    let (stop, rx) = watch::channel(false);
    let ep = endpoint.to_string_lossy().into_owned();
    tokio::spawn(async move { serve_local(ep, "policy-token-0123456", mesh, rx).await });
    tokio::time::sleep(Duration::from_millis(50)).await;

    let (control, _) = connect_core_operator(endpoint.to_str().unwrap(), "policy-token-0123456")
        .await
        .unwrap();
    let err = control
        .request(
            NodeRoute {
                node_id: NodeId::new("missing-node").unwrap(),
                expected_incarnation_id: NodeIncarnationId::from_bytes([0u8; 16]),
            },
            NodeRequest::Snapshot,
        )
        .await
        .unwrap_err();
    match err {
        C2ControlError::Relay(failure) => {
            assert!(
                failure.message.contains("not on drain") || failure.message.contains("offline"),
                "{failure:?}"
            );
        }
        other => panic!("expected relay failure, got {other:?}"),
    }
    let _ = stop.send(true);
}
