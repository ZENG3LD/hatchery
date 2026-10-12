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

use hatchery_enroll::{CallHomeChallenge, EnrollmentRequest, EnrollmentResponse, ANSWER_LEN};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, watch};
use tokio::time::timeout;

use crate::enrollment::{EnrollmentError, EnrollmentRegistry, NodeRecord, NodeStatus, TunnelPool};

const MAX_BODY: usize = 16 * 1024;
const IO_DEADLINE: Duration = Duration::from_secs(5);
const DEFAULT_CODE_TTL_MS: u64 = 15 * 60 * 1000;

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
}

#[derive(Clone)]
pub struct EnrollConfig {
    pub registry_path: Option<std::path::PathBuf>,
    /// Default true: enrollment only with a one-time operator code.
    pub require_code: bool,
    pub pool: Option<TunnelPool>,
    /// Where nodes POST their enrollment request.
    pub listen: SocketAddr,
    pub provisioner: Arc<dyn PeerProvisioner>,
    pub public: PublicInfo,
    /// Requests per source address per minute (applies to every request, valid or not).
    pub rate_limit_per_minute: u32,
}

impl EnrollConfig {
    pub fn new(listen: SocketAddr) -> Self {
        Self {
            registry_path: None,
            require_code: true,
            pool: None,
            listen,
            provisioner: Arc::new(NoopProvisioner),
            public: PublicInfo::default(),
            rate_limit_per_minute: 30,
        }
    }
}

/// What the runtime must do after an operator or node action.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EnrollEvent {
    /// Make sure a relay worker exists for this node.
    Enrolled(String),
    /// Status changed; suspended and revoked nodes are disconnected.
    StatusChanged(String, NodeStatus),
}

pub struct EnrollService {
    registry: Mutex<EnrollmentRegistry>,
    provisioner: Arc<dyn PeerProvisioner>,
    public: PublicInfo,
    events: mpsc::UnboundedSender<EnrollEvent>,
    rate: Mutex<HashMap<IpAddr, (Instant, u32)>>,
    rate_limit: u32,
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
        let (tx, rx) = mpsc::unbounded_channel();
        let service = Arc::new(Self {
            registry: Mutex::new(registry),
            provisioner: Arc::clone(&config.provisioner),
            public: config.public.clone(),
            events: tx,
            rate: Mutex::new(HashMap::new()),
            rate_limit: config.rate_limit_per_minute.max(1),
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
                HttpReply::json(200, &serde_json::json!({ "nodes": nodes }))
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
        self.registry.lock().expect("registry lock").get(node_id).is_some_and(|r| is_active(r.status))
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
        if !is_active(status) {
            return Err("node is suspended".to_owned());
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
            let _ = timeout(IO_DEADLINE * 2, serve_one(stream, peer.ip(), &service)).await;
        });
    }
}

async fn serve_one(mut stream: TcpStream, peer: IpAddr, service: &EnrollService) -> io::Result<()> {
    let reply = match read_http(&mut stream).await {
        Ok((method, path, body)) if method == "POST" && path == "/v1/enroll" => service.handle_enroll(&body, peer),
        Ok(_) => HttpReply::error(404, "not found"),
        Err(_) => HttpReply::error(400, "bad request"),
    };
    let reason = match reply.status {
        200 => "OK",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        429 => "Too Many Requests",
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
