//! Node -> C2 -> server calls: a node-signed request for one named operation. C2 verifies the
//! signature and the status policy, then forwards to the server with a bearer only C2 holds.
//! The server trusts the node id because only C2 can present that bearer.

use serde::{Deserialize, Serialize};

pub const CALL_DOMAIN: &[u8] = b"hatchery-node-call-v1\n";
pub const MAX_SKEW_MS: u64 = 60_000;

/// Capability a node needs for an operation, by operation family. `None` = unknown operation.
pub fn capability_for(op: &str) -> Option<&'static str> {
    match op.split_once('.')?.0 {
        "mail" => Some("mail"),
        "task" => Some("tasks"),
        // Answered by C2 itself from its own configuration (no core, no node dial).
        "node" => Some("config"),
        _ => None,
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct SignedCall {
    pub node_id: String,
    pub op: String,
    /// JSON text of the operation arguments (kept as text so the signature covers exact bytes).
    pub payload: String,
    pub ts_ms: u64,
    pub nonce: String,
    pub signature: String,
}

impl SignedCall {
    pub fn signing_bytes(node_id: &str, op: &str, payload: &str, ts_ms: u64, nonce: &str) -> Vec<u8> {
        let mut out = CALL_DOMAIN.to_vec();
        for part in [node_id, op, &hex_sha256(payload.as_bytes()), &ts_ms.to_string(), nonce] {
            out.extend_from_slice(part.as_bytes());
            out.push(b'\n');
        }
        out
    }

    pub fn well_formed(&self) -> bool {
        let name = |v: &str, max: usize| !v.is_empty() && v.len() <= max && v.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'));
        name(&self.node_id, 64)
            && name(&self.op, 32)
            && capability_for(&self.op).is_some()
            && self.payload.len() <= 64 * 1024
            && !self.nonce.is_empty()
            && self.nonce.len() <= 64
            && self.nonce.bytes().all(|b| b.is_ascii_alphanumeric())
    }
}

/// C2-local op: the node asks for its public config (`{"m4a_product_url": ...}`).
pub const NODE_CONFIG_OP: &str = "node.config";

/// What C2 forwards to the server.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ForwardedCall {
    pub node_id: String,
    pub op: String,
    pub payload: serde_json::Value,
}

fn hex_sha256(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}
