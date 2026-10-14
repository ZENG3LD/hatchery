//! A2: Spawn a host shell PTY over the drain and observe live terminal bytes.
#![cfg(unix)]

use gate4agent_node_protocol::{AgentId, NodeIncarnationId, SessionMode, WorkspaceId};
use gate4agent_types::TerminalSize;
use hatchery_c2_client::connect_core_operator;
use hatchery_core::{C2NodeEvent, C2NodeResponse, NodeId, NodeRequest, NodeRoute};
use hatchery_e2e::restream_test_rig::{RestreamRig, RestreamRigConfig};
use hatchery_node::{NodeServer, NodeServerConfig, WorkspaceConfig};
use std::fs;
use std::os::unix::fs::DirBuilderExt;
use std::path::PathBuf;
use std::time::Duration;

const NODE_TOKEN: &str = "spawn-pty-e2e-node-token-0123";
const OP_TOKEN: &str = "spawn-pty-e2e-op-token-012345";

struct Tree {
    base: PathBuf,
    endpoint: String,
    control: String,
    workspace: PathBuf,
}

impl Tree {
    fn new() -> Self {
        let base = std::env::temp_dir().join(format!(
            "restream-spawn-pty-{}-{}",
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
async fn spawn_shell_pty_streams_bytes_over_drain() {
    let _ = tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::from_default_env()).with_test_writer().try_init();
    let tree = Tree::new();
    let node_id = NodeId::new("spawn-pty-node").unwrap();
    let ws = WorkspaceId::new("primary").unwrap();
    let config = NodeServerConfig::new(
        &tree.endpoint,
        NODE_TOKEN,
        node_id.clone(),
        [WorkspaceConfig::new(ws.clone(), &tree.workspace).unwrap()],
    )
    .unwrap();
    let server = NodeServer::new(config).unwrap();
    let shutdown = server.shutdown_handle();
    let node_task = tokio::spawn(server.run());
    tokio::time::sleep(Duration::from_millis(100)).await;

    let _rig = RestreamRig::start(RestreamRigConfig {
        node_id: node_id.clone(),
        local_endpoint: tree.endpoint.clone(),
        local_token: NODE_TOKEN.into(),
        control_endpoint: tree.control.clone(),
        operator_token: OP_TOKEN.into(),
    })
    .await;

    let (control, mut events) = connect_core_operator(&tree.control, OP_TOKEN)
        .await
        .expect("operator connect");

    // Wildcard expected incarnation (same path HQ uses after the fix).
    let route = NodeRoute {
        node_id: node_id.clone(),
        expected_incarnation_id: NodeIncarnationId::from_bytes([0; 16]),
    };
    let spawned = control
        .request(
            route.clone(),
            NodeRequest::Spawn {
                workspace_id: ws,
                provider: AgentId::new("shell").unwrap(),
                mode: SessionMode::Pty,
                terminal_size: TerminalSize {
                    rows: 24,
                    columns: 80,
                },
                initial_prompt: None,
            },
        )
        .await
        .expect("spawn request");
    let session = match spawned.response {
        Ok(C2NodeResponse::SpawnAccepted { session }) => session,
        other => panic!("spawn refused: {other:?}"),
    };
    let mut route = route;
    route.expected_incarnation_id = spawned.incarnation_id;

    // Spawn is accepted before the PTY is up: input sent right away can race the start
    // (refused or stuck). Wait for the first terminal frame (the shell's prompt) first.
    let ready_by = tokio::time::Instant::now() + Duration::from_secs(10);
    while tokio::time::Instant::now() < ready_by {
        match tokio::time::timeout(Duration::from_millis(500), events.recv()).await {
            Ok(Some(ev)) if matches!(ev.event, C2NodeEvent::TerminalFrame { .. }) => break,
            Ok(None) => break,
            _ => continue,
        }
    }

    let marker = format!("HATCH_PTY_{}", std::process::id());
    let cmd = format!("echo {marker}\n");
    for chunk in cmd.as_bytes().chunks(64) {
        let reply = control
            .request(
                route.clone(),
                NodeRequest::TerminalBytes {
                    session: session.clone(),
                    bytes: chunk.to_vec(),
                },
            )
            .await
            .expect("terminal bytes");
        assert!(
            reply.response.is_ok(),
            "TerminalBytes failed: {:?}",
            reply.response
        );
    }

    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    let mut saw_marker = false;
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_millis(500), events.recv()).await {
            Ok(Some(ev)) => {
                let blob = serde_json::to_string(&ev).unwrap_or_default();
                if blob.contains(&marker) {
                    saw_marker = true;
                    break;
                }
                if let C2NodeEvent::TerminalFrame { frame, .. } = &ev.event {
                    if frame.contents.contains(&marker) {
                        saw_marker = true;
                        break;
                    }
                }
            }
            Ok(None) => break,
            Err(_) => continue,
        }
    }
    assert!(saw_marker, "did not observe marker {marker} in PTY events");

    let _ = control
        .request(
            route,
            NodeRequest::Stop {
                session,
                force: true,
            },
        )
        .await;
    let _ = shutdown.request_shutdown().await;
    let _ = node_task.await;
}
