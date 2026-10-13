//! Node side of enrollment: key generation in the node's own state directory, the signed
//! request with the operator's one-time code, and the stored public answer.
//!
//! Keys live in `<state dir>/identity.key` and `<state dir>/wg.key` (directory 0700, files
//! 0600) and never leave the host; only public keys are sent. Run provider agents as another
//! user (or inside a guest) so they cannot read this directory.

use std::path::{Path, PathBuf};
use std::time::Duration;

use hatchery_enroll::{EnrollmentResponse, NodeIdentity, NodeKind};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;

const ENROLLMENT_FILE: &str = "enrollment.json";

#[derive(Debug, thiserror::Error)]
pub enum EnrollClientError {
    #[error("enrollment request failed: {0}")]
    Transport(String),
    #[error("C2 refused the request (for enrollment: the one-time code is single use and expires; for a build: the node status must hold the build capability)")]
    Refused,
    #[error("C2 answered with an unexpected reply: {0}")]
    Reply(String),
    #[error("key or state error: {0}")]
    State(String),
}

/// Public result of a successful enrollment, stored next to the keys.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EnrolledState {
    pub node_id: String,
    pub response: EnrollmentResponse,
}

/// Default state directory: `$XDG_STATE_HOME/gate4agent-node/enroll` or `$HOME/.local/state/...`.
pub fn default_enroll_dir() -> Option<PathBuf> {
    if let Some(base) = std::env::var_os("XDG_STATE_HOME").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(base).join("gate4agent-node").join("enroll"));
    }
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    Some(PathBuf::from(home).join(".local").join("state").join("gate4agent-node").join("enroll"))
}

fn unix_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}

/// Where to send the enrollment request.
#[derive(Clone, Debug)]
pub enum EnrollTarget {
    /// Plain TCP (loopback rigs, old C2s). The one-time code crosses the network in the clear.
    Plain(std::net::SocketAddr),
    /// `https://host[:port]`: the C2's TLS front (certificate verified against the WebPKI roots,
    /// host name checked, no redirects). The code is only ever sent encrypted.
    Https(String),
}

impl EnrollTarget {
    /// `https://...` or `HOST:PORT`. Plain `http://` is refused: it adds nothing over HOST:PORT
    /// and looks safer than it is.
    pub fn parse(value: &str) -> Result<Self, String> {
        if let Some(rest) = value.strip_prefix("https://") {
            let host = rest.trim_end_matches('/');
            if host.is_empty() || host.contains('/') || host.contains('@') {
                return Err("https target must be https://HOST[:PORT]".into());
            }
            return Ok(Self::Https(format!("https://{host}")));
        }
        if value.starts_with("http://") {
            return Err("use https://HOST or HOST:PORT, not http://".into());
        }
        {
            use std::net::ToSocketAddrs;
            let mut addrs: Vec<_> = value.to_socket_addrs().map_err(|e| format!("{value}: {e}"))?.collect();
            addrs.sort_by_key(|a| !a.is_ipv4());
            addrs.into_iter().next().map(Self::Plain).ok_or_else(|| format!("{value} resolves to no address"))
        }
    }
}

/// Generate (or load) the keys, send the signed request, store the public answer.
pub async fn enroll(
    enroll_addr: std::net::SocketAddr,
    node_id: &str,
    kind: NodeKind,
    code: Option<&str>,
    state_dir: &Path,
) -> Result<EnrolledState, EnrollClientError> {
    enroll_to(&EnrollTarget::Plain(enroll_addr), node_id, kind, code, state_dir).await
}

/// [`enroll`] to any [`EnrollTarget`].
pub async fn enroll_to(
    target: &EnrollTarget,
    node_id: &str,
    kind: NodeKind,
    code: Option<&str>,
    state_dir: &Path,
) -> Result<EnrolledState, EnrollClientError> {
    let identity = NodeIdentity::load_or_create(state_dir).map_err(|e| EnrollClientError::State(e.to_string()))?;
    let request = identity.sign_request(node_id, kind, code, unix_ms()).map_err(|e| EnrollClientError::State(e.to_string()))?;
    let body = serde_json::to_vec(&request).map_err(|e| EnrollClientError::State(e.to_string()))?;
    let reply = match target {
        EnrollTarget::Plain(addr) => post(*addr, "/v1/enroll", &body).await?,
        EnrollTarget::Https(base) => post_https(base, "/v1/enroll", body).await?,
    };
    if reply.0 != 200 {
        return Err(EnrollClientError::Refused);
    }
    let response: EnrollmentResponse = serde_json::from_slice(&reply.1).map_err(|e| EnrollClientError::Reply(e.to_string()))?;
    let state = EnrolledState { node_id: node_id.to_owned(), response };
    let json = serde_json::to_vec_pretty(&state).map_err(|e| EnrollClientError::State(e.to_string()))?;
    hatchery_enroll::write_private(&state_dir.join(ENROLLMENT_FILE), &json).map_err(|e| EnrollClientError::State(e.to_string()))?;
    Ok(state)
}

/// Load the identity and the stored public answer of an enrolled node.
pub fn load_enrolled(state_dir: &Path) -> Result<(NodeIdentity, EnrolledState), EnrollClientError> {
    let identity = NodeIdentity::load_or_create(state_dir).map_err(|e| EnrollClientError::State(e.to_string()))?;
    let bytes = std::fs::read(state_dir.join(ENROLLMENT_FILE))
        .map_err(|_| EnrollClientError::State("not enrolled yet: run `enroll` first".to_owned()))?;
    let state = serde_json::from_slice(&bytes).map_err(|e| EnrollClientError::State(e.to_string()))?;
    Ok((identity, state))
}

/// Load the identity for signing only (no WireGuard key is read) and the enrolled state.
/// For a node whose tunnel is brought up by a privileged helper.
pub fn load_enrolled_signing_only(state_dir: &Path) -> Result<(NodeIdentity, EnrolledState), EnrollClientError> {
    let identity = NodeIdentity::load_signing_only(state_dir).map_err(|e| EnrollClientError::State(e.to_string()))?;
    let bytes = std::fs::read(state_dir.join(ENROLLMENT_FILE))
        .map_err(|_| EnrollClientError::State("not enrolled yet: run `enroll` first".to_owned()))?;
    let state = serde_json::from_slice(&bytes).map_err(|e| EnrollClientError::State(e.to_string()))?;
    Ok((identity, state))
}

/// Ask C2 to build and sign a configured recipe on the build host. The node proves who it is
/// with its identity key; it holds no build-host credential and never sees a signing key.
/// The receipt is checked against `trusted_signer` (the operator-provided public key of the
/// build host) and the artifact against the signed hash before it is returned.
pub async fn request_build(
    c2_enroll: std::net::SocketAddr,
    identity: &NodeIdentity,
    node_id: &str,
    recipe: &str,
    source_ref: Option<&str>,
    trusted_signer: &str,
) -> Result<(hatchery_build::BuildReceipt, Vec<u8>), EnrollClientError> {
    use base64::Engine;
    use hatchery_build::wire::SignedBuildRequest;
    let ts_ms = unix_ms();
    let mut raw = [0_u8; 12];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| std::io::Read::read_exact(&mut f, &mut raw))
        .map_err(|e| EnrollClientError::State(e.to_string()))?;
    let nonce: String = raw.iter().map(|b| format!("{b:02x}")).collect();
    let message = SignedBuildRequest::signing_bytes(node_id, recipe, source_ref, ts_ms, &nonce);
    let request = SignedBuildRequest {
        node_id: node_id.to_owned(),
        recipe: recipe.to_owned(),
        source_ref: source_ref.map(str::to_owned),
        ts_ms,
        nonce,
        signature: identity.sign_bytes(&message),
    };
    let body = serde_json::to_vec(&request).map_err(|e| EnrollClientError::State(e.to_string()))?;
    let (status, reply) = hatchery_build::http::post(c2_enroll, "/v1/build", None, &body, Duration::from_secs(25 * 60))
        .await
        .map_err(|e| EnrollClientError::Transport(e.to_string()))?;
    match status {
        200 => {}
        403 => return Err(EnrollClientError::Refused),
        other => return Err(EnrollClientError::Reply(format!("{other}: {}", String::from_utf8_lossy(&reply).chars().take(200).collect::<String>()))),
    }
    let response: hatchery_build::BuildResponse = serde_json::from_slice(&reply).map_err(|e| EnrollClientError::Reply(e.to_string()))?;
    let artifact = base64::engine::general_purpose::STANDARD
        .decode(&response.artifact_b64)
        .map_err(|e| EnrollClientError::Reply(e.to_string()))?;
    response
        .receipt
        .verify(trusted_signer, Some(&artifact))
        .map_err(|e| EnrollClientError::Reply(format!("receipt rejected: {e}")))?;
    Ok((response.receipt, artifact))
}

/// Make a signed mail/task call through C2 (`op` like `mail.send`, `task.list`). The node proves
/// who it is with its identity key; no server credential is involved. Returns the reply JSON.
pub async fn node_call(
    c2_enroll: std::net::SocketAddr,
    identity: &NodeIdentity,
    node_id: &str,
    op: &str,
    payload: &serde_json::Value,
) -> Result<serde_json::Value, EnrollClientError> {
    use hatchery_enroll::call::SignedCall;
    let ts_ms = unix_ms();
    let mut raw = [0_u8; 12];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| std::io::Read::read_exact(&mut f, &mut raw))
        .map_err(|e| EnrollClientError::State(e.to_string()))?;
    let nonce: String = raw.iter().map(|b| format!("{b:02x}")).collect();
    let payload_text = payload.to_string();
    let message = SignedCall::signing_bytes(node_id, op, &payload_text, ts_ms, &nonce);
    let call = SignedCall { node_id: node_id.to_owned(), op: op.to_owned(), payload: payload_text, ts_ms, nonce, signature: identity.sign_bytes(&message) };
    let body = serde_json::to_vec(&call).map_err(|e| EnrollClientError::State(e.to_string()))?;
    let (status, reply) = hatchery_build::http::post(c2_enroll, "/v1/node-call", None, &body, Duration::from_secs(45))
        .await
        .map_err(|e| EnrollClientError::Transport(e.to_string()))?;
    match status {
        200 => serde_json::from_slice(&reply).map_err(|e| EnrollClientError::Reply(e.to_string())),
        403 => Err(EnrollClientError::Refused),
        other => Err(EnrollClientError::Reply(format!("{other}: {}", String::from_utf8_lossy(&reply).chars().take(200).collect::<String>()))),
    }
}

/// Environment the kernel WireGuard bring-up reads, from the enrolled state. Only variables
/// that are unset are filled. Returns the call-home address when the answer carries one.
pub fn wireguard_env_from_state(state_dir: &Path, state: &EnrolledState, interface: &str) -> Vec<(String, String)> {
    let r = &state.response;
    let mut out = Vec::new();
    let mut push = |k: &str, v: Option<String>| {
        if let Some(v) = v {
            out.push((k.to_owned(), v));
        }
    };
    push("GATE4AGENT_WG_INTERFACE", Some(interface.to_owned()));
    push("GATE4AGENT_WG_PRIVATE_KEY", Some(NodeIdentity::wg_path(state_dir).to_string_lossy().into_owned()));
    push("GATE4AGENT_WG_ADDRESS", r.tunnel_ip.clone());
    push("GATE4AGENT_WG_PEER_ADDRESS", r.c2_tunnel_ip.clone());
    push("GATE4AGENT_WG_PEER_KEY", r.c2_wg_public_key.clone());
    push("GATE4AGENT_WG_PEER_ENDPOINT", r.c2_wg_endpoint.clone());
    out
}

async fn post_https(base: &str, path: &str, body: Vec<u8>) -> Result<(u16, Vec<u8>), EnrollClientError> {
    let client = reqwest::Client::builder()
        .use_rustls_tls()
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| EnrollClientError::Transport(e.to_string()))?;
    let response = client
        .post(format!("{base}{path}"))
        .header("content-type", "application/json")
        .body(body)
        .send()
        .await
        .map_err(|e| EnrollClientError::Transport(format!("https: {e}")))?;
    let status = response.status().as_u16();
    let bytes = response.bytes().await.map_err(|e| EnrollClientError::Transport(e.to_string()))?;
    Ok((status, bytes.to_vec()))
}

async fn post(addr: std::net::SocketAddr, path: &str, body: &[u8]) -> Result<(u16, Vec<u8>), EnrollClientError> {
    let io = async {
        let mut stream = TcpStream::connect(addr).await?;
        let head = format!(
            "POST {path} HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        stream.write_all(head.as_bytes()).await?;
        stream.write_all(body).await?;
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).await?;
        Ok::<_, std::io::Error>(raw)
    };
    let raw = timeout(Duration::from_secs(15), io)
        .await
        .map_err(|_| EnrollClientError::Transport("timed out".to_owned()))?
        .map_err(|e| EnrollClientError::Transport(e.to_string()))?;
    let split = raw.windows(4).position(|w| w == b"\r\n\r\n").ok_or_else(|| EnrollClientError::Reply("no header end".to_owned()))?;
    let head = String::from_utf8_lossy(&raw[..split]);
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| EnrollClientError::Reply("bad status line".to_owned()))?;
    Ok((status, raw[split + 4..].to_vec()))
}

#[cfg(test)]
mod target_tests {
    use super::EnrollTarget;

    #[test]
    fn https_and_plain_targets_parse_and_http_is_refused() {
        assert!(matches!(EnrollTarget::parse("https://enroll.node.example"), Ok(EnrollTarget::Https(u)) if u == "https://enroll.node.example"));
        assert!(matches!(EnrollTarget::parse("https://enroll.example:8443/"), Ok(EnrollTarget::Https(u)) if u == "https://enroll.example:8443"));
        assert!(matches!(EnrollTarget::parse("127.0.0.1:18421"), Ok(EnrollTarget::Plain(_))));
        assert!(EnrollTarget::parse("http://enroll.example").is_err());
        assert!(EnrollTarget::parse("https://user@host").is_err());
        assert!(EnrollTarget::parse("https://host/path").is_err());
    }
}
