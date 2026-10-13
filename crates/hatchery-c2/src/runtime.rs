use crate::protocol::{
    C2ErrorCategory, GapKind, HealthResponse, NodeCursor, NodeFreshness, NodeGap, NodeId,
    NodeIncarnationId, NodeTransportState, ObservedNode, ProviderAdapterContractSupport,
    ProviderContractSupport, ReadyResponse, SanitizedError, SlimNodeInventory, StatusResponse,
    C2_API_VERSION, C2_PROVIDER_CONTRACT_MANIFEST_CAPABILITY, MAX_C2_GAPS_PER_NODE, MAX_C2_NODES,
};
#[cfg(windows)]
use crate::protocol::MAX_C2_ENDPOINT_BYTES;
use gate4agent_node_protocol::{NegotiatedNodeCompatibility, NodeEvent, NodeSnapshot};
use gate4agent_node_wire::read_call_home_announce;
use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
#[cfg(unix)]
use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use thiserror::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, watch, Semaphore};
use tokio::task::{JoinHandle, JoinSet};
use tokio::time::{sleep, timeout};

const HEADER_LIMIT_BYTES: usize = 16 * 1024;
const OPERATOR_BODY_LIMIT_BYTES: usize = 16 * 1024;
const MAX_HTTP_CONNECTIONS: usize = 16;
const RESPONSE_BODY_LIMIT_BYTES: usize = 8 * 1024 * 1024;


#[cfg(windows)]
pub const DEFAULT_C2_CONTROL_ENDPOINT: &str = r"\\.\pipe\gate4agent-c2";
#[cfg(unix)]
pub const DEFAULT_C2_CONTROL_ENDPOINT: &str = "gate4agent-c2.sock";

#[cfg(windows)]
pub fn default_c2_control_endpoint() -> Result<String, C2ConfigError> {
    Ok(DEFAULT_C2_CONTROL_ENDPOINT.to_owned())
}

#[cfg(unix)]
pub fn default_c2_control_endpoint() -> Result<String, C2ConfigError> {
    let root = unix_runtime_root()?;
    let directory = root.join("gate4agent");
    let endpoint = directory.join(DEFAULT_C2_CONTROL_ENDPOINT);
    validate_unix_endpoint(&endpoint).map_err(|_| C2ConfigError::InvalidControlEndpoint)?;
    Ok(endpoint.to_string_lossy().into_owned())
}

#[cfg(unix)]
fn unix_runtime_root() -> Result<PathBuf, C2ConfigError> {
    if let Some(root) = std::env::var_os("XDG_RUNTIME_DIR").filter(|value| !value.is_empty()) {
        let root = PathBuf::from(root);
        if root.is_absolute() { return Ok(root); }
    }
    let home = std::env::var_os("HOME").filter(|value| !value.is_empty())
        .ok_or_else(|| C2ConfigError::RuntimeEndpoint(
            "neither absolute XDG_RUNTIME_DIR nor HOME is available".to_owned(),
        ))?;
    let home = PathBuf::from(home);
    if !home.is_absolute() {
        return Err(C2ConfigError::RuntimeEndpoint("HOME is not absolute".to_owned()));
    }
    Ok(home.join(".gate4agent").join("run"))
}

#[derive(Clone)]
pub struct C2NodeConfig {
    pub node_id: NodeId,
    pub endpoint: String,
    route: C2NodeRoute,
    token: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum C2NodeRoute {
    Local,
    SshForwardedLoopback(SocketAddr),
    /// This relay does not dial the node; the node dials the relay.
    ///
    /// For a node the relay has no way to reach -- behind NAT, in a
    /// container with no published port, on a machine with no listener the
    /// relay is allowed to open. The protocol roles are unchanged: the
    /// node is still the server, this relay is still the client asking it
    /// for things. Only the direction of the TCP connection moves, which
    /// is why the whole of this variant's implementation is "wait for a
    /// socket instead of opening one" and then the identical handshake.
    CallHome,
}

impl C2NodeConfig {
    pub fn new(node_id: NodeId, endpoint: impl Into<String>, token: impl Into<String>) -> Result<Self, C2ConfigError> {
        let endpoint = endpoint.into();
        let token = token.into();
        let (endpoint, route) = parse_node_endpoint(&endpoint)
            .ok_or_else(|| C2ConfigError::InvalidEndpoint(node_id.clone()))?;
        validate_token(&token)?;
        Ok(Self { node_id, endpoint, route, token })
    }

    fn transport_label(&self) -> &'static str {
        match self.route {
            C2NodeRoute::Local => local_transport_label(),
            C2NodeRoute::SshForwardedLoopback(_) => "ssh-forwarded-loopback",
            C2NodeRoute::CallHome => "call-home",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct C2Timings {
    pub poll_interval: Duration,
    pub fresh_for: Duration,
    pub attempt_deadline: Duration,
    pub transient_backoffs: [Duration; 5],
    pub parked_backoff: Duration,
    pub http_io_deadline: Duration,
}

impl Default for C2Timings {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_millis(250),
            fresh_for: Duration::from_secs(10),
            attempt_deadline: Duration::from_secs(5),
            transient_backoffs: [
                Duration::from_millis(500), Duration::from_secs(1), Duration::from_secs(2),
                Duration::from_secs(4), Duration::from_secs(8),
            ],
            parked_backoff: Duration::from_secs(30),
            http_io_deadline: Duration::from_secs(3),
        }
    }
}

#[derive(Clone)]
pub struct C2Config {
    pub api_listen: SocketAddr,
    /// Deprecated (A2): kept for config compatibility. C2 no longer opens an
    /// operator control socket; core drains and parses g4a.
    pub control_endpoint: String,
    /// Deprecated (A2): ignored. Operator control is not accepted on C2.
    pub control_tunnel_listen: Option<SocketAddr>,
    api_token: String,
    pub nodes: Vec<C2NodeConfig>,
    /// Where nodes that cannot be dialled come to announce themselves.
    /// `None` unless at least one node is configured `accept`.
    pub node_listen: Option<SocketAddr>,
    pub timings: C2Timings,
    /// Node enrollment (one-time code, neutral status, runtime node add). `None` keeps the
    /// fixed `--node` set and token authentication only.
    pub enrollment: Option<crate::enroll_service::EnrollConfig>,
    /// A2 restreamer: node multiplex uplink listen. Requires `restreamer_drain_listen`.
    pub restreamer_node_listen: Option<SocketAddr>,
    /// A2 restreamer: core drain listen (C2 never dials core).
    pub restreamer_drain_listen: Option<SocketAddr>,
    /// Bearer core presents on the drain stream. Never logged.
    restreamer_core_token: Option<String>,
}

impl C2Config {
    /// Turn on enrollment. Requires a node-listen address (checked by `validate_call_home`).
    pub fn with_enrollment(mut self, enrollment: crate::enroll_service::EnrollConfig) -> Self {
        self.enrollment = Some(enrollment);
        self
    }

    pub fn new(api_listen: SocketAddr, api_token: impl Into<String>, nodes: Vec<C2NodeConfig>) -> Result<Self, C2ConfigError> {
        let api_token = api_token.into();
        if !api_listen.ip().is_loopback() { return Err(C2ConfigError::NonLoopback(api_listen)); }
        validate_token(&api_token)?;
        if nodes.len() > MAX_C2_NODES { return Err(C2ConfigError::NodeCount(nodes.len())); }
        let mut ids = BTreeSet::new();
        let mut endpoints = BTreeSet::new();
        if nodes.iter().any(|node| !ids.insert(node.node_id.clone())) { return Err(C2ConfigError::DuplicateNode); }
        // Call-home nodes are exempt: they name no endpoint, so every one
        // of them carries the same `accept` and the uniqueness this check
        // enforces is `node_id`'s job for them (already done above). The
        // check exists to stop two dialled nodes pointing at one socket,
        // which cannot happen to a node that is never dialled.
        if nodes
            .iter()
            .filter(|node| node.route != C2NodeRoute::CallHome)
            .any(|node| !endpoints.insert(endpoint_key(&node.endpoint)))
        {
            return Err(C2ConfigError::DuplicateEndpoint);
        }
        let control_endpoint = default_c2_control_endpoint()?;
        if nodes.iter().any(|node| endpoints_equal(&node.endpoint, &control_endpoint)) {
            return Err(C2ConfigError::ControlEndpointConflict);
        }
        Ok(Self {
            api_listen,
            control_endpoint,
            control_tunnel_listen: None,
            api_token,
            nodes,
            node_listen: None,
            timings: C2Timings::default(),
            enrollment: None,
            restreamer_node_listen: None,
            restreamer_drain_listen: None,
            restreamer_core_token: None,
        })
    }

    /// Enable the A2 byte restreamer (node uplink + core drain). C2 never dials core.
    pub fn with_restreamer(
        mut self,
        node_listen: SocketAddr,
        drain_listen: SocketAddr,
        core_token: impl Into<String>,
    ) -> Result<Self, C2ConfigError> {
        let core_token = core_token.into();
        if core_token.len() < 16 {
            return Err(C2ConfigError::RestreamerToken);
        }
        if node_listen == drain_listen {
            return Err(C2ConfigError::RestreamerListenConflict);
        }
        if node_listen.port() == 0 || node_listen.ip().is_unspecified() {
            return Err(C2ConfigError::NonLoopbackNodeListen(node_listen));
        }
        if drain_listen.port() == 0 || drain_listen.ip().is_unspecified() {
            return Err(C2ConfigError::InvalidControlTunnel(drain_listen));
        }
        self.restreamer_node_listen = Some(node_listen);
        self.restreamer_drain_listen = Some(drain_listen);
        self.restreamer_core_token = Some(core_token);
        Ok(self)
    }

    /// Production restreamer: both listeners on the C2 WireGuard tunnel IP.
    pub fn with_restreamer_wg(
        self,
        c2_tunnel_ip: std::net::IpAddr,
        node_port: u16,
        drain_port: u16,
        core_token: impl Into<String>,
    ) -> Result<Self, C2ConfigError> {
        let bind = crate::restreamer::RestreamerWgBind::production(c2_tunnel_ip, node_port, drain_port)
            .map_err(|_| C2ConfigError::RestreamerListenConflict)?;
        self.with_restreamer(bind.node_listen, bind.drain_listen, core_token)
    }

    /// Binds the address nodes call in on.
    ///
    /// Loopback, or one concrete C2 WireGuard tunnel address (same rule as
    /// [`Self::with_control_tunnel_listen`]). The node wire authenticates both
    /// ends but encrypts nothing, so `0.0.0.0` and other unspecified binds are
    /// refused: off-tunnel plaintext would expose terminal contents. When the
    /// listen address is the C2 tunnel IP, kernel WireGuard encrypts the path
    /// and either side may dial.
    pub fn with_node_listen(mut self, node_listen: SocketAddr) -> Result<Self, C2ConfigError> {
        // Concrete non-unspecified + nonzero port. Loopback stays valid; WG
        // tunnel addresses (e.g. 10.88.1.2:PORT) are valid for kit peers.
        if node_listen.port() == 0 || node_listen.ip().is_unspecified() {
            return Err(C2ConfigError::NonLoopbackNodeListen(node_listen));
        }
        if node_listen == self.api_listen {
            return Err(C2ConfigError::NodeListenConflict);
        }
        if self.control_tunnel_listen == Some(node_listen) {
            return Err(C2ConfigError::NonLoopbackNodeListen(node_listen));
        }
        self.node_listen = Some(node_listen);
        Ok(self)
    }

    /// Every node that waits to be called needs somewhere to call, so a
    /// config that asks for one without the other is refused rather than
    /// started into a relay that can never reach that node. Checked at
    /// startup, not at connect time, because the failure is total and
    /// permanent -- there is nothing to retry.
    pub fn validate_call_home(&self) -> Result<(), C2ConfigError> {
        if self.nodes.is_empty() && self.enrollment.is_none() {
            return Err(C2ConfigError::NodeCount(0));
        }
        let waiting = self.nodes.iter().find(|node| node.route == C2NodeRoute::CallHome);
        match (waiting, self.node_listen) {
            (Some(node), None) => Err(C2ConfigError::CallHomeWithoutListener(node.node_id.clone())),
            (None, None) if self.enrollment.is_some() => Err(C2ConfigError::EnrollmentWithoutListener),
            _ => Ok(()),
        }
    }

    pub fn with_timings(mut self, timings: C2Timings) -> Self {
        self.timings = timings;
        self
    }

    /// Deprecated (A2): records the path but C2 does not listen on it.
    pub fn with_control_endpoint(mut self, endpoint: impl Into<String>) -> Result<Self, C2ConfigError> {
        let endpoint = endpoint.into();
        validate_control_endpoint(&endpoint)?;
        if self.nodes.iter().any(|node| endpoints_equal(&node.endpoint, &endpoint)) {
            return Err(C2ConfigError::ControlEndpointConflict);
        }
        self.control_endpoint = endpoint;
        Ok(self)
    }

    /// TCP door for an HQ WireGuard client. Bound to one concrete address
    /// (the address on the C2 tunnel), never `0.0.0.0` or port 0. Does not
    /// replace the unix control listener.
    /// Deprecated (A2): records the address but C2 does not listen on it.
    pub fn with_control_tunnel_listen(mut self, listen: SocketAddr) -> Result<Self, C2ConfigError> {
        if listen.port() == 0 || listen.ip().is_unspecified() {
            return Err(C2ConfigError::InvalidControlTunnel(listen));
        }
        if listen == self.api_listen || self.node_listen == Some(listen) {
            return Err(C2ConfigError::InvalidControlTunnel(listen));
        }
        self.control_tunnel_listen = Some(listen);
        Ok(self)
    }
}

fn validate_control_endpoint(endpoint: &str) -> Result<(), C2ConfigError> {
    if !valid_local_endpoint(endpoint) {
        return Err(C2ConfigError::InvalidControlEndpoint);
    }
    Ok(())
}

fn parse_node_endpoint(endpoint: &str) -> Option<(String, C2NodeRoute)> {
    // The one assignment that names no address, because there is nothing
    // to address: this node will arrive on the call-home listener under
    // its own name. Spelled as a word rather than an empty value so a
    // config reader can tell "waits to be called" from "somebody forgot to
    // fill this in".
    if endpoint == "accept" {
        return Some((endpoint.to_owned(), C2NodeRoute::CallHome));
    }
    if let Some(authority) = endpoint.strip_prefix("tcp://") {
        let address = authority.parse::<SocketAddr>().ok()?;
        let is_exact_loopback = match address.ip() {
            std::net::IpAddr::V4(ip) => ip == std::net::Ipv4Addr::LOCALHOST,
            std::net::IpAddr::V6(ip) => ip == std::net::Ipv6Addr::LOCALHOST,
        };
        if !is_exact_loopback || address.port() == 0 {
            return None;
        }
        return Some((format!("tcp://{address}"), C2NodeRoute::SshForwardedLoopback(address)));
    }
    if endpoint.contains("://") || !valid_local_endpoint(endpoint) {
        return None;
    }
    Some((endpoint.to_owned(), C2NodeRoute::Local))
}

#[cfg(windows)]
fn valid_local_endpoint(endpoint: &str) -> bool {
    endpoint.starts_with(r"\\.\pipe\") && endpoint.len() > r"\\.\pipe\".len()
        && endpoint.len() <= MAX_C2_ENDPOINT_BYTES
}

#[cfg(unix)]
fn valid_local_endpoint(endpoint: &str) -> bool {
    validate_unix_endpoint(Path::new(endpoint)).is_ok()
}

#[cfg(unix)]
fn validate_unix_endpoint(endpoint: &Path) -> Result<(), ()> {
    const MAX_UNIX_ENDPOINT_BYTES: usize = 103;
    use std::os::unix::ffi::OsStrExt;

    if !endpoint.is_absolute() || endpoint.file_name().is_none()
        || endpoint.as_os_str().as_bytes().len() > MAX_UNIX_ENDPOINT_BYTES
    {
        return Err(());
    }
    Ok(())
}

#[cfg(windows)]
fn endpoint_key(endpoint: &str) -> String {
    if endpoint.starts_with("tcp://") { endpoint.to_owned() } else { endpoint.to_ascii_lowercase() }
}

#[cfg(unix)]
fn endpoint_key(endpoint: &str) -> String { endpoint.to_owned() }

fn endpoints_equal(left: &str, right: &str) -> bool { endpoint_key(left) == endpoint_key(right) }

fn validate_token(token: &str) -> Result<(), C2ConfigError> {
    if token.is_empty() || token.len() > 4096 || !token.bytes().all(|byte| matches!(byte, 0x21..=0x7e)) {
        return Err(C2ConfigError::InvalidToken);
    }
    Ok(())
}

#[derive(Debug, Error)]
pub enum C2ConfigError {
    #[error("C2 tokens must contain 1..=4096 visible ASCII bytes without whitespace")]
    InvalidToken,
    #[cfg_attr(windows, error("node '{0}' requires a bounded Windows named pipe or exact loopback TCP endpoint"))]
    #[cfg_attr(unix, error("node '{0}' requires a bounded local socket or exact loopback TCP endpoint"))]
    InvalidEndpoint(NodeId),
    #[error("C2 API listen address must be loopback: {0}")]
    NonLoopback(SocketAddr),
    #[error("C2 requires 1..=64 configured nodes; received {0}")]
    NodeCount(usize),
    #[error("C2 node IDs must be unique")]
    DuplicateNode,
    #[error("C2 node endpoints must be unique")]
    DuplicateEndpoint,
    #[cfg_attr(windows, error("C2 control endpoint must be a bounded local Windows named pipe"))]
    #[cfg_attr(unix, error("C2 control endpoint must be a bounded local endpoint"))]
    InvalidControlEndpoint,
    #[error("enrollment needs a --node-listen address for the nodes to call home on")]
    EnrollmentWithoutListener,
    #[error("restreamer core token must be at least 16 bytes")]
    RestreamerToken,
    #[error("restreamer node and drain listen addresses must differ")]
    RestreamerListenConflict,
    #[error("C2 control endpoint must not equal a configured node endpoint")]
    ControlEndpointConflict,
    #[error("C2 control tunnel listen must be a concrete non-unspecified address with a nonzero port, not shared with another listener: {0}")]
    InvalidControlTunnel(SocketAddr),
    #[error("C2 node call-home listen must be loopback or a concrete C2 tunnel address with a nonzero port (not 0.0.0.0, not shared with another listener): {0}")]
    NonLoopbackNodeListen(SocketAddr),
    #[error("C2 node call-home listen address must not equal the API listen address")]
    NodeListenConflict,
    #[error("node '{0}' waits to be called but no --node-listen address was configured")]
    CallHomeWithoutListener(NodeId),
    #[cfg(unix)]
    #[error("C2 default runtime endpoint is unavailable: {0}")]
    RuntimeEndpoint(String),
}

/// Presence endpoint for a node (A2): no g4a on C2 — only call-home liveness.
#[derive(Clone)]
struct RelayRegistry {
    map: Arc<std::sync::RwLock<Arc<BTreeMap<NodeId, RelayEndpoint>>>>,
    gate: Option<Arc<dyn crate::enroll_service::CapabilityGate>>,
}

impl RelayRegistry {
    fn new(initial: BTreeMap<NodeId, RelayEndpoint>) -> Self {
        Self { map: Arc::new(std::sync::RwLock::new(Arc::new(initial))), gate: None }
    }
    fn allows(&self, node_id: &NodeId, capability: &str) -> bool {
        self.gate.as_ref().and_then(|gate| gate.allows(node_id.as_str(), capability)).unwrap_or(true)
    }
    fn redact(&self, status: &mut StatusResponse) {
        for (id, node) in status.nodes.iter_mut() {
            if !self.allows(id, "attach") {
                node.inventory = None;
            }
        }
    }
    fn snapshot(&self) -> Arc<BTreeMap<NodeId, RelayEndpoint>> {
        Arc::clone(&self.map.read().expect("relay registry lock"))
    }
    fn insert(&self, node_id: NodeId, endpoint: RelayEndpoint) {
        let mut guard = self.map.write().expect("relay registry lock");
        let mut next = (**guard).clone();
        next.insert(node_id, endpoint);
        *guard = Arc::new(next);
    }
}

#[derive(Clone)]
struct RelayEndpoint {
    force_disconnect: watch::Sender<u64>,
}

#[derive(Debug, Error)]
pub enum C2Error {
    #[error("C2 API failed: {0}")]
    Api(#[from] io::Error),
    #[error("C2 task failed: {0}")]
    Task(#[from] tokio::task::JoinError),
}

#[derive(Clone)]
pub struct C2ShutdownHandle {
    shutdown: watch::Sender<bool>,
}

impl C2ShutdownHandle {
    pub fn shutdown(&self) { let _ = self.shutdown.send(true); }
}

pub struct C2Running {
    api_addr: SocketAddr,
    shutdown: C2ShutdownHandle,
    task: Option<JoinHandle<Result<(), C2Error>>>,
}


fn load_restreamer_keys_from_env() -> Result<Option<crate::restreamer::RestreamerKeys>, C2Error> {
    use crate::restreamer::{PublicKeyBytes, RestreamerKeys};
    use std::path::PathBuf;
    let seed_path = match std::env::var_os("HATCHERY_RESTREAMER_C2_SEED_FILE") {
        Some(p) => PathBuf::from(p),
        None => return Ok(None),
    };
    let recip_b64 = match std::env::var("HATCHERY_RESTREAMER_RECIPIENT_PUBLIC_B64") {
        Ok(v) if !v.trim().is_empty() => v,
        _ => return Ok(None),
    };
    let seed_bytes = std::fs::read(&seed_path).map_err(|e| {
        C2Error::Api(std::io::Error::other(format!(
            "HATCHERY_RESTREAMER_C2_SEED_FILE {}: {e}",
            seed_path.display()
        )))
    })?;
    if seed_bytes.len() != 32 {
        return Err(C2Error::Api(std::io::Error::other(
            "HATCHERY_RESTREAMER_C2_SEED_FILE must be exactly 32 bytes",
        )));
    }
    let mut seed = [0u8; 32];
    seed.copy_from_slice(&seed_bytes);
    let recip = decode_b64_32(&recip_b64).map_err(|e| {
        C2Error::Api(std::io::Error::other(format!(
            "HATCHERY_RESTREAMER_RECIPIENT_PUBLIC_B64: {e}"
        )))
    })?;
    Ok(Some(RestreamerKeys::from_seeds(seed, recip)))
}

fn decode_b64_32(text: &str) -> Result<[u8; 32], String> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(text.trim())
        .or_else(|_| base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(text.trim()))
        .map_err(|e| e.to_string())?;
    if bytes.len() != 32 {
        return Err(format!("expected 32 bytes, got {}", bytes.len()));
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes);
    Ok(out)
}

impl C2Running {
    pub async fn start(config: C2Config) -> Result<Self, C2Error> {
        prepare_default_control_parent(&config.control_endpoint)?;
        let listener = TcpListener::bind(config.api_listen).await?;
        let api_addr = listener.local_addr()?;
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let shutdown = C2ShutdownHandle { shutdown: shutdown_tx };
        let task = tokio::spawn(run_bound(config, listener, shutdown_rx));
        Ok(Self { api_addr, shutdown, task: Some(task) })
    }

    pub fn api_addr(&self) -> SocketAddr { self.api_addr }
    pub fn shutdown_handle(&self) -> C2ShutdownHandle { self.shutdown.clone() }
    pub async fn wait(mut self) -> Result<(), C2Error> {
        self.task.take().expect("C2 task is present").await?
    }
}

#[cfg(windows)]
fn prepare_default_control_parent(_endpoint: &str) -> io::Result<()> { Ok(()) }

#[cfg(unix)]
fn prepare_default_control_parent(endpoint: &str) -> io::Result<()> {
    let default = default_c2_control_endpoint()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error.to_string()))?;
    if endpoint != default { return Ok(()); }
    let parent = Path::new(endpoint).parent().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "default C2 endpoint has no parent")
    })?;
    std::fs::create_dir_all(parent)?;
    std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))
}

impl Drop for C2Running {
    fn drop(&mut self) {
        self.shutdown.shutdown();
        if let Some(task) = self.task.take() { task.abort(); }
    }
}

/// A socket handed from the call-home listener to a node's relay worker, with the
/// per-connection session token when the node authenticated by signature.
type CallHomeSocket = (TcpStream, Option<String>);
type CallHomeRoutes = Arc<Mutex<BTreeMap<NodeId, mpsc::Sender<CallHomeSocket>>>>;

struct Spawner {
    timings: C2Timings,
    relay_senders: RelayRegistry,
    call_home_routes: CallHomeRoutes,
    ingress: mpsc::Sender<Attempt>,
}

impl Spawner {
    /// Presence worker for a runtime-enrolled node (call-home liveness only; no g4a).
    fn spawn_enrolled(&self, node_id: &NodeId, shutdown: &watch::Receiver<bool>, tasks: &mut JoinSet<io::Result<()>>) {
        if self.relay_senders.snapshot().contains_key(node_id) {
            return;
        }
        let placeholder = "0".repeat(64);
        let Ok(node) = C2NodeConfig::new(node_id.clone(), "accept", placeholder) else { return };
        let (force_tx, force_rx) = watch::channel(0_u64);
        let (call_home_tx, call_home_rx) = mpsc::channel(1);
        self.call_home_routes.lock().expect("routes lock").insert(node_id.clone(), call_home_tx);
        self.relay_senders.insert(node_id.clone(), RelayEndpoint { force_disconnect: force_tx });
        tasks.spawn(node_presence_worker(
            node,
            self.timings,
            force_rx,
            Some(call_home_rx),
            self.ingress.clone(),
            shutdown.clone(),
        ));
        tracing::info!(%node_id, "presence worker created for enrolled node");
    }
}

async fn run_bound(config: C2Config, listener: TcpListener, mut shutdown: watch::Receiver<bool>) -> Result<(), C2Error> {
    let now = unix_ms();
    let nodes = config.nodes.iter().map(|node| {
        (node.node_id.clone(), initial_observed_node(node))
    }).collect();
    let initial = Arc::new(StatusResponse { api_version: C2_API_VERSION, ready: config.nodes.is_empty(), observed_at_unix_ms: now, nodes });
    let (status_tx, status_rx) = watch::channel(initial);
    let (ingress_tx, ingress_rx) = mpsc::channel(config.nodes.len().saturating_mul(2).max(16));
    let mut tasks = JoinSet::new();
    let mut relay_senders = BTreeMap::new();
    let mut presence_receivers = Vec::new();
    let mut static_routes: BTreeMap<NodeId, mpsc::Sender<CallHomeSocket>> = BTreeMap::new();
    for node in config.nodes.clone() {
        let (force_tx, force_rx) = watch::channel(0_u64);
        relay_senders.insert(node.node_id.clone(), RelayEndpoint { force_disconnect: force_tx });
        let call_home_rx = if node.route == C2NodeRoute::CallHome {
            let (call_home_tx, call_home_rx) = mpsc::channel(1);
            static_routes.insert(node.node_id.clone(), call_home_tx);
            Some(call_home_rx)
        } else {
            None
        };
        presence_receivers.push((node, force_rx, call_home_rx));
    }
    let static_ids: BTreeSet<NodeId> = static_routes.keys().cloned().collect();
    let call_home_routes: CallHomeRoutes = Arc::new(Mutex::new(static_routes));
    let mut relay_senders = RelayRegistry::new(relay_senders);
    // Enrollment: registry, listener, runtime node adds.
    let mut enroll_events = None;
    let mut enroll_service = None;
    if let Some(enrollment) = &config.enrollment {
        let (service, events) = crate::enroll_service::EnrollService::start(enrollment)
            .map_err(|error| C2Error::Api(io::Error::other(error.to_string())))?;
        let enroll_listener = TcpListener::bind(enrollment.listen).await?;
        tasks.spawn(crate::enroll_service::serve_enroll(enroll_listener, Arc::clone(&service), shutdown.clone()));
        relay_senders.gate = Some(Arc::clone(&service) as Arc<dyn crate::enroll_service::CapabilityGate>);
        enroll_events = Some(events);
        enroll_service = Some(service);
    }
    if let Some(node_listen) = config.node_listen {
        tasks.spawn(accept_call_home(node_listen, Arc::clone(&call_home_routes), static_ids, enroll_service.clone(), shutdown.clone()));
    }
    // A2 restreamer: opaque node uplink + core drain (C2 never dials core).
    if let (Some(node_listen), Some(drain_listen), Some(core_token)) = (
        config.restreamer_node_listen,
        config.restreamer_drain_listen,
        config.restreamer_core_token.clone(),
    ) {
        let journal = Arc::new(crate::restreamer::MemoryJournal::new());
        let keys = load_restreamer_keys_from_env()?;
        if keys.is_some() {
            tracing::info!("A2 restreamer nested keys loaded");
        } else {
            tracing::warn!("A2 restreamer nested keys missing; uplink DATA cannot be unwrapped (set HATCHERY_RESTREAMER_C2_SEED_FILE + HATCHERY_RESTREAMER_RECIPIENT_PUBLIC_B64)");
        }
        let restreamer = crate::restreamer::Restreamer::new(
            crate::restreamer::BufferLimits::default(),
            journal,
            core_token.clone(),
            keys,
        );
        let cfg = crate::restreamer::RestreamerConfig {
            node_listen,
            drain_listen,
            buffer: crate::restreamer::BufferLimits::default(),
            core_token,
        };
        let rs = restreamer.clone();
        let sd = shutdown.clone();
        tasks.spawn(async move {
            crate::restreamer::serve(cfg, rs, sd)
                .await
                .map_err(|error| io::Error::other(error.to_string()))
        });
        tracing::info!(%node_listen, %drain_listen, "A2 restreamer listening");
    }
    tasks.spawn(inventory_owner(config.timings.fresh_for, ingress_rx, status_tx, shutdown.clone()));
    tracing::info!("A2: g4a node relays removed; call-home presence only");
    for (node, force_disconnect, call_home) in presence_receivers {
        tasks.spawn(node_presence_worker(node, config.timings, force_disconnect, call_home, ingress_tx.clone(), shutdown.clone()));
    }
    let spawner = Spawner {
        timings: config.timings,
        relay_senders: relay_senders.clone(),
        call_home_routes: Arc::clone(&call_home_routes),
        ingress: ingress_tx.clone(),
    };
    drop(ingress_tx);
    tasks.spawn(http_server(listener, config.api_token.clone(), config.timings.http_io_deadline, status_rx.clone(), enroll_service.clone(), shutdown.clone()));
    // A2: no operator control listener on C2. Core opens the drain and parses g4a.
    let _ = (config.control_endpoint, config.control_tunnel_listen, status_rx);
    loop {
        tokio::select! {
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() { break; }
            }
            event = async {
                match enroll_events.as_mut() {
                    Some(events) => events.recv().await,
                    None => std::future::pending().await,
                }
            } => {
                match event {
                    Some(crate::enroll_service::EnrollEvent::Enrolled(id)) => {
                        if let Ok(node_id) = NodeId::new(id) { spawner.spawn_enrolled(&node_id, &shutdown, &mut tasks); }
                    }
                    Some(crate::enroll_service::EnrollEvent::StatusChanged(id, _)) => {
                        // A node that no longer holds the heartbeat capability is dropped.
                        if let Ok(node_id) = NodeId::new(id) {
                            if !relay_senders.allows(&node_id, "heartbeat") {
                                if let Some(endpoint) = relay_senders.snapshot().get(&node_id) {
                                    endpoint.force_disconnect.send_modify(|generation| *generation = generation.wrapping_add(1));
                                }
                            }
                        }
                    }
                    Some(crate::enroll_service::EnrollEvent::PolicyChanged) => {
                        if let Some(service) = &enroll_service {
                            for id in service.nodes_without("heartbeat") {
                                if let Ok(node_id) = NodeId::new(id) {
                                    if let Some(endpoint) = relay_senders.snapshot().get(&node_id) {
                                        endpoint.force_disconnect.send_modify(|generation| *generation = generation.wrapping_add(1));
                                    }
                                }
                            }
                        }
                    }
                    None => enroll_events = None,
                }
            }
            result = tasks.join_next() => {
                match result {
                    Some(Ok(Ok(()))) if *shutdown.borrow() => break,
                    Some(Ok(Ok(()))) => return Err(C2Error::Api(io::Error::new(io::ErrorKind::Other, "C2 task exited unexpectedly"))),
                    Some(Ok(Err(error))) => return Err(C2Error::Api(error)),
                    Some(Err(error)) => return Err(C2Error::Task(error)),
                    None => break,
                }
            }
        }
    }
    tasks.shutdown().await;
    Ok(())
}

fn initial_enrolled_node() -> ObservedNode {
    ObservedNode {
        endpoint: "call-home".to_owned(), transport_label: "call-home".to_owned(),
        transport: NodeTransportState::Offline, freshness: NodeFreshness::Unavailable,
        cursor: None, inventory: None, machine: None, last_attempt_unix_ms: None, last_success_unix_ms: None,
        consecutive_failures: 0, last_error: None, gaps: Vec::new(), gaps_truncated: 0,
    }
}

fn initial_observed_node(node: &C2NodeConfig) -> ObservedNode {
    ObservedNode {
        endpoint: node.endpoint.clone(), transport_label: node.transport_label().to_owned(),
        transport: NodeTransportState::Offline, freshness: NodeFreshness::Unavailable,
        cursor: None, inventory: None, machine: None, last_attempt_unix_ms: None, last_success_unix_ms: None,
        consecutive_failures: 0, last_error: None, gaps: Vec::new(), gaps_truncated: 0,
    }
}

#[cfg(windows)]
fn local_transport_label() -> &'static str { "windows-named-pipe" }

#[cfg(unix)]
fn local_transport_label() -> &'static str { "unix-domain-socket" }

#[derive(Clone, Default)]
struct ProviderContractManifest {
    provider_contracts: Vec<ProviderContractSupport>,
    provider_adapter_contracts: Vec<ProviderAdapterContractSupport>,
}

impl ProviderContractManifest {
    fn from_compatibility(compatibility: Option<&NegotiatedNodeCompatibility>) -> Self {
        let Some(compatibility) = compatibility.filter(|compatibility| {
            compatibility.capabilities.iter().any(|capability| {
                capability.as_str() == C2_PROVIDER_CONTRACT_MANIFEST_CAPABILITY
            })
        }) else {
            return Self::default();
        };
        Self {
            provider_contracts: compatibility.provider_contracts.clone(),
            provider_adapter_contracts: compatibility.provider_adapter_contracts.clone(),
        }
    }
}

enum AttemptResult {
    Connected {
        cursor: NodeCursor,
        snapshot: NodeSnapshot,
        gaps: Vec<GapKind>,
        provider_contract_manifest: ProviderContractManifest,
    },
    Success { cursor: NodeCursor, snapshot: NodeSnapshot, gaps: Vec<GapKind> },
    Cursor {
        cursor: NodeCursor,
        gaps: Vec<GapKind>,
        managed_worktree_events: Vec<NodeEvent>,
    },
    Failure { error: SanitizedError, hard: bool },
}

struct Attempt { node_id: NodeId, at_unix_ms: u64, result: AttemptResult }

/// Accepts node connections and hands each one to the relay worker for
/// whichever node it says it is.
///
/// This is the only place in the relay that learns a node's identity from
/// the wire rather than from configuration, and it is careful about what
/// that means: the announced name SELECTS a worker and nothing more. The
/// worker then runs the same mutual challenge-response it runs on a
/// dialled connection, against the token configured for that node, so a
/// caller that announced a name it cannot prove gets no further than the
/// next frame.
///
/// A socket for an unknown node, an unreadable preface, or a node whose
/// slot is already full is dropped with a line saying which -- never
/// silently, because "the node is calling but nothing happens" is exactly
/// the failure that is impossible to diagnose from the node's end.
async fn accept_call_home(
    listen: SocketAddr,
    routes: CallHomeRoutes,
    static_ids: BTreeSet<NodeId>,
    enroll: Option<Arc<crate::enroll_service::EnrollService>>,
    mut shutdown: watch::Receiver<bool>,
) -> io::Result<()> {
    let listener = TcpListener::bind(listen).await?;
    tracing::info!(%listen, waiting_nodes = routes.lock().expect("routes lock").len(), "call-home listener bound");
    loop {
        if *shutdown.borrow() {
            return Ok(());
        }
        let (stream, peer) = tokio::select! {
            accepted = listener.accept() => accepted?,
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() { return Ok(()); }
                continue;
            }
        };
        let routes = Arc::clone(&routes);
        let static_ids = static_ids.clone();
        let enroll = enroll.clone();
        // Off the accept loop: the preface has its own deadline, and one
        // peer that connects and then says nothing must not stop every
        // other node from being accepted meanwhile.
        tokio::spawn(async move {
            let mut stream = stream;
            let _ = stream.set_nodelay(true);
            let node_id = match read_call_home_announce(&mut stream).await {
                Ok(node_id) => node_id,
                Err(error) => {
                    tracing::warn!(%peer, cause = %error, "call-home connection rejected");
                    return;
                }
            };
            // Token nodes (configured with --node) keep the original path. Enrolled nodes
            // prove their identity by signing a challenge; the exchange yields a
            // per-connection session token for the unchanged wire handshake.
            let mut session_token = None;
            if !static_ids.contains(&node_id) {
                let Some(enroll) = enroll.as_ref() else {
                    tracing::warn!(%peer, %node_id, "call-home connection named a node this relay does not wait for");
                    return;
                };
                match enroll.signed_call_home(&mut stream, node_id.as_str()).await {
                    Ok(token) => session_token = Some(token),
                    Err(cause) => {
                        tracing::warn!(%peer, %node_id, %cause, "signed call-home refused");
                        return;
                    }
                }
            }
            let sender = routes.lock().expect("routes lock").get(&node_id).cloned();
            let Some(sender) = sender else {
                tracing::warn!(
                    %peer,
                    %node_id,
                    "call-home connection named a node this relay does not wait for",
                );
                return;
            };
            match sender.try_send((stream, session_token)) {
                Ok(()) => tracing::info!(%peer, %node_id, "node called home"),
                Err(mpsc::error::TrySendError::Full(_)) => tracing::warn!(
                    %peer,
                    %node_id,
                    "node called home while its previous connection is still being taken up",
                ),
                Err(mpsc::error::TrySendError::Closed(_)) => tracing::warn!(
                    %peer,
                    %node_id,
                    "node called home but its relay worker is gone",
                ),
            }
        });
    }
}

/// Blocks until this node calls in, or shutdown -- with no deadline of its
/// own on purpose.
///
/// A dialled connection either answers within `attempt_deadline` or has
/// failed, and treating a slow answer as a failure is right there. Waiting
/// for a node to call is the opposite: a node that has not called yet has
/// not failed at anything, and it may be minutes from starting. Putting
/// that wait under the attempt deadline would turn an idle relay into a
/// stream of "connection deadline exceeded" warnings about nodes that are
/// merely not running. The handshake that FOLLOWS is deadlined normally --
/// a peer that connects and then stalls is a real failure.
async fn await_call_home(
    receiver: Option<&mut mpsc::Receiver<CallHomeSocket>>,
    shutdown: &mut watch::Receiver<bool>,
) -> Option<CallHomeSocket> {
    let receiver = receiver?;
    loop {
        if *shutdown.borrow() {
            return None;
        }
        tokio::select! {
            stream = receiver.recv() => return stream,
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() { return None; }
            }
        }
    }
}

async fn node_presence_worker(
    node: C2NodeConfig,
    timings: C2Timings,
    mut force_disconnect: watch::Receiver<u64>,
    mut call_home: Option<mpsc::Receiver<CallHomeSocket>>,
    ingress: mpsc::Sender<Attempt>,
    mut shutdown: watch::Receiver<bool>,
) -> io::Result<()> {
    // A2: C2 never speaks g4a. Call-home only proves the node can reach edge;
    // spawn/inventory go through the opaque restreamer to core.
    let _ = timings;
    loop {
        if *shutdown.borrow() { return Ok(()); }
        let adopted = if node.route == C2NodeRoute::CallHome {
            match await_call_home(call_home.as_mut(), &mut shutdown).await {
                Some((stream, _token)) => stream,
                None => return Ok(()),
            }
        } else {
            // Dialled/local routes are unused under A2 opaque restreamer.
            tokio::select! {
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() { return Ok(()); }
                }
                _ = force_disconnect.changed() => {}
                _ = tokio::time::sleep(Duration::from_secs(60)) => {}
            }
            continue;
        };
        let incarnation_id = gate4agent_node_wire::random_incarnation_id()
            .unwrap_or_else(|_| NodeIncarnationId::from_bytes([0u8; 16]));
        let cursor = NodeCursor { incarnation_id, sequence: 0 };
        let snapshot = NodeSnapshot {
            node_id: node.node_id.clone(),
            enabled_providers: Vec::new(),
            provider_runtime_statuses: Default::default(),
            workspaces: Vec::new(),
            session_records: Vec::new(),
            managed_worktrees: Vec::new(),
            launch_inventory: None,
            agent_progress: Vec::new(),
        };
        ingress_attempt(&ingress, &node.node_id, AttemptResult::Connected {
            cursor,
            snapshot,
            gaps: Vec::new(),
            provider_contract_manifest: ProviderContractManifest::default(),
        }).await?;
        tracing::info!(node_id = %node.node_id, "node present on call-home (no g4a on C2)");
        let mut stream = adopted;
        let mut buf = [0u8; 256];
        let drop_reason = loop {
            tokio::select! {
                changed = force_disconnect.changed() => {
                    let _ = changed;
                    break "forced disconnect";
                }
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() {
                        return Ok(());
                    }
                }
                read = tokio::io::AsyncReadExt::read(&mut stream, &mut buf) => {
                    match read {
                        Ok(0) => break "call-home closed",
                        Ok(_) => {}
                        Err(_) => break "call-home read error",
                    }
                }
            }
        };
        tracing::info!(node_id = %node.node_id, reason = drop_reason, "node presence ended");
        ingress_attempt(&ingress, &node.node_id, AttemptResult::Failure {
            error: SanitizedError { category: C2ErrorCategory::Transport, message: drop_reason.to_owned() },
            hard: false,
        }).await?;
    }
}

async fn ingress_attempt(
    ingress: &mpsc::Sender<Attempt>,
    node_id: &NodeId,
    result: AttemptResult,
) -> io::Result<()> {
    ingress.send(Attempt { node_id: node_id.clone(), at_unix_ms: unix_ms(), result }).await
        .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "inventory owner closed"))
}

async fn inventory_owner(
    fresh_for: Duration, mut ingress: mpsc::Receiver<Attempt>,
    status: watch::Sender<Arc<StatusResponse>>, mut shutdown: watch::Receiver<bool>,
) -> io::Result<()> {
    let mut current = (**status.borrow()).clone();
    let mut attempted = BTreeSet::new();
    loop {
        tokio::select! {
            attempt = ingress.recv() => {
                let Some(attempt) = attempt else { return Ok(()); };
                attempted.insert(attempt.node_id.clone());
                let node = current.nodes.entry(attempt.node_id.clone()).or_insert_with(initial_enrolled_node);
                node.last_attempt_unix_ms = Some(attempt.at_unix_ms);
                match attempt.result {
                    AttemptResult::Connected {
                        cursor,
                        snapshot,
                        gaps,
                        provider_contract_manifest,
                    } => {
                        let previous = node.cursor;
                        let mut inventory = SlimNodeInventory::from_snapshot(&snapshot);
                        inventory.provider_contracts =
                            provider_contract_manifest.provider_contracts;
                        inventory.provider_adapter_contracts =
                            provider_contract_manifest.provider_adapter_contracts;
                        node.transport = NodeTransportState::Online;
                        node.freshness = NodeFreshness::Fresh;
                        node.cursor = Some(cursor);
                        node.inventory = Some(inventory);
                        node.last_success_unix_ms = Some(attempt.at_unix_ms);
                        node.consecutive_failures = 0;
                        node.last_error = None;
                        for kind in gaps {
                            if node.gaps.len() == MAX_C2_GAPS_PER_NODE { node.gaps.remove(0); node.gaps_truncated += 1; }
                            node.gaps.push(NodeGap { kind, detected_at_unix_ms: attempt.at_unix_ms, previous, observed: cursor });
                        }
                    }
                    AttemptResult::Success { cursor, snapshot, gaps } => {
                        let previous = node.cursor;
                        let provider_contract_manifest = node.inventory.as_ref().map(|inventory| {
                            ProviderContractManifest {
                                provider_contracts: inventory.provider_contracts.clone(),
                                provider_adapter_contracts: inventory.provider_adapter_contracts.clone(),
                            }
                        }).unwrap_or_default();
                        let mut inventory = SlimNodeInventory::from_snapshot(&snapshot);
                        inventory.provider_contracts =
                            provider_contract_manifest.provider_contracts;
                        inventory.provider_adapter_contracts =
                            provider_contract_manifest.provider_adapter_contracts;
                        node.transport = NodeTransportState::Online;
                        node.freshness = NodeFreshness::Fresh;
                        node.cursor = Some(cursor);
                        node.inventory = Some(inventory);
                        node.last_success_unix_ms = Some(attempt.at_unix_ms);
                        node.consecutive_failures = 0;
                        node.last_error = None;
                        for kind in gaps {
                            if node.gaps.len() == MAX_C2_GAPS_PER_NODE { node.gaps.remove(0); node.gaps_truncated += 1; }
                            node.gaps.push(NodeGap { kind, detected_at_unix_ms: attempt.at_unix_ms, previous, observed: cursor });
                        }
                    }
                    AttemptResult::Cursor {
                        cursor,
                        gaps,
                        managed_worktree_events,
                    } => {
                        let previous = node.cursor;
                        let incarnation_changed = previous.is_some_and(|previous| {
                            previous.incarnation_id != cursor.incarnation_id
                        });
                        apply_managed_worktree_cursor(
                            node.inventory.as_mut(),
                            incarnation_changed,
                            &managed_worktree_events,
                        );
                        node.transport = NodeTransportState::Online;
                        node.freshness = NodeFreshness::Fresh;
                        node.cursor = Some(cursor);
                        node.last_success_unix_ms = Some(attempt.at_unix_ms);
                        node.consecutive_failures = 0;
                        node.last_error = None;
                        for kind in gaps {
                            if node.gaps.len() == MAX_C2_GAPS_PER_NODE { node.gaps.remove(0); node.gaps_truncated += 1; }
                            node.gaps.push(NodeGap { kind, detected_at_unix_ms: attempt.at_unix_ms, previous, observed: cursor });
                        }
                    }
                    AttemptResult::Failure { error, hard } => {
                        node.consecutive_failures = node.consecutive_failures.saturating_add(1);
                        node.transport = if hard || node.consecutive_failures >= 5 { NodeTransportState::Parked } else { NodeTransportState::Offline };
                        node.last_error = Some(error);
                    }
                }
                current.ready = current.nodes.keys().all(|id| attempted.contains(id));
                refresh_freshness(&mut current, fresh_for);
                current.observed_at_unix_ms = unix_ms();
                status.send_replace(Arc::new(current.clone()));
            }
            _ = sleep(Duration::from_millis(250)) => {
                refresh_freshness(&mut current, fresh_for);
                current.observed_at_unix_ms = unix_ms();
                status.send_replace(Arc::new(current.clone()));
            }
            changed = shutdown.changed() => if changed.is_err() || *shutdown.borrow() { return Ok(()); },
        }
    }
}

fn apply_managed_worktree_cursor(
    inventory: Option<&mut SlimNodeInventory>,
    incarnation_changed: bool,
    events: &[NodeEvent],
) {
    let Some(inventory) = inventory else { return; };
    if incarnation_changed {
        inventory.provider_runtime_statuses.clear();
        inventory.managed_worktrees.clear();
        inventory.managed_worktree_count = 0;
        inventory.managed_worktrees_truncated = false;
        return;
    }
    for event in events {
        match event {
            NodeEvent::ManagedWorktreeUpserted { lease } => {
                inventory.managed_worktrees.retain(|existing| {
                    existing.lease_id != lease.lease_id
                        && existing.workspace_id != lease.workspace_id
                });
                inventory.managed_worktrees.push(lease.clone());
                inventory.managed_worktrees.sort_by(|left, right| {
                    left.lease_id.cmp(&right.lease_id)
                });
                inventory.managed_worktree_count = inventory.managed_worktrees.len();
                inventory.managed_worktrees.truncate(
                    crate::protocol::MAX_C2_MANAGED_WORKTREES_PER_NODE,
                );
                inventory.managed_worktrees_truncated =
                    inventory.managed_worktrees.len() < inventory.managed_worktree_count;
            }
            NodeEvent::ManagedWorktreeRemoved { lease_id } => {
                let before = inventory.managed_worktrees.len();
                inventory
                    .managed_worktrees
                    .retain(|lease| &lease.lease_id != lease_id);
                if inventory.managed_worktrees.len() < before
                    || inventory.managed_worktrees_truncated
                {
                    inventory.managed_worktree_count =
                        inventory.managed_worktree_count.saturating_sub(1);
                }
                inventory.managed_worktrees_truncated =
                    inventory.managed_worktrees.len() < inventory.managed_worktree_count;
            }
            _ => {}
        }
    }
}

fn refresh_freshness(status: &mut StatusResponse, fresh_for: Duration) {
    let now = unix_ms();
    let fresh_ms = fresh_for.as_millis().min(u64::MAX as u128) as u64;
    for node in status.nodes.values_mut() {
        node.freshness = match node.last_success_unix_ms {
            None => NodeFreshness::Unavailable,
            Some(last) if now.saturating_sub(last) <= fresh_ms => NodeFreshness::Fresh,
            Some(_) => NodeFreshness::Stale,
        };
    }
}

async fn http_server(
    listener: TcpListener, token: String, io_deadline: Duration,
    status: watch::Receiver<Arc<StatusResponse>>,
    enroll: Option<Arc<crate::enroll_service::EnrollService>>,
    mut shutdown: watch::Receiver<bool>,
) -> io::Result<()> {
    let permits = Arc::new(Semaphore::new(MAX_HTTP_CONNECTIONS));
    let mut connections = JoinSet::new();
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let (stream, _) = accepted?;
                let Ok(permit) = Arc::clone(&permits).try_acquire_owned() else { drop(stream); continue; };
                let token = token.clone();
                let status = status.clone();
                let enroll = enroll.clone();
                connections.spawn(async move { let _permit = permit; let _ = serve_http(stream, &token, io_deadline, &status, enroll.as_deref()).await; });
            }
            changed = shutdown.changed() => if changed.is_err() || *shutdown.borrow() { break; },
        }
        while let Some(result) = connections.try_join_next() { result.map_err(io::Error::other)?; }
    }
    connections.shutdown().await;
    Ok(())
}

async fn serve_http(mut stream: TcpStream, token: &str, deadline: Duration, status: &watch::Receiver<Arc<StatusResponse>>, enroll: Option<&crate::enroll_service::EnrollService>) -> io::Result<()> {
    let request = match timeout(deadline, read_request(&mut stream)).await {
        Ok(Ok(request)) => request,
        Ok(Err(ReadError::TooLarge)) => return write_response(&mut stream, deadline, Response::plain(413, "Payload Too Large")).await,
        Ok(Err(ReadError::Io(error))) => return Err(error),
        _ => return Ok(()),
    };
    let response = route(request, token, status.borrow().as_ref(), enroll);
    write_response(&mut stream, deadline, response).await
}

struct Request { method: String, path: String, authorization: Option<String>, body: Vec<u8> }
enum ReadError { Closed, Invalid, TooLarge, Io(io::Error) }

async fn read_request(stream: &mut TcpStream) -> Result<Request, ReadError> {
    let mut bytes = Vec::with_capacity(1024);
    let mut chunk = [0_u8; 1024];
    let header_end = loop {
        let count = stream.read(&mut chunk).await.map_err(ReadError::Io)?;
        if count == 0 { return Err(ReadError::Closed); }
        if bytes.len().saturating_add(count) > HEADER_LIMIT_BYTES + OPERATOR_BODY_LIMIT_BYTES { return Err(ReadError::TooLarge); }
        bytes.extend_from_slice(&chunk[..count]);
        if let Some(position) = bytes.windows(4).position(|window| window == b"\r\n\r\n") { break position + 4; }
    };
    let text = std::str::from_utf8(&bytes[..header_end]).map_err(|_| ReadError::Invalid)?;
    let mut lines = text.split("\r\n");
    let mut first = lines.next().ok_or(ReadError::Invalid)?.split_whitespace();
    let method = first.next().ok_or(ReadError::Invalid)?;
    let path = first.next().ok_or(ReadError::Invalid)?;
    let version = first.next().ok_or(ReadError::Invalid)?;
    if first.next().is_some() || !version.starts_with("HTTP/1.") || !path.starts_with('/') { return Err(ReadError::Invalid); }
    let mut authorization = None;
    let mut content_length = 0_usize;
    for line in lines {
        if line.is_empty() { break; }
        let (name, value) = line.split_once(':').ok_or(ReadError::Invalid)?;
        if name.eq_ignore_ascii_case("authorization") {
            if authorization.is_some() { return Err(ReadError::Invalid); }
            authorization = Some(value.trim().to_owned());
        } else if name.eq_ignore_ascii_case("content-length") {
            content_length = value.trim().parse().map_err(|_| ReadError::Invalid)?;
        }
    }
    if content_length > OPERATOR_BODY_LIMIT_BYTES { return Err(ReadError::TooLarge); }
    let mut body = bytes[header_end..].to_vec();
    while body.len() < content_length {
        let count = stream.read(&mut chunk).await.map_err(ReadError::Io)?;
        if count == 0 { return Err(ReadError::Closed); }
        body.extend_from_slice(&chunk[..count]);
    }
    body.truncate(content_length);
    Ok(Request { method: method.to_owned(), path: path.to_owned(), authorization, body })
}

fn route(request: Request, token: &str, status: &StatusResponse, enroll: Option<&crate::enroll_service::EnrollService>) -> Response {
    let path = request.path.split_once('?').map_or(request.path.as_str(), |pair| pair.0);
    // Operator enrollment endpoints: bearer-authenticated, loopback API only.
    if path == "/v1/enroll" || path.starts_with("/v1/enroll/") {
        let Some(service) = enroll else { return Response::plain(404, "Not Found"); };
        if !authorized(request.authorization.as_deref(), token) { return Response::plain(401, "Unauthorized").authenticate(); }
        let reply = service.handle_operator(&request.method, path, &request.body);
        return Response::raw_json(reply.status, reply.body);
    }
    if request.method != "GET" { return Response::plain(405, "Method Not Allowed").allow_get(); }
    match path {
        "/health" => Response::json(200, &HealthResponse { ok: true, service: "gate4agent-c2".to_owned(), api_version: C2_API_VERSION, pid: std::process::id(), version: env!("CARGO_PKG_VERSION").to_owned() }),
        "/ready" => {
            let online_nodes = status.nodes.values().filter(|node| node.transport == NodeTransportState::Online).count();
            let offline_nodes = status.nodes.values().filter(|node| node.transport == NodeTransportState::Offline).count();
            let parked_nodes = status.nodes.values().filter(|node| node.transport == NodeTransportState::Parked).count();
            let body = ReadyResponse { ready: status.ready, api_version: C2_API_VERSION, configured_nodes: status.nodes.len(), attempted_nodes: status.nodes.values().filter(|node| node.last_attempt_unix_ms.is_some()).count(), online_nodes, offline_nodes, parked_nodes };
            Response::json(if status.ready { 200 } else { 503 }, &body)
        }
        "/status" => {
            if !authorized(request.authorization.as_deref(), token) { Response::plain(401, "Unauthorized").authenticate() }
            else {
                // Nodes whose status does not hold `attach` show liveness only.
                let mut shown = status.clone();
                if let Some(service) = enroll {
                    for (id, node) in shown.nodes.iter_mut() {
                        if service.allows(id.as_str(), "attach") == Some(false) { node.inventory = None; }
                    }
                }
                Response::json(200, &shown)
            }
        }
        _ => Response::plain(404, "Not Found"),
    }
}

fn authorized(header: Option<&str>, token: &str) -> bool {
    let Some((scheme, candidate)) = header.and_then(|value| value.split_once(' ')) else { return false; };
    scheme.eq_ignore_ascii_case("bearer") && constant_time_eq(candidate.as_bytes(), token.as_bytes())
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() { return false; }
    left.iter().zip(right).fold(0_u8, |difference, (left, right)| difference | (left ^ right)) == 0
}

struct Response { status: u16, reason: &'static str, content_type: &'static str, body: Vec<u8>, headers: Vec<(&'static str, &'static str)> }
impl Response {
    fn plain(status: u16, reason: &'static str) -> Self { Self { status, reason, content_type: "text/plain; charset=utf-8", body: reason.as_bytes().to_vec(), headers: Vec::new() } }
    fn json<T: serde::Serialize>(status: u16, value: &T) -> Self {
        let body = serde_json::to_vec(value).expect("C2 DTO must serialize");
        if body.len() > RESPONSE_BODY_LIMIT_BYTES { return Self::plain(503, "Service Unavailable"); }
        let reason = if status == 200 { "OK" } else { "Service Unavailable" };
        Self { status, reason, content_type: "application/json", body, headers: Vec::new() }
    }
    fn raw_json(status: u16, body: Vec<u8>) -> Self {
        let reason = match status { 200 => "OK", 400 => "Bad Request", 404 => "Not Found", 409 => "Conflict", 429 => "Too Many Requests", _ => "Service Unavailable" };
        Self { status, reason, content_type: "application/json", body, headers: Vec::new() }
    }
    fn allow_get(mut self) -> Self { self.headers.push(("Allow", "GET")); self }
    fn authenticate(mut self) -> Self { self.headers.push(("WWW-Authenticate", "Bearer")); self }
}

async fn write_response(stream: &mut TcpStream, deadline: Duration, response: Response) -> io::Result<()> {
    let mut headers = format!("HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n", response.status, response.reason, response.content_type, response.body.len());
    for (name, value) in response.headers { headers.push_str(name); headers.push_str(": "); headers.push_str(value); headers.push_str("\r\n"); }
    headers.push_str("\r\n");
    timeout(deadline, async { stream.write_all(headers.as_bytes()).await?; stream.write_all(&response.body).await?; stream.shutdown().await }).await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "C2 HTTP write timed out"))?
}

fn unix_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis().min(u64::MAX as u128) as u64
}

#[cfg(test)]
mod endpoint_tests {
    use super::*;
    use crate::protocol::{C2RelayRoute, C2Topology};

    fn node(endpoint: &str) -> Result<C2NodeConfig, C2ConfigError> {
        C2NodeConfig::new(NodeId::new("remote-node").unwrap(), endpoint, "safe-token")
    }

    #[cfg(windows)]
    const LOCAL_ENDPOINT: &str = r"\\.\pipe\relay-route-fact";
    #[cfg(unix)]
    const LOCAL_ENDPOINT: &str = "/tmp/gate4agent-relay-route-fact.sock";

    fn projected_route(node: &C2NodeConfig, transport: NodeTransportState) -> C2RelayRoute {
        let mut observed = initial_observed_node(node);
        observed.transport = transport;
        let status = StatusResponse {
            api_version: C2_API_VERSION,
            ready: false,
            observed_at_unix_ms: 1,
            nodes: BTreeMap::from([(node.node_id.clone(), observed)]),
        };
        C2Topology::from_status(&status).nodes[0].relay_route
    }

    #[test]
    fn relay_route_fact_projects_exactly_and_survives_offline_and_parked() {
        let local = node(LOCAL_ENDPOINT).unwrap();
        let ssh = node("tcp://127.0.0.1:48100").unwrap();

        for transport in [
            NodeTransportState::Online,
            NodeTransportState::Offline,
            NodeTransportState::Parked,
        ] {
            assert_eq!(projected_route(&local, transport), C2RelayRoute::LocalIpc);
            assert_eq!(
                projected_route(&ssh, transport),
                C2RelayRoute::SshForwardedLoopback,
            );
        }
    }

    /// `accept` is the assignment that names no address, and the config
    /// has to treat it as a different KIND of route rather than as a
    /// malformed endpoint: it is how a node that cannot be dialled is
    /// declared.
    #[test]
    fn a_node_that_waits_to_be_called_is_declared_by_name_not_by_address() {
        let waiting = node("accept").unwrap();
        assert_eq!(waiting.route, C2NodeRoute::CallHome);
        assert_eq!(waiting.transport_label(), "call-home");

        // Every waiting node carries the same `accept`, so the
        // duplicate-endpoint check must not see them as two nodes fighting
        // over one socket. Their uniqueness is `node_id`'s job, and that
        // check still applies.
        let second = C2NodeConfig::new(
            NodeId::new("second-waiting-node").unwrap(),
            "accept",
            "safe-token",
        )
        .unwrap();
        let config = C2Config::new(
            "127.0.0.1:0".parse().unwrap(),
            "safe-token",
            vec![waiting.clone(), second],
        )
        .expect("two waiting nodes are not a duplicate endpoint");
        assert_eq!(config.nodes.len(), 2);
    }

    /// A node that waits for a call the relay never listens for is a
    /// deployment that can never work, so it is refused at startup instead
    /// of becoming a node that is silently offline forever.
    #[test]
    fn waiting_for_a_call_requires_somewhere_to_be_called() {
        let waiting = node("accept").unwrap();
        let config =
            C2Config::new("127.0.0.1:0".parse().unwrap(), "safe-token", vec![waiting]).unwrap();
        assert!(matches!(
            config.validate_call_home(),
            Err(C2ConfigError::CallHomeWithoutListener(_)),
        ));
        let config = config.with_node_listen("127.0.0.1:48200".parse().unwrap()).unwrap();
        assert!(config.validate_call_home().is_ok());
    }

    /// Call-home may bind loopback or a concrete C2 WireGuard tunnel address.
    /// Unspecified (`0.0.0.0`) and port 0 stay refused so plaintext frames do
    /// not land on every interface.
    #[test]
    fn the_call_home_listener_refuses_unspecified_and_accepts_tunnel() {
        let waiting = node("accept").unwrap();
        let config =
            C2Config::new("127.0.0.1:9000".parse().unwrap(), "safe-token", vec![waiting]).unwrap();
        for refused in ["0.0.0.0:48200", "127.0.0.1:0", "[::]:48200"] {
            assert!(
                matches!(
                    config.clone().with_node_listen(refused.parse().unwrap()),
                    Err(C2ConfigError::NonLoopbackNodeListen(_)),
                ),
                "accepted {refused}",
            );
        }
        let tunnel: SocketAddr = "10.88.1.2:27442".parse().unwrap();
        let accepted = config.clone().with_node_listen(tunnel).unwrap();
        assert_eq!(accepted.node_listen, Some(tunnel));
        // Sharing the API's own address would mean HTTP and node frames
        // arriving on one socket; neither parser would survive the other.
        assert!(matches!(
            config.with_node_listen("127.0.0.1:9000".parse().unwrap()),
            Err(C2ConfigError::NodeListenConflict),
        ));
    }

    #[test]
    fn ssh_forwarded_loopback_route_is_strict_canonical_and_control_stays_local() {
        let ipv4 = node("tcp://127.0.0.1:48100").unwrap();
        assert_eq!(ipv4.endpoint, "tcp://127.0.0.1:48100");
        assert_eq!(ipv4.route, C2NodeRoute::SshForwardedLoopback("127.0.0.1:48100".parse().unwrap()));
        assert_eq!(ipv4.transport_label(), "ssh-forwarded-loopback");

        let ipv6 = node("tcp://[0:0:0:0:0:0:0:1]:48100").unwrap();
        assert_eq!(ipv6.endpoint, "tcp://[::1]:48100");
        assert_eq!(ipv6.transport_label(), "ssh-forwarded-loopback");

        for invalid in [
            "tcp://localhost:48100",
            "tcp://127.0.0.2:48100",
            "tcp://0.0.0.0:48100",
            "tcp://127.0.0.1:0",
            "tcp://user@127.0.0.1:48100",
            "tcp://127.0.0.1:48100/path",
            "TCP://127.0.0.1:48100",
        ] {
            assert!(matches!(node(invalid), Err(C2ConfigError::InvalidEndpoint(_))), "accepted {invalid}");
        }

        assert!(matches!(
            C2Config::new(
                "127.0.0.1:0".parse().unwrap(),
                "safe-token",
                vec![ipv4.clone(), C2NodeConfig::new(
                    NodeId::new("duplicate-route").unwrap(),
                    "tcp://127.0.0.1:48100",
                    "safe-token",
                ).unwrap()],
            ),
            Err(C2ConfigError::DuplicateEndpoint)
        ));
        assert!(matches!(
            C2Config::new("127.0.0.1:0".parse().unwrap(), "safe-token", vec![ipv4])
                .unwrap()
                .with_control_endpoint("tcp://127.0.0.1:48101"),
            Err(C2ConfigError::InvalidControlEndpoint)
        ));
    }
}

#[cfg(all(test, windows))]
mod tests {
    // A2: former g4a relay unit tests removed with node_relay_worker.
    // Inventory/HTTP coverage lives in unix e2e and restreamer tests.
}

#[cfg(test)]
mod control_tunnel_listen_tests {
    use super::*;

    fn config() -> C2Config {
        let node = C2NodeConfig::new(
            NodeId::new("node-a").unwrap(),
            if cfg!(unix) { "/tmp/gate4agent-node-a.sock" } else { r"\\.\pipe\gate4agent-node-a" },
            "token-token",
        ).unwrap();
        C2Config::new("127.0.0.1:18321".parse().unwrap(), "api-token", vec![node]).unwrap()
    }

    #[test]
    fn control_tunnel_rejects_unspecified_and_accepts_tunnel_address() {
        let bad: SocketAddr = "0.0.0.0:27441".parse().unwrap();
        assert!(matches!(
            config().with_control_tunnel_listen(bad),
            Err(C2ConfigError::InvalidControlTunnel(_))
        ));
        let ok: SocketAddr = "10.88.1.2:27441".parse().unwrap();
        let config = config().with_control_tunnel_listen(ok).unwrap();
        assert_eq!(config.control_tunnel_listen, Some(ok));
        assert!(!config.control_endpoint.is_empty());
    }
}


#[cfg(test)]
mod opaque_restreamer_tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn with_restreamer_config_starts_without_g4a_control_listener() {
        // Waiting node: no dial-out. With restreamer, no g4a relay worker either.
        let node = C2NodeConfig::new(
            NodeId::new("n1").unwrap(),
            "accept",
            "safe-token",
        ).unwrap();
        let node_up = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap();
        let drain = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap();
        let sock = std::env::temp_dir().join(format!("c2-opaque-{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&sock);
        let config = C2Config::new("127.0.0.1:0".parse().unwrap(), "opaque-token-01234567", vec![node])
            .unwrap()
            .with_control_endpoint(sock.to_string_lossy())
            .unwrap()
            .with_restreamer(node_up, drain, "drain-token-0123456789ab")
            .unwrap()
            .with_timings(C2Timings {
                attempt_deadline: Duration::from_millis(50),
                ..C2Timings::default()
            });
        let running = C2Running::start(config).await.unwrap();
        tokio::time::sleep(Duration::from_millis(80)).await;
        assert!(!sock.exists(), "C2 must not bind the legacy operator control socket");
        let client = crate::restreamer::DrainClient::new(drain, "drain-token-0123456789ab");
        let mut session = client.connect().await.expect("drain connect");
        session.open("n1", 1, hatchery_core::ChannelKind::G4aControl).await.unwrap();
        session.resume(0).await.unwrap();
        running.shutdown_handle().shutdown();
        let _ = running.wait().await;
        let _ = std::fs::remove_file(&sock);
    }
}
