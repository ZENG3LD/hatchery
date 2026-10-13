//! A2: local g4a NodeServer tunnels through restream_session; core operator
//! reaches it via DrainConsumer + OperatorLink + NodeOperatorMesh.
#![cfg(unix)]

use hatchery_c2::restreamer::{
    serve, BufferLimits, MemoryJournal, Restreamer, RestreamerConfig, RestreamerKeys,
};
use hatchery_c2_client::connect_core_operator;
use hatchery_c2_client::DrainClient;
use hatchery_core::nested::HqRecipientSecret;
use hatchery_core::{
    C2NodeResponse, ChannelKind, NodeId, NodeRequest, NodeRoute, NodeTransportState,
};
use hatchery_node::restream_session::UpstreamKeys;
use hatchery_node::{restream_g4a_bridge, NodeServer, NodeServerConfig, WorkspaceConfig};
use hatchery_node::protocol::WorkspaceId;
use hatchery_server::drain_consumer::{DrainConsumer, NodeStream};
use hatchery_server::node_controller::OperatorLink;
use hatchery_server::operator_control::{serve_local, NodeOperatorMesh};
use std::fs;
use std::net::SocketAddr;
use std::os::unix::fs::DirBuilderExt;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, watch};

const CORE_TOKEN: &str = "bridge-e2e-core-token-012345";
const NODE_TOKEN: &str = "bridge-e2e-node-token-012345";
const OP_TOKEN: &str = "bridge-e2e-op-token-01234567";

fn free_addr() -> SocketAddr {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
}

struct Tree {
    base: PathBuf,
    endpoint: String,
    control: String,
    workspace: PathBuf,
}

impl Tree {
    fn new() -> Self {
        let base = std::env::temp_dir().join(format!(
            "restream-g4a-bridge-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::DirBuilder::new().mode(0o700).create(&base).unwrap();
        let sock = base.join("sock");
        fs::DirBuilder::new().mode(0o700).create(&sock).unwrap();
        let workspace = base.join("ws");
        fs::create_dir(&workspace).unwrap();
        Self {
            endpoint: sock.join("node.sock").to_string_lossy().into_owned(),
            control: sock.join("core.sock").to_string_lossy().into_owned(),
            workspace,
            base,
        }
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn snapshot_reaches_local_g4a_through_restream_bridge() {
    let tree = Tree::new();
    let node_id = NodeId::new("bridge-node").unwrap();
    let ws = WorkspaceId::new("primary").unwrap();
    let config = NodeServerConfig::new(
        &tree.endpoint,
        NODE_TOKEN,
        node_id.clone(),
        [WorkspaceConfig::new(ws, &tree.workspace).unwrap()],
    )
    .unwrap();
    let server = NodeServer::new(config).unwrap();
    let node_shutdown = server.shutdown_handle();
    let node_task = tokio::spawn(server.run());
    tokio::time::sleep(Duration::from_millis(80)).await;

    let recipient = HqRecipientSecret::generate();
    let keys = RestreamerKeys::generate(hatchery_c2::restreamer::PublicKeyBytes(
        recipient.public_bytes().0,
    ));
    let (c2_public, recipient_public) = keys.upstream_public_keys();
    let rs = Restreamer::new(
        BufferLimits::default(),
        Arc::new(MemoryJournal::new()),
        CORE_TOKEN,
        Some(keys),
    );
    let (node_listen, drain_listen) = (free_addr(), free_addr());
    let (edge_stop, edge_rx) = watch::channel(false);
    let cfg = RestreamerConfig {
        node_listen,
        drain_listen,
        buffer: BufferLimits::default(),
        core_token: CORE_TOKEN.into(),
    };
    let rs2 = rs.clone();
    tokio::spawn(async move {
        let _ = serve(cfg, rs2, edge_rx).await;
    });
    tokio::time::sleep(Duration::from_millis(40)).await;

    let mesh = NodeOperatorMesh::new();
    let (events_tx, _events_rx) = mpsc::unbounded_channel();
    let (link, outbound) = OperatorLink::new(Some(events_tx));
    mesh.register(node_id.as_str(), link.clone());

    let (drain_stop, drain_rx) = watch::channel(false);
    let consumer = DrainConsumer::new(
        DrainClient::new(drain_listen, CORE_TOKEN),
        recipient.clone(),
        NodeStream {
            node_id: node_id.as_str().into(),
            channel_id: 1,
            kind: ChannelKind::G4aControl,
        },
        link.handler(),
    )
    .with_outbound(outbound);
    tokio::spawn(consumer.run(drain_rx));

    let (op_stop, op_rx) = watch::channel(false);
    let ep = tree.control.clone();
    let mesh2 = mesh.clone();
    tokio::spawn(async move {
        let _ = serve_local(ep, OP_TOKEN, mesh2, op_rx).await;
    });

    let upstream = UpstreamKeys {
        c2_public: hatchery_core::nested::PublicKeyBytes(c2_public.0),
        recipient_public: hatchery_core::nested::PublicKeyBytes(recipient_public.0),
    };
    let (bridge_stop, bridge_rx) = watch::channel(false);
    let endpoint = tree.endpoint.clone();
    let nid = node_id.as_str().to_owned();
    let bridge = tokio::spawn(async move {
        restream_g4a_bridge::run(
            endpoint,
            nid,
            NODE_TOKEN,
            node_listen,
            upstream,
            None,
            bridge_rx,
        )
        .await
    });
    // Wait for Noise handshake (bridge dials after drain consumer is listening).
    tokio::time::sleep(Duration::from_millis(200)).await;

    let (control, _) = tokio::time::timeout(
        Duration::from_secs(5),
        connect_core_operator(&tree.control, OP_TOKEN),
    )
    .await
    .expect("operator connect timeout")
    .expect("operator connect");
    assert_eq!(
        control.current_topology().nodes[0].transport,
        NodeTransportState::Online
    );

    // Need the node's real incarnation from a first probe — bridge answers Snapshot.
    // Use a dummy incarnation first? Operator mesh dispatches with expected from route.
    // Fetch via requesting with a wrong incarnation would fail; hello from local node
    // isn't on the operator control wire. Ask Snapshot with incarnation from a soft probe:
    // the bridge rejects stale incarnation via RoutedNodeResponse Err — so we need the
    // real one. Read it from the local node directly once.
    let local = gate4agent_node_wire::LocalNodeClient::connect(
        &tree.endpoint,
        &node_id,
        gate4agent_node_protocol::ClientRole::Observer,
        NODE_TOKEN,
    )
    .await
    .unwrap();
    let incarnation = local.hello().incarnation_id;
    drop(local);

    let routed = control
        .request(
            NodeRoute {
                node_id: node_id.clone(),
                expected_incarnation_id: incarnation,
            },
            NodeRequest::Snapshot,
        )
        .await
        .unwrap();
    assert_eq!(routed.node_id, node_id);
    assert!(matches!(
        routed.response,
        Ok(C2NodeResponse::Snapshot { .. })
    ));
    assert!(!rs.ever_pushed_to_core());

    let _ = bridge_stop.send(true);
    let _ = drain_stop.send(true);
    let _ = op_stop.send(true);
    let _ = edge_stop.send(true);
    node_shutdown.request_shutdown().await.unwrap();
    let _ = tokio::time::timeout(Duration::from_secs(3), node_task).await;
    let _ = tokio::time::timeout(Duration::from_secs(2), bridge).await;
}
