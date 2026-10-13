//! Production wiring for [`NodeOperatorMesh`] (A2).
//!
//! When drain + operator control are configured, core:
//! 1. listens for operator clients on an owner-only local endpoint;
//! 2. opens one drain consumer per configured node id (g4a control);
//! 3. registers each [`OperatorLink`] on the mesh so operator requests reach the node.
//!
//! Optional: m4a channel consumers share the same node list when a hub is present.
//! Secrets are read from named environment variables and scrubbed; never logged.

use crate::drain_consumer::{DrainConsumer, NodeStream};
use crate::node_controller::OperatorLink;
use crate::operator_control::{serve_local, NodeOperatorMesh};
use hatchery_c2_client::DrainClient;
use hatchery_core::nested::HqRecipientSecret;
use hatchery_core::{ChannelKind, NodeRequest, NodeRoute, NodeId, NodeCursor, RoutedNodeRequest};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

pub const DRAIN_ADDR_ENV: &str = "HATCHERY_CORE_DRAIN_ADDR";
pub const DRAIN_TOKEN_VAR_ENV: &str = "HATCHERY_CORE_DRAIN_TOKEN_VAR";
pub const OPERATOR_LISTEN_ENV: &str = "HATCHERY_CORE_OPERATOR_LISTEN";
pub const OPERATOR_TOKEN_VAR_ENV: &str = "HATCHERY_CORE_OPERATOR_TOKEN_VAR";
pub const NODE_IDS_ENV: &str = "HATCHERY_CORE_NODE_IDS";
pub const RECIPIENT_SECRET_VAR_ENV: &str = "HATCHERY_CORE_RECIPIENT_SECRET_VAR";

const DEFAULT_DRAIN_TOKEN_VAR: &str = "HATCHERY_CORE_DRAIN_TOKEN";
const DEFAULT_OPERATOR_TOKEN_VAR: &str = "GATE4AGENT_CORE_OPERATOR_TOKEN";
const G4A_CONTROL_CHANNEL: u32 = 1;
const M4A_CHANNEL: u32 = 2;

/// Optional production mesh configuration (all-or-nothing for the drain path).
#[derive(Clone)]
pub struct MeshConfig {
    pub drain_addr: SocketAddr,
    pub drain_token: String,
    pub operator_listen: String,
    pub operator_token: String,
    pub node_ids: Vec<String>,
    pub recipient: HqRecipientSecret,
}

impl MeshConfig {
    /// Resolve from the environment. Returns `Ok(None)` when drain is not configured.
    /// Token values are taken from the named vars and those vars are removed from the process env.
    pub fn from_env(data_dir: &Path) -> Result<Option<Self>, String> {
        let Some(drain_addr) = std::env::var(DRAIN_ADDR_ENV).ok().filter(|s| !s.is_empty()) else {
            return Ok(None);
        };
        let drain_addr: SocketAddr = drain_addr
            .parse()
            .map_err(|e| format!("{DRAIN_ADDR_ENV}: {e}"))?;
        let operator_listen = std::env::var(OPERATOR_LISTEN_ENV)
            .map_err(|_| format!("{OPERATOR_LISTEN_ENV} is required with {DRAIN_ADDR_ENV}"))?;
        if operator_listen.is_empty() {
            return Err(format!("{OPERATOR_LISTEN_ENV} is empty"));
        }
        let node_ids = std::env::var(NODE_IDS_ENV)
            .map_err(|_| format!("{NODE_IDS_ENV} is required with {DRAIN_ADDR_ENV}"))?
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect::<Vec<_>>();
        if node_ids.is_empty() {
            return Err(format!("{NODE_IDS_ENV} must list at least one node id"));
        }

        let drain_token_var =
            std::env::var(DRAIN_TOKEN_VAR_ENV).unwrap_or_else(|_| DEFAULT_DRAIN_TOKEN_VAR.into());
        let operator_token_var = std::env::var(OPERATOR_TOKEN_VAR_ENV)
            .unwrap_or_else(|_| DEFAULT_OPERATOR_TOKEN_VAR.into());
        let drain_token = take_secret(&drain_token_var)?;
        let operator_token = take_secret(&operator_token_var)?;
        if drain_token.len() < 16 || operator_token.len() < 16 {
            return Err("drain and operator tokens must be at least 16 bytes".into());
        }
        let recipient = load_or_create_recipient(data_dir)?;
        Ok(Some(Self {
            drain_addr,
            drain_token,
            operator_listen,
            operator_token,
            node_ids,
            recipient,
        }))
    }
}

fn take_secret(var: &str) -> Result<String, String> {
    let value = std::env::var(var).map_err(|_| format!("{var} is not set"))?;
    std::env::remove_var(var);
    if value.is_empty() {
        return Err(format!("{var} is empty"));
    }
    Ok(value)
}

fn load_or_create_recipient(data_dir: &Path) -> Result<HqRecipientSecret, String> {
    if let Ok(var) = std::env::var(RECIPIENT_SECRET_VAR_ENV) {
        if !var.is_empty() {
            let secret = take_secret(&var)?;
            return parse_recipient_hex(&secret);
        }
    }
    let path = data_dir.join("core-recipient.seed");
    if path.exists() {
        let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
        if bytes.len() != 32 {
            return Err(format!("{}: expected 32 bytes", path.display()));
        }
        let mut seed = [0u8; 32];
        seed.copy_from_slice(&bytes);
        return Ok(HqRecipientSecret::from_seed(seed));
    }
    let recipient = HqRecipientSecret::generate();
    std::fs::create_dir_all(data_dir).map_err(|e| e.to_string())?;
    // Persist only the seed bytes; never log them.
    let seed = recipient.seed_bytes();
    std::fs::write(&path, seed).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(recipient)
}

fn parse_recipient_hex(hex: &str) -> Result<HqRecipientSecret, String> {
    let hex = hex.trim();
    if hex.len() != 64 {
        return Err("recipient secret must be 64 hex digits".into());
    }
    let mut seed = [0u8; 32];
    for i in 0..32 {
        seed[i] = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16)
            .map_err(|_| "recipient secret is not hex".to_owned())?;
    }
    Ok(HqRecipientSecret::from_seed(seed))
}

/// Running mesh: operator control + per-node drain consumers.
pub struct MeshRuntime {
    pub mesh: NodeOperatorMesh,
    tasks: Vec<JoinHandle<()>>,
    stop: watch::Sender<bool>,
}


async fn bootstrap_cursor(
    mesh: NodeOperatorMesh,
    link: OperatorLink,
    node_id: String,
    mut shutdown: watch::Receiver<bool>,
) {
    use hatchery_core::{NodeCursor, NodeId, NodeIncarnationId, NodeRequest, NodeRoute, RoutedNodeRequest};
    use crate::node_controller::G4aMessage;
    // Retry until Snapshot lands or shutdown.
    for attempt in 1u32..=120 {
        if *shutdown.borrow() {
            return;
        }
        let Ok(nid) = NodeId::new(node_id.clone()) else { return };
        let route = NodeRoute {
            node_id: nid,
            expected_incarnation_id: NodeIncarnationId::from_bytes([0; 16]),
        };
        let req = RoutedNodeRequest {
            route,
            request: NodeRequest::Snapshot,
        };
        match link.request(&req, std::time::Duration::from_secs(5)).await {
            Ok(G4aMessage::Response(routed)) => {
                if routed.response.is_ok() {
                    mesh.set_cursor(
                        &node_id,
                        NodeCursor {
                            incarnation_id: routed.incarnation_id,
                            sequence: 0,
                        },
                    );
                    tracing::info!(
                        %node_id,
                        incarnation = %routed.incarnation_id,
                        "core mesh cursor from Snapshot"
                    );
                    return;
                }
            }
            Ok(_) => {}
            Err(err) => {
                tracing::debug!(%node_id, %attempt, error = %err, "cursor bootstrap waiting for drain session");
            }
        }
        tokio::select! {
            _ = tokio::time::sleep(std::time::Duration::from_secs(2)) => {}
            _ = shutdown.changed() => return,
        }
    }
    tracing::warn!(%node_id, "core mesh cursor bootstrap gave up");
}

impl MeshRuntime {
    pub fn start(config: MeshConfig) -> Self {
        Self::start_with_m4a(config, None)
    }

    pub fn start_with_m4a(
        config: MeshConfig,
        m4a_handler: Option<crate::drain_consumer::CoreHandler>,
    ) -> Self {
        let mesh = NodeOperatorMesh::new();
        let (stop, shutdown) = watch::channel(false);
        let mut tasks = Vec::new();

        let listen = config.operator_listen.clone();
        let token = config.operator_token.clone();
        let mesh_serve = mesh.clone();
        let sd = shutdown.clone();
        tasks.push(tokio::spawn(async move {
            if let Err(err) = serve_local(listen, token, mesh_serve, sd).await {
                tracing::error!(%err, "core operator control stopped");
            }
        }));

        for node_id in &config.node_ids {
            let (events_tx, _events_rx) = mpsc::unbounded_channel();
            let (link, outbound) = OperatorLink::new(Some(events_tx));
            mesh.register(node_id, link.clone());
            let consumer = DrainConsumer::new(
                DrainClient::new(config.drain_addr, config.drain_token.clone()),
                config.recipient.clone(),
                NodeStream {
                    node_id: node_id.clone(),
                    channel_id: G4A_CONTROL_CHANNEL,
                    kind: ChannelKind::G4aControl,
                },
                link.clone().handler(),
            )
            .with_outbound(outbound);
            let sd = shutdown.clone();
            tasks.push(tokio::spawn(async move {
                consumer.run(sd).await;
            }));

            // Learn incarnation once the Noise session is up so HQ Spawn has a cursor.
            let mesh_boot = mesh.clone();
            let link_boot = link.clone();
            let boot_id = node_id.clone();
            let sd = shutdown.clone();
            tasks.push(tokio::spawn(async move {
                bootstrap_cursor(mesh_boot, link_boot, boot_id, sd).await;
            }));

            if let Some(handler) = m4a_handler.clone() {
                let consumer = DrainConsumer::new(
                    DrainClient::new(config.drain_addr, config.drain_token.clone()),
                    config.recipient.clone(),
                    NodeStream {
                        node_id: node_id.clone(),
                        channel_id: M4A_CHANNEL,
                        kind: ChannelKind::M4a,
                    },
                    handler,
                );
                let sd = shutdown.clone();
                tasks.push(tokio::spawn(async move {
                    consumer.run(sd).await;
                }));
            }
        }

        tracing::info!(
            nodes = config.node_ids.len(),
            listen = %config.operator_listen,
            "core operator mesh started (drain consumers registered)"
        );
        Self { mesh, tasks, stop }
    }

    pub fn shutdown(&self) {
        let _ = self.stop.send(true);
        for t in &self.tasks {
            t.abort();
        }
    }
}

/// Public path of the persisted recipient seed (for operators to hand the pub key to C2).
pub fn recipient_seed_path(data_dir: &Path) -> PathBuf {
    data_dir.join("core-recipient.seed")
}
