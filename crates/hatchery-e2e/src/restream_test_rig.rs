//! Shared test / fixture rig: local NodeServer + restreamer + core mesh + g4a bridge (A2).
//!
//! Replaces `C2Running` for suites that used C2 as a g4a relay. C2 here is only the
//! opaque restreamer; operator control is on core.

use hatchery_node::restream_g4a_bridge;
use hatchery_node::restream_session::UpstreamKeys;
use hatchery_c2::restreamer::{
    serve, BufferLimits, MemoryJournal, Restreamer, RestreamerConfig, RestreamerKeys,
};
use hatchery_c2_client::DrainClient;
use hatchery_c2_protocol::nested::HqRecipientSecret;
use hatchery_c2_protocol::{
    C2NodeResponse, ChannelKind, NodeCursor, NodeId, NodeRequest, NodeRoute,
};
use hatchery_server::drain_consumer::{DrainConsumer, NodeStream};
use hatchery_server::node_controller::{G4aMessage, OperatorLink};
use hatchery_server::operator_control::{serve_local, NodeOperatorMesh};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

const CORE_TOKEN: &str = "restream-rig-core-token-012345";
const DEFAULT_OP_TOKEN: &str = "restream-rig-op-token-0123456";
const G4A_CHANNEL: u32 = 1;

fn free_addr() -> SocketAddr {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
}

/// Live restream + core operator stack for one node.
pub struct RestreamRig {
    pub control_endpoint: String,
    pub operator_token: String,
    pub node_id: NodeId,
    pub mesh: NodeOperatorMesh,
    pub link: OperatorLink,
    edge_stop: watch::Sender<bool>,
    drain_stop: watch::Sender<bool>,
    op_stop: watch::Sender<bool>,
    bridge_stop: watch::Sender<bool>,
    _bridge: JoinHandle<Result<(), String>>,
    _rs: Restreamer,
}

pub struct RestreamRigConfig {
    pub node_id: NodeId,
    pub local_endpoint: String,
    pub local_token: String,
    pub control_endpoint: String,
    pub operator_token: String,
}

impl RestreamRig {
    /// Start restreamer, core mesh/drain/operator control, and the node g4a bridge.
    /// The local `NodeServer` must already be listening at `local_endpoint`.
    pub async fn start(cfg: RestreamRigConfig) -> Self {
        let operator_token = if cfg.operator_token.is_empty() {
            DEFAULT_OP_TOKEN.to_owned()
        } else {
            cfg.operator_token
        };
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
        let serve_cfg = RestreamerConfig {
            node_listen,
            drain_listen,
            buffer: BufferLimits::default(),
            core_token: CORE_TOKEN.into(),
        };
        let rs2 = rs.clone();
        tokio::spawn(async move {
            let _ = serve(serve_cfg, rs2, edge_rx).await;
        });
        tokio::time::sleep(Duration::from_millis(40)).await;

        let mesh = NodeOperatorMesh::new();
        let (events_tx, _events_rx) = mpsc::unbounded_channel();
        let (link, outbound) = OperatorLink::new(Some(events_tx));
        mesh.register(cfg.node_id.as_str(), link.clone());

        let (drain_stop, drain_rx) = watch::channel(false);
        let consumer = DrainConsumer::new(
            DrainClient::new(drain_listen, CORE_TOKEN),
            recipient.clone(),
            NodeStream {
                node_id: cfg.node_id.as_str().into(),
                channel_id: G4A_CHANNEL,
                kind: ChannelKind::G4aControl,
            },
            link.clone().handler(),
        )
        .with_outbound(outbound);
        tokio::spawn(consumer.run(drain_rx));

        let (op_stop, op_rx) = watch::channel(false);
        let ep = cfg.control_endpoint.clone();
        let mesh2 = mesh.clone();
        let token = operator_token.clone();
        tokio::spawn(async move {
            let _ = serve_local(ep, token, mesh2, op_rx).await;
        });

        let upstream = UpstreamKeys {
            c2_public: hatchery_c2_protocol::nested::PublicKeyBytes(c2_public.0),
            recipient_public: hatchery_c2_protocol::nested::PublicKeyBytes(recipient_public.0),
        };
        let (bridge_stop, bridge_rx) = watch::channel(false);
        let endpoint = cfg.local_endpoint.clone();
        let nid = cfg.node_id.as_str().to_owned();
        let local_token = cfg.local_token.clone();
        let bridge = tokio::spawn(async move {
            restream_g4a_bridge::run(
                endpoint,
                nid,
                local_token,
                node_listen,
                upstream,
                None,
                bridge_rx,
            )
            .await
        });

        // Learn incarnation from the local node (same process), publish on mesh, then
        // confirm Snapshot round-trips through the bridge.
        let local = gate4agent_node_wire::LocalNodeClient::connect(
            &cfg.local_endpoint,
            &cfg.node_id,
            gate4agent_node_protocol::ClientRole::Observer,
            &cfg.local_token,
        )
        .await
        .expect("local node for incarnation");
        let incarnation = local.hello().incarnation_id;
        drop(local);
        mesh.set_cursor(
            cfg.node_id.as_str(),
            NodeCursor {
                incarnation_id: incarnation,
                sequence: 0,
            },
        );

        let mut rig = Self {
            control_endpoint: cfg.control_endpoint,
            operator_token,
            node_id: cfg.node_id.clone(),
            mesh: mesh.clone(),
            link: link.clone(),
            edge_stop,
            drain_stop,
            op_stop,
            bridge_stop,
            _bridge: bridge,
            _rs: rs,
        };
        rig.wait_snapshot(Duration::from_secs(10)).await;
        rig
    }

    async fn wait_snapshot(&self, budget: Duration) {
        let deadline = tokio::time::Instant::now() + budget;
        let route = self.route();
        let routed = hatchery_c2_protocol::RoutedNodeRequest {
            route,
            request: NodeRequest::Snapshot,
        };
        loop {
            if tokio::time::Instant::now() > deadline {
                panic!("restream rig: Snapshot did not round-trip for {}", self.node_id);
            }
            match self.link.request(&routed, Duration::from_secs(2)).await {
                Ok(G4aMessage::Response(resp))
                    if matches!(resp.response, Ok(C2NodeResponse::Snapshot { .. })) =>
                {
                    return;
                }
                _ => tokio::time::sleep(Duration::from_millis(50)).await,
            }
        }
    }

    pub fn route(&self) -> NodeRoute {
        let status = self.mesh.subscribe().borrow().clone();
        let node = status
            .nodes
            .get(&self.node_id)
            .expect("node registered on mesh");
        let incarnation = node
            .cursor
            .expect("incarnation published")
            .incarnation_id;
        NodeRoute {
            node_id: self.node_id.clone(),
            expected_incarnation_id: incarnation,
        }
    }

    /// Token used for `connect_core_operator` / `HarnessC2Adapter::connect`.
    pub fn token(&self) -> &str {
        &self.operator_token
    }

    pub fn control_endpoint(&self) -> &str {
        &self.control_endpoint
    }

    /// Re-learn incarnation after the local NodeServer was restarted, then wait for Snapshot.
    pub async fn refresh_route_after_local_restart(
        &self,
        local_endpoint: &str,
        local_token: &str,
    ) -> NodeRoute {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
        let incarnation = loop {
            if tokio::time::Instant::now() > deadline {
                panic!("restream rig: local node did not accept Observer after restart");
            }
            match gate4agent_node_wire::LocalNodeClient::connect(
                local_endpoint,
                &self.node_id,
                gate4agent_node_protocol::ClientRole::Observer,
                local_token,
            )
            .await
            {
                Ok(local) => {
                    let incarnation = local.hello().incarnation_id;
                    drop(local);
                    break incarnation;
                }
                Err(_) => tokio::time::sleep(Duration::from_millis(50)).await,
            }
        };
        self.mesh.set_cursor(
            self.node_id.as_str(),
            NodeCursor {
                incarnation_id: incarnation,
                sequence: 0,
            },
        );
        self.wait_snapshot(Duration::from_secs(15)).await;
        self.route()
    }
}


impl Drop for RestreamRig {
    fn drop(&mut self) {
        let _ = self.bridge_stop.send(true);
        let _ = self.drain_stop.send(true);
        let _ = self.op_stop.send(true);
        let _ = self.edge_stop.send(true);
    }
}
