//! Runtime side of enrollment: the public enroll listener, operator operations on the
//! registry, WireGuard peer provisioning and the signed call-home decision.
//!
//! Secrets never appear in logs: neither enrollment codes, nor session tokens, nor keys.

use std::collections::HashMap;
use std::io;
use std::net::{IpAddr, SocketAddr};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use hatchery_enroll::{CallHomeChallenge, EnrollmentRequest, EnrollmentResponse, Policy, ANSWER_LEN};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, watch};
use tokio::time::timeout;

use crate::enrollment::{EnrollmentError, EnrollmentRegistry, NodeRecord, NodeStatus, TunnelPool};
use hatchery_enroll::call::ForwardedCall;

const MAX_BODY: usize = 16 * 1024;
const IO_DEADLINE: Duration = Duration::from_secs(5);
const BUILD_DEADLINE: Duration = Duration::from_secs(20 * 60);
const DEFAULT_CODE_TTL_MS: u64 = 15 * 60 * 1000;

/// Answers "may this node do X right now?" for the enforcement points in the relay.
/// `None` means the node is not an enrolled node (a token node), which policy does not govern.
pub trait CapabilityGate: Send + Sync {
    fn allows(&self, node_id: &str, capability: &str) -> Option<bool>;
}

/// Core-side applicator for admitted node calls (A2).
///
/// Invoked in-process after C2 policy checks. Must not dial core over the network —
/// production wires this to a drain consumer; tests may apply directly to a Hub.
#[async_trait::async_trait]
pub trait NodeCallSink: Send + Sync {
    async fn apply(&self, call: ForwardedCall) -> HttpReply;
}

/// Adds and removes node WireGuard peers.
pub trait PeerProvisioner: Send + Sync {
    fn add_peer(&self, wg_public_key: &str, tunnel_ip: &str) -> io::Result<()>;
    fn remove_peer(&self, wg_public_key: &str) -> io::Result<()>;
}

/// Kernel WireGuard through the `wg` tool (`wg set IF peer KEY allowed-ips IP/32`). Needs
/// CAP_NET_ADMIN. Runtime-only: C2 re-applies all active nodes at start.
pub struct WgCli {
    pub interface: String,
}

impl PeerProvisioner for WgCli {
    fn add_peer(&self, key: &str, ip: &str) -> io::Result<()> {
        run_wg(&["set", &self.interface, "peer", key, "allowed-ips", &format!("{ip}/32")])
    }
    fn remove_peer(&self, key: &str) -> io::Result<()> {
        run_wg(&["set", &self.interface, "peer", key, "remove"])
    }
}

fn run_wg(args: &[&str]) -> io::Result<()> {
    let output = Command::new("wg").args(args).output()?;
    if output.status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!("wg exited with {}", output.status)))
    }
}

/// Provisioner that does nothing (tests, nodes whose tunnel is managed elsewhere).
pub struct NoopProvisioner;

impl PeerProvisioner for NoopProvisioner {
    fn add_peer(&self, _: &str, _: &str) -> io::Result<()> {
        Ok(())
    }
    fn remove_peer(&self, _: &str) -> io::Result<()> {
        Ok(())
    }
}

/// Public values a node needs to bring its tunnel up. No secrets.
#[derive(Clone, Default)]
pub struct PublicInfo {
    pub c2_tunnel_ip: Option<String>,
    pub c2_wg_public_key: Option<String>,
    pub c2_wg_endpoint: Option<String>,
    pub c2_call_home: Option<String>,
    /// A2: outer-layer C2 public key (base64) handed to nodes at enrollment.
    pub upstream_c2_public_key: Option<String>,
    /// A2: HQ recipient public key (base64) relayed to nodes; C2 has no private half.
    pub upstream_recipient_public_key: Option<String>,
    /// Messenger product URL for enrolled nodes (operator sets `HATCHERY_M4A_PRODUCT_URL` on C2).
    pub m4a_product_url: Option<String>,
}

#[derive(Clone)]
pub struct EnrollConfig {
    pub registry_path: Option<std::path::PathBuf>,
    /// Default true: enrollment only with a one-time operator code.
    pub require_code: bool,
    /// Code enrollments only from a loopback peer (the TLS proxy on this host). A code sent in
    /// the clear from any other peer is burned (it was exposed) and the request refused.
    pub tls_only: bool,
    pub pool: Option<TunnelPool>,
    /// Where nodes POST their enrollment request.
    pub listen: SocketAddr,
    pub provisioner: Arc<dyn PeerProvisioner>,
    pub public: PublicInfo,
    /// Requests per source address per minute (applies to every request, valid or not).
    pub rate_limit_per_minute: u32,
    /// Status -> capability policy file (JSON); `None` uses the built-in table.
    pub policy_path: Option<std::path::PathBuf>,
    /// Build host to forward admitted `POST /v1/build` requests to, and the bearer it expects.
    pub build_host: Option<(SocketAddr, String)>,
    /// Deprecated (A2): previously the core address C2 pushed node-calls to.
    /// Ignored — C2 never dials core; retained so old configs still parse.
    pub server_upstream: Option<(SocketAddr, String)>,
    /// In-process sink for admitted node calls (no network push to core).
    pub node_call_sink: Option<Arc<dyn NodeCallSink>>,
}

impl EnrollConfig {
    pub fn new(listen: SocketAddr) -> Self {
        Self {
            registry_path: None,
            require_code: true,
            tls_only: false,
            pool: None,
            listen,
            provisioner: Arc::new(NoopProvisioner),
            public: PublicInfo::default(),
            rate_limit_per_minute: 30,
            policy_path: None,
            build_host: None,
            server_upstream: None,
            node_call_sink: None,
        }
    }
}

/// What the runtime must do after an operator or node action.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EnrollEvent {
    /// Make sure a relay worker exists for this node.
    Enrolled(String),
    /// Status changed; nodes that lost the heartbeat capability are disconnected.
    StatusChanged(String, NodeStatus),
    /// The policy was reloaded; nodes that no longer hold the heartbeat capability are
    /// disconnected.
    PolicyChanged,
}

pub struct EnrollService {
    registry: Mutex<EnrollmentRegistry>,
    policy: std::sync::RwLock<Policy>,
    policy_path: Option<std::path::PathBuf>,
    provisioner: Arc<dyn PeerProvisioner>,
    public: PublicInfo,
    events: mpsc::UnboundedSender<EnrollEvent>,
    rate: Mutex<HashMap<IpAddr, (Instant, u32)>>,
    rate_limit: u32,
    tls_only: bool,
    build_host: Option<(SocketAddr, String)>,
    server_upstream: Option<(SocketAddr, String)>,
    node_call_sink: Option<Arc<dyn NodeCallSink>>,
    build_nonces: Mutex<HashMap<(String, String), u64>>,
    node_rate: Mutex<HashMap<(String, &'static str), (u64, u32)>>,
    pub listen: SocketAddr,
}

#[derive(Serialize)]
struct ErrorBody<'a> {
    error: &'a str,
}

pub struct HttpReply {
    pub status: u16,
    pub body: Vec<u8>,
}

impl HttpReply {
    fn json<T: Serialize>(status: u16, value: &T) -> Self {
        Self { status, body: serde_json::to_vec(value).unwrap_or_default() }
    }
    fn error(status: u16, message: &str) -> Self {
        Self::json(status, &ErrorBody { error: message })
    }
}

fn unix_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}

fn is_active(status: NodeStatus) -> bool {
    !matches!(status, NodeStatus::Suspended | NodeStatus::Revoked)
}

impl EnrollService {
    pub fn start(config: &EnrollConfig) -> Result<(Arc<Self>, mpsc::UnboundedReceiver<EnrollEvent>), EnrollmentError> {
        let mut registry = match &config.registry_path {
            Some(path) => EnrollmentRegistry::open(path, config.require_code)?,
            None => EnrollmentRegistry::in_memory(config.require_code),
        };
        if let Some(pool) = config.pool {
            registry = registry.with_pool(pool);
        }
        let policy = match &config.policy_path {
            Some(path) => Policy::load(path).map_err(|e| EnrollmentError::Storage(e.to_string()))?,
            None => Policy::default(),
        };
        let (tx, rx) = mpsc::unbounded_channel();
        let service = Arc::new(Self {
            registry: Mutex::new(registry),
            policy: std::sync::RwLock::new(policy),
            policy_path: config.policy_path.clone(),
            provisioner: Arc::clone(&config.provisioner),
            public: config.public.clone(),
            events: tx,
            rate: Mutex::new(HashMap::new()),
            rate_limit: config.rate_limit_per_minute.max(1),
            tls_only: config.tls_only,
            build_host: config.build_host.clone(),
            server_upstream: config.server_upstream.clone(),
            node_call_sink: config.node_call_sink.clone(),
            build_nonces: Mutex::new(HashMap::new()),
            node_rate: Mutex::new(HashMap::new()),
            listen: config.listen,
        });
        // Restart persistence: re-apply every active node's peer and recreate its worker.
        for record in service.registry.lock().expect("registry lock").list() {
            if is_active(record.status) {
                service.apply_peer(&record);
                let _ = service.events.send(EnrollEvent::Enrolled(record.node_id.clone()));
            }
        }
        Ok((service, rx))
    }

    fn apply_peer(&self, record: &NodeRecord) {
        let Some(ip) = &record.tunnel_ip else { return };
        let result = if is_active(record.status) {
            self.provisioner.add_peer(&record.wg_public_key, ip)
        } else {
            self.provisioner.remove_peer(&record.wg_public_key)
        };
        if let Err(error) = result {
            tracing::warn!(node_id = %record.node_id, cause = %error, "wireguard peer update failed");
        }
    }

    fn allow(&self, peer: IpAddr) -> bool {
        let mut rate = self.rate.lock().expect("rate lock");
        let now = Instant::now();
        let entry = rate.entry(peer).or_insert((now, 0));
        if now.duration_since(entry.0) > Duration::from_secs(60) {
            *entry = (now, 0);
        }
        entry.1 += 1;
        entry.1 <= self.rate_limit
    }

    /// Public enroll request body -> reply. Errors are generic towards the caller.
    pub fn handle_enroll(&self, body: &[u8], peer: IpAddr) -> HttpReply {
        if !self.allow(peer) {
            return HttpReply::error(429, "rate limited");
        }
        let Ok(request) = serde_json::from_slice::<EnrollmentRequest>(body) else {
            return HttpReply::error(400, "malformed request");
        };
        if self.tls_only && !peer.is_loopback() {
            if let Some(code) = request.code.as_deref() {
                let burned = self.registry.lock().expect("registry lock").burn_code(code, unix_ms());
                tracing::warn!(%peer, burned, "plaintext enrollment refused; the exposed code was burned (use https)");
            }
            return HttpReply::error(403, "enrollment requires https");
        }
        let result = self.registry.lock().expect("registry lock").submit(&request, unix_ms());
        match result {
            Ok(record) => {
                self.apply_peer(&record);
                let _ = self.events.send(EnrollEvent::Enrolled(record.node_id.clone()));
                tracing::info!(node_id = %record.node_id, status = ?record.status, "node enrolled");
                HttpReply::json(
                    200,
                    &EnrollmentResponse {
                        node_id: record.node_id,
                        status: format!("{:?}", record.status).to_lowercase(),
                        tunnel_ip: record.tunnel_ip,
                        c2_tunnel_ip: self.public.c2_tunnel_ip.clone(),
                        c2_wg_public_key: self.public.c2_wg_public_key.clone(),
                        c2_wg_endpoint: self.public.c2_wg_endpoint.clone(),
                        c2_call_home: self.public.c2_call_home.clone(),
                        upstream_c2_public_key: self.public.upstream_c2_public_key.clone(),
                        upstream_recipient_public_key: self.public.upstream_recipient_public_key.clone(),
                        m4a_product_url: self.public.m4a_product_url.clone(),
                    },
                )
            }
            Err(error) => {
                tracing::warn!(cause = %error, %peer, "enrollment refused");
                // Do not reveal which check failed beyond the broad class.
                match error {
                    EnrollmentError::Storage(_) => HttpReply::error(503, "enrollment unavailable"),
                    EnrollmentError::Revoked => HttpReply::error(403, "refused"),
                    _ => HttpReply::error(403, "refused"),
                }
            }
        }
    }

    /// Per-node limit for signature-authenticated calls (applied after the signature checks, so
    /// anonymous traffic cannot burn a node's budget): per minute.
    fn node_allow(&self, node_id: &str, kind: &'static str, per_minute: u32) -> bool {
        let now = unix_ms();
        let mut map = self.node_rate.lock().expect("rate lock");
        let slot = map.entry((node_id.to_owned(), kind)).or_insert((now, 0));
        if now.saturating_sub(slot.0) >= 60_000 {
            *slot = (now, 0);
        }
        slot.1 += 1;
        slot.1 <= per_minute
    }

    /// `POST /v1/build`: a node-signed request to build and sign a configured recipe.
    /// Checks, in order: shape, clock window, replay, node signature (enrolled identity key),
    /// status/capability policy (`build`), then forwards to the build host with a bearer the
    /// node never sees. Failures towards the node are generic.
    pub async fn handle_build(&self, body: &[u8], peer: IpAddr) -> HttpReply {
        use hatchery_build::wire::{HostRequest, SignedBuildRequest, MAX_SKEW_MS};
        let Ok(request) = serde_json::from_slice::<SignedBuildRequest>(body) else {
            return HttpReply::error(400, "malformed request");
        };
        if !request.well_formed() {
            return HttpReply::error(400, "malformed request");
        }
        let now = unix_ms();
        if now.abs_diff(request.ts_ms) > MAX_SKEW_MS {
            return HttpReply::error(403, "refused");
        }
        let message = SignedBuildRequest::signing_bytes(&request.node_id, &request.recipe, request.source_ref.as_deref(), request.ts_ms, &request.nonce);
        let verified = self.registry.lock().expect("registry lock").verify_node_message(&request.node_id, &message, &request.signature);
        if verified.is_err() {
            tracing::warn!(node_id = %request.node_id, %peer, "build request refused: signature");
            return HttpReply::error(403, "refused");
        }
        if !self.node_allow(&request.node_id, "build", 30) {
            return HttpReply::error(429, "rate limited");
        }
        {
            let mut seen = self.build_nonces.lock().expect("nonce lock");
            seen.retain(|_, ts| now.abs_diff(*ts) <= MAX_SKEW_MS * 2);
            if seen.insert((request.node_id.clone(), request.nonce.clone()), request.ts_ms).is_some() {
                tracing::warn!(node_id = %request.node_id, "build request refused: replay");
                return HttpReply::error(403, "refused");
            }
        }
        if self.allows(&request.node_id, "build") != Some(true) {
            tracing::warn!(node_id = %request.node_id, "build request refused: status does not hold build");
            return HttpReply::error(403, "refused");
        }
        let Some((addr, bearer)) = &self.build_host else {
            return HttpReply::error(503, "build service unavailable");
        };
        let forward = HostRequest { node_id: request.node_id.clone(), recipe: request.recipe.clone(), source_ref: request.source_ref.clone() };
        let payload = serde_json::to_vec(&forward).unwrap_or_default();
        match hatchery_build::http::post(*addr, "/v1/build", Some(bearer), &payload, BUILD_DEADLINE).await {
            Ok((status, body)) => {
                tracing::info!(node_id = %request.node_id, recipe = %request.recipe, status, "build forwarded");
                HttpReply { status, body }
            }
            Err(error) => {
                tracing::warn!(node_id = %request.node_id, cause = %error, "build host unreachable");
                HttpReply::error(502, "build service unavailable")
            }
        }
    }

    /// `POST /v1/node-call`: a node-signed mail/task operation. Same gate as builds (shape, clock,
    /// replay, signature, status policy for the operation's capability), then forwarded to the
    /// server's control API with a bearer only C2 holds. The server re-checks its own ledger.
    pub async fn handle_node_call(&self, body: &[u8], peer: IpAddr) -> HttpReply {
        use hatchery_enroll::call::{capability_for, SignedCall, MAX_SKEW_MS};
        let Ok(call) = serde_json::from_slice::<SignedCall>(body) else {
            return HttpReply::error(400, "malformed request");
        };
        if !call.well_formed() {
            return HttpReply::error(400, "malformed request");
        }
        let now = unix_ms();
        if now.abs_diff(call.ts_ms) > MAX_SKEW_MS {
            return HttpReply::error(403, "refused");
        }
        let message = SignedCall::signing_bytes(&call.node_id, &call.op, &call.payload, call.ts_ms, &call.nonce);
        if self.registry.lock().expect("registry lock").verify_node_message(&call.node_id, &message, &call.signature).is_err() {
            tracing::warn!(node_id = %call.node_id, %peer, "node call refused: signature");
            return HttpReply::error(403, "refused");
        }
        if !self.node_allow(&call.node_id, "call", 600) {
            return HttpReply::error(429, "rate limited");
        }
        {
            let mut seen = self.build_nonces.lock().expect("nonce lock");
            seen.retain(|_, ts| now.abs_diff(*ts) <= MAX_SKEW_MS * 2);
            if seen.insert((call.node_id.clone(), call.nonce.clone()), call.ts_ms).is_some() {
                tracing::warn!(node_id = %call.node_id, "node call refused: replay");
                return HttpReply::error(403, "refused");
            }
        }
        let capability = capability_for(&call.op).unwrap_or("none");
        if self.allows(&call.node_id, capability) != Some(true) {
            tracing::warn!(node_id = %call.node_id, op = %call.op, "node call refused: status does not hold the capability");
            return HttpReply::error(403, "refused");
        }
        // A2: never dial core. Apply through an in-process sink (drain consumer / test hub).
        let _ = capability;
        let Ok(payload) = serde_json::from_str::<serde_json::Value>(&call.payload) else {
            return HttpReply::error(400, "malformed payload");
        };
        let forward = ForwardedCall {
            node_id: call.node_id.clone(),
            op: call.op.clone(),
            payload,
        };
        let Some(sink) = &self.node_call_sink else {
            if self.server_upstream.is_some() {
                tracing::warn!(
                    node_id = %call.node_id,
                    op = %call.op,
                    "node-call push to core is removed (A2); configure node_call_sink"
                );
            }
            return HttpReply::error(410, "use restreamer uplink");
        };
        sink.apply(forward).await
    }

    /// Operator endpoints under `/v1/enroll/...` (the caller has checked the bearer).
    pub fn handle_operator(&self, method: &str, path: &str, body: &[u8]) -> HttpReply {
        let rest = path.trim_start_matches("/v1/enroll").trim_matches('/');
        let parts: Vec<&str> = rest.split('/').filter(|p| !p.is_empty()).collect();
        match (method, parts.as_slice()) {
            ("POST", ["codes"]) => {
                #[derive(Deserialize, Default)]
                struct Body {
                    ttl_ms: Option<u64>,
                }
                let ttl = serde_json::from_slice::<Body>(body).unwrap_or_default().ttl_ms.unwrap_or(DEFAULT_CODE_TTL_MS);
                match self.registry.lock().expect("registry lock").issue_code(unix_ms(), ttl.min(24 * 3600 * 1000), "operator") {
                    Ok(code) => HttpReply::json(200, &serde_json::json!({ "code": code, "ttl_ms": ttl })),
                    Err(_) => HttpReply::error(503, "storage failed"),
                }
            }
            ("GET", ["nodes"]) => {
                let nodes = self.registry.lock().expect("registry lock").list();
                let policy = self.policy.read().expect("policy lock");
                let nodes: Vec<serde_json::Value> = nodes
                    .into_iter()
                    .map(|n| {
                        let grants = policy.grants(n.status);
                        let mut value = serde_json::to_value(&n).unwrap_or_default();
                        value["grants"] = serde_json::to_value(grants).unwrap_or_default();
                        value
                    })
                    .collect();
                HttpReply::json(200, &serde_json::json!({ "nodes": nodes }))
            }
            ("GET", ["policy"]) => HttpReply::json(200, &*self.policy.read().expect("policy lock")),
            ("POST", ["policy", "reload"]) => match self.reload_policy() {
                Ok(()) => HttpReply::json(200, &*self.policy.read().expect("policy lock")),
                Err(message) => HttpReply::json(409, &serde_json::json!({ "error": message })),
            },
            ("POST", ["nodes", id, "allows"]) => {
                #[derive(Deserialize)]
                struct Body {
                    capability: String,
                }
                match serde_json::from_slice::<Body>(body) {
                    Ok(b) => match self.allows(id, &b.capability) {
                        Some(allowed) => HttpReply::json(200, &serde_json::json!({ "allowed": allowed })),
                        None => HttpReply::error(404, "unknown node"),
                    },
                    Err(_) => HttpReply::error(400, "body must be {\"capability\": ...}"),
                }
            }
            ("POST", ["nodes", id, "status"]) => {
                #[derive(Deserialize)]
                struct Body {
                    status: NodeStatus,
                }
                match serde_json::from_slice::<Body>(body) {
                    Ok(b) => self.set_status(id, b.status),
                    Err(_) => HttpReply::error(400, "body must be {\"status\": neutral|member|trusted|suspended|revoked}"),
                }
            }
            ("POST", ["nodes", id, "revoke"]) => self.set_status(id, NodeStatus::Revoked),
            _ => HttpReply::error(404, "not found"),
        }
    }

    fn set_status(&self, id: &str, status: NodeStatus) -> HttpReply {
        let result = self.registry.lock().expect("registry lock").set_status(id, status, "operator", unix_ms());
        match result {
            Ok(record) => {
                self.apply_peer(&record);
                let _ = self.events.send(EnrollEvent::StatusChanged(record.node_id.clone(), record.status));
                tracing::info!(node_id = %record.node_id, status = ?record.status, "node status changed");
                HttpReply::json(200, &record)
            }
            Err(EnrollmentError::Unknown) => HttpReply::error(404, "unknown node"),
            Err(EnrollmentError::Revoked) => HttpReply::error(409, "node is revoked"),
            Err(_) => HttpReply::error(503, "storage failed"),
        }
    }

    /// Is this node enrolled and allowed to connect (neutral, member, trusted)?
    pub fn callhome_allowed(&self, node_id: &str) -> bool {
        self.allows(node_id, "heartbeat").unwrap_or(false)
    }

    /// Policy decision for an enrolled node: its current status must hold `capability`.
    pub fn allows(&self, node_id: &str, capability: &str) -> Option<bool> {
        let status = self.registry.lock().expect("registry lock").get(node_id).map(|r| r.status)?;
        Some(self.policy.read().expect("policy lock").allows(status, capability))
    }

    /// Re-read the policy file. On any error the previous policy stays in force.
    pub fn reload_policy(&self) -> Result<(), String> {
        let path = self.policy_path.as_ref().ok_or_else(|| "no policy file is configured".to_owned())?;
        let next = Policy::load(path).map_err(|e| e.to_string())?;
        *self.policy.write().expect("policy lock") = next;
        let _ = self.events.send(EnrollEvent::PolicyChanged);
        tracing::info!("policy reloaded");
        Ok(())
    }

    /// Enrolled nodes whose current status no longer holds `capability`.
    pub fn nodes_without(&self, capability: &str) -> Vec<String> {
        let policy = self.policy.read().expect("policy lock");
        self.registry
            .lock()
            .expect("registry lock")
            .list()
            .into_iter()
            .filter(|r| !policy.allows(r.status, capability))
            .map(|r| r.node_id)
            .collect()
    }

    /// Run the signed call-home exchange on a socket whose announce named `node_id`.
    /// Returns the per-connection session token on success.
    pub async fn signed_call_home(&self, stream: &mut TcpStream, node_id: &str) -> Result<String, String> {
        if !self.callhome_allowed(node_id) {
            return Err("node is not enrolled or not allowed".to_owned());
        }
        let challenge = CallHomeChallenge::new().map_err(|e| e.to_string())?;
        timeout(IO_DEADLINE, stream.write_all(&challenge.frame())).await.map_err(|_| "challenge write timed out")?.map_err(|e| e.to_string())?;
        let mut answer = [0_u8; ANSWER_LEN];
        timeout(IO_DEADLINE, stream.read_exact(&mut answer)).await.map_err(|_| "answer timed out")?.map_err(|e| e.to_string())?;
        let verified = challenge.accept_answer(&answer);
        let status = self
            .registry
            .lock()
            .expect("registry lock")
            .verify_node_signature(node_id, &verified.material, &verified.signature_b64)
            .map_err(|e| e.to_string())?;
        if !is_active(status) || !self.policy.read().expect("policy lock").allows(status, "heartbeat") {
            return Err("node status does not permit calling home".to_owned());
        }
        Ok(verified.session_token)
    }
}

/// Public enroll listener: only `POST /v1/enroll`.
pub async fn serve_enroll(listener: TcpListener, service: Arc<EnrollService>, mut shutdown: watch::Receiver<bool>) -> io::Result<()> {
    tracing::info!(listen = %service.listen, "enrollment listener bound");
    let permits = Arc::new(tokio::sync::Semaphore::new(64));
    loop {
        let (stream, peer) = tokio::select! {
            accepted = listener.accept() => accepted?,
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() { return Ok(()); }
                continue;
            }
        };
        let Ok(permit) = Arc::clone(&permits).try_acquire_owned() else { continue };
        let service = Arc::clone(&service);
        tokio::spawn(async move {
            let _permit = permit;
            let _ = serve_one(stream, peer.ip(), &service).await;
        });
    }
}

async fn serve_one(mut stream: TcpStream, peer: IpAddr, service: &EnrollService) -> io::Result<()> {
    let request = timeout(IO_DEADLINE * 2, read_http(&mut stream)).await;
    let reply = match request {
        Ok(Ok((method, path, body))) if method == "POST" && path == "/v1/enroll" => service.handle_enroll(&body, peer),
        Ok(Ok((method, path, body))) if method == "POST" && path == "/v1/build" => {
            timeout(BUILD_DEADLINE + IO_DEADLINE, service.handle_build(&body, peer))
                .await
                .unwrap_or_else(|_| HttpReply::error(504, "build timed out"))
        }
        Ok(Ok((method, path, body))) if method == "POST" && path == "/v1/node-call" => {
            timeout(Duration::from_secs(45), service.handle_node_call(&body, peer))
                .await
                .unwrap_or_else(|_| HttpReply::error(504, "timed out"))
        }
        Ok(Ok(_)) => HttpReply::error(404, "not found"),
        _ => HttpReply::error(400, "bad request"),
    };
    let reason = match reply.status {
        200 => "OK",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        429 => "Too Many Requests",
        422 => "Unprocessable Entity",
        502 => "Bad Gateway",
        504 => "Gateway Timeout",
        _ => "Service Unavailable",
    };
    let head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        reply.status,
        reason,
        reply.body.len()
    );
    stream.write_all(head.as_bytes()).await?;
    stream.write_all(&reply.body).await?;
    stream.shutdown().await
}

/// Minimal HTTP/1.1 request reader: (method, path, body). Body limited to [`MAX_BODY`].
pub async fn read_http(stream: &mut TcpStream) -> io::Result<(String, String, Vec<u8>)> {
    let mut bytes = Vec::with_capacity(2048);
    let mut chunk = [0_u8; 2048];
    let header_end = loop {
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        bytes.extend_from_slice(&chunk[..n]);
        if bytes.len() > MAX_BODY + 8192 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "too large"));
        }
        if let Some(pos) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
            break pos + 4;
        }
    };
    let head = std::str::from_utf8(&bytes[..header_end]).map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "utf8"))?;
    let mut lines = head.split("\r\n");
    let mut first = lines.next().unwrap_or_default().split_whitespace();
    let method = first.next().unwrap_or_default().to_owned();
    let path = first.next().unwrap_or_default().to_owned();
    let mut length = 0_usize;
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                length = value.trim().parse().map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "length"))?;
            }
        }
    }
    if length > MAX_BODY {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "body too large"));
    }
    let mut body = bytes[header_end..].to_vec();
    while body.len() < length {
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        body.extend_from_slice(&chunk[..n]);
    }
    body.truncate(length);
    Ok((method, path, body))
}

impl CapabilityGate for EnrollService {
    fn allows(&self, node_id: &str, capability: &str) -> Option<bool> {
        EnrollService::allows(self, node_id, capability)
    }
}

#[cfg(test)]
mod tls_only_tests {
    use super::*;
    use hatchery_enroll::{NodeIdentity, NodeKind};

    fn code(service: &EnrollService) -> String {
        let reply = service.handle_operator("POST", "/v1/enroll/codes", b"{}");
        assert_eq!(reply.status, 200);
        serde_json::from_slice::<serde_json::Value>(&reply.body).unwrap()["code"].as_str().unwrap().to_owned()
    }

    fn request(dir: &std::path::Path, id: &str, code: &str) -> Vec<u8> {
        let identity = NodeIdentity::load_or_create(dir).unwrap();
        serde_json::to_vec(&identity.sign_request(id, NodeKind::Local, Some(code), unix_ms()).unwrap()).unwrap()
    }

    #[test]
    fn a_code_sent_in_the_clear_is_refused_and_burned_while_the_proxy_path_works() {
        let dir = std::env::temp_dir().join(format!("c2-tls-only-{}", std::process::id()));
        let mut config = EnrollConfig::new("127.0.0.1:0".parse().unwrap());
        config.tls_only = true;
        let (service, _events) = EnrollService::start(&config).unwrap();
        let public: IpAddr = "203.0.113.9".parse().unwrap();
        let loopback: IpAddr = "127.0.0.1".parse().unwrap();

        let leaked = code(&service);
        let body = request(&dir.join("a"), "alpha", &leaked);
        assert_eq!(service.handle_enroll(&body, public).status, 403);
        // The exposed code no longer works even through the proxy.
        let body = request(&dir.join("a"), "alpha", &leaked);
        assert_eq!(service.handle_enroll(&body, loopback).status, 403);

        let fresh = code(&service);
        let body = request(&dir.join("b"), "beta", &fresh);
        assert_eq!(service.handle_enroll(&body, loopback).status, 200);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn without_the_flag_plaintext_still_enrolls() {
        let dir = std::env::temp_dir().join(format!("c2-tls-off-{}", std::process::id()));
        let (service, _events) = EnrollService::start(&EnrollConfig::new("127.0.0.1:0".parse().unwrap())).unwrap();
        let c = code(&service);
        let body = request(&dir, "gamma", &c);
        assert_eq!(service.handle_enroll(&body, "203.0.113.9".parse().unwrap()).status, 200);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn node_call_no_longer_pushes_to_core() {
        use hatchery_enroll::call::SignedCall;
        let dir = std::env::temp_dir().join(format!("c2-no-push-{}", std::process::id()));
        let mut config = EnrollConfig::new("127.0.0.1:0".parse().unwrap());
        // Even if an old launch line still sets upstream, C2 must not dial it.
        config.server_upstream = Some(("127.0.0.1:9".parse().unwrap(), "token-not-used-here!".into()));
        let (service, _events) = EnrollService::start(&config).unwrap();
        let identity = NodeIdentity::load_or_create(&dir).unwrap();
        let c = code(&service);
        let body = request(&dir, "delta", &c);
        assert_eq!(service.handle_enroll(&body, "127.0.0.1".parse().unwrap()).status, 200);
        service.handle_operator(
            "POST",
            "/v1/enroll/nodes/delta/status",
            br#"{"status":"trusted"}"#,
        );
        let ts = unix_ms();
        let payload = "{}".to_owned();
        let msg = SignedCall::signing_bytes("delta", "mail.inbox", &payload, ts, "noncea21");
        let signed = SignedCall {
            node_id: "delta".into(),
            op: "mail.inbox".into(),
            payload,
            ts_ms: ts,
            nonce: "noncea21".into(),
            signature: identity.sign_bytes(&msg),
        };
        let reply = service
            .handle_node_call(&serde_json::to_vec(&signed).unwrap(), "127.0.0.1".parse().unwrap())
            .await;
        assert_eq!(reply.status, 410, "push path removed");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
