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
    #[error("C2 refused the enrollment (check the one-time code, it is single use and expires)")]
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

/// Generate (or load) the keys, send the signed request, store the public answer.
pub async fn enroll(
    enroll_addr: std::net::SocketAddr,
    node_id: &str,
    kind: NodeKind,
    code: Option<&str>,
    state_dir: &Path,
) -> Result<EnrolledState, EnrollClientError> {
    let identity = NodeIdentity::load_or_create(state_dir).map_err(|e| EnrollClientError::State(e.to_string()))?;
    let request = identity.sign_request(node_id, kind, code, unix_ms()).map_err(|e| EnrollClientError::State(e.to_string()))?;
    let body = serde_json::to_vec(&request).map_err(|e| EnrollClientError::State(e.to_string()))?;
    let reply = post(enroll_addr, "/v1/enroll", &body).await?;
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
