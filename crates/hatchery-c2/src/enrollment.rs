//! Node enrollment registry.
//!
//! A node generates two key pairs on its own host: a WireGuard key pair (only the
//! public half is ever sent) and an Ed25519 identity key pair (only the public
//! half is sent). It submits a signed [`EnrollmentRequest`]. The registry checks
//! the signature (proof of possession), freshness, replay and an optional
//! one-time code, and records the node with the neutral status
//! [`NodeStatus::Neutral`]. Only an operator call changes the status.
//! Afterwards the node authenticates by signing a challenge
//! ([`EnrollmentRegistry::verify_node_signature`]); no shared secret exists, so
//! no secret ever has to be shown to an agent.
//!
//! The registry persists to a JSON file (written atomically, mode 0600 on unix).
//! It knows nothing about what a status allows: that is policy (phase P3).

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Path, PathBuf};

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Largest accepted clock difference between node and C2 for a request.
pub const MAX_CLOCK_SKEW_MS: u64 = 5 * 60 * 1000;
const DOMAIN_ENROLL: &[u8] = b"hatchery-enroll-v1\n";
const DOMAIN_AUTH: &[u8] = b"hatchery-node-auth-v1\n";

/// Where a node runs; decides which tunnels and isolation apply later (P4).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    Local,
    Docker,
    Qemu,
    Remote,
}

impl NodeKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Docker => "docker",
            Self::Qemu => "qemu",
            Self::Remote => "remote",
        }
    }
}

/// Status of an enrolled node. Every node starts `Neutral`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeStatus {
    Neutral,
    Member,
    Trusted,
    Suspended,
    Revoked,
}

#[derive(Debug, thiserror::Error, Eq, PartialEq)]
pub enum EnrollmentError {
    #[error("node id must be 1-64 characters of a-z, 0-9, '-' or '_'")]
    InvalidNodeId,
    #[error("public key is malformed")]
    MalformedKey,
    #[error("signature is invalid")]
    BadSignature,
    #[error("request timestamp is outside the allowed clock skew")]
    Stale,
    #[error("request nonce was already used")]
    Replay,
    #[error("an enrollment code is required")]
    CodeRequired,
    #[error("enrollment code is unknown, used or expired")]
    CodeRejected,
    #[error("node id is enrolled with different keys")]
    KeyMismatch,
    #[error("node is revoked")]
    Revoked,
    #[error("node is not enrolled")]
    Unknown,
    #[error("registry storage failed: {0}")]
    Storage(String),
}

/// What a node sends. `signature` covers every other field (see [`Self::signing_bytes`]).
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct EnrollmentRequest {
    pub node_id: String,
    pub kind: NodeKind,
    /// WireGuard public key, base64, 32 bytes.
    pub wg_public_key: String,
    /// Ed25519 identity public key, base64, 32 bytes.
    pub identity_public_key: String,
    pub timestamp_ms: u64,
    pub nonce: String,
    /// One-time code the operator handed out, when the registry requires one.
    pub code: Option<String>,
    /// Ed25519 signature, base64, 64 bytes.
    pub signature: String,
}

impl EnrollmentRequest {
    /// Canonical bytes covered by the signature.
    pub fn signing_bytes(&self) -> Vec<u8> {
        let mut out = DOMAIN_ENROLL.to_vec();
        for part in [
            self.node_id.as_str(),
            self.kind.as_str(),
            self.wg_public_key.as_str(),
            self.identity_public_key.as_str(),
            &self.timestamp_ms.to_string(),
            self.nonce.as_str(),
            self.code.as_deref().unwrap_or(""),
        ] {
            out.extend_from_slice(part.as_bytes());
            out.push(b'\n');
        }
        out
    }
}

/// One audit line per state change. Never contains keys other than public ones.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AuditEntry {
    pub at_ms: u64,
    pub actor: String,
    pub action: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct NodeRecord {
    pub node_id: String,
    pub kind: NodeKind,
    pub wg_public_key: String,
    pub identity_public_key: String,
    pub status: NodeStatus,
    pub enrolled_at_ms: u64,
    pub status_changed_at_ms: u64,
}

#[derive(Default, Serialize, Deserialize)]
struct State {
    nodes: BTreeMap<String, NodeRecord>,
    /// sha256 (hex) of unused codes -> expiry ms.
    codes: BTreeMap<String, u64>,
    seen_nonces: BTreeSet<String>,
    audit: Vec<AuditEntry>,
}

/// The registry. Single-writer: wrap it in a mutex when sharing.
pub struct EnrollmentRegistry {
    path: Option<PathBuf>,
    require_code: bool,
    state: State,
}

impl EnrollmentRegistry {
    /// In-memory registry (tests, ephemeral C2).
    pub fn in_memory(require_code: bool) -> Self {
        Self { path: None, require_code, state: State::default() }
    }

    /// Load or create a persistent registry at `path`.
    pub fn open(path: &Path, require_code: bool) -> Result<Self, EnrollmentError> {
        let state = match std::fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| EnrollmentError::Storage(e.to_string()))?,
            Err(e) if e.kind() == io::ErrorKind::NotFound => State::default(),
            Err(e) => return Err(EnrollmentError::Storage(e.to_string())),
        };
        Ok(Self { path: Some(path.to_owned()), require_code, state })
    }

    /// Operator: create a one-time enrollment code valid for `ttl_ms`. The code is
    /// returned once; only its hash is stored.
    pub fn issue_code(&mut self, now_ms: u64, ttl_ms: u64, actor: &str) -> Result<String, EnrollmentError> {
        let mut raw = [0_u8; 16];
        getrandom::getrandom(&mut raw).map_err(|e| EnrollmentError::Storage(e.to_string()))?;
        let code: String = raw.iter().map(|b| format!("{b:02x}")).collect();
        self.state.codes.insert(hash_hex(code.as_bytes()), now_ms.saturating_add(ttl_ms));
        self.audit(now_ms, actor, "issue_code".to_owned());
        self.save()?;
        Ok(code)
    }

    /// Node request. Idempotent for an identical key set (returns the record).
    pub fn submit(&mut self, request: &EnrollmentRequest, now_ms: u64) -> Result<NodeRecord, EnrollmentError> {
        validate_node_id(&request.node_id)?;
        let wg = decode_key(&request.wg_public_key)?;
        let identity = decode_key(&request.identity_public_key)?;
        verify(&request.identity_public_key, &request.signing_bytes(), &request.signature)?;
        if now_ms.abs_diff(request.timestamp_ms) > MAX_CLOCK_SKEW_MS {
            return Err(EnrollmentError::Stale);
        }
        let _ = (wg, identity);
        if let Some(existing) = self.state.nodes.get(&request.node_id) {
            if existing.wg_public_key != request.wg_public_key || existing.identity_public_key != request.identity_public_key {
                return Err(EnrollmentError::KeyMismatch);
            }
            if existing.status == NodeStatus::Revoked {
                return Err(EnrollmentError::Revoked);
            }
            return Ok(existing.clone());
        }
        let nonce_key = format!("{}:{}", request.node_id, request.nonce);
        if self.state.seen_nonces.contains(&nonce_key) {
            return Err(EnrollmentError::Replay);
        }
        if self.require_code {
            let code = request.code.as_deref().ok_or(EnrollmentError::CodeRequired)?;
            let hash = hash_hex(code.as_bytes());
            match self.state.codes.get(&hash) {
                Some(expiry) if *expiry >= now_ms => {
                    self.state.codes.remove(&hash);
                }
                _ => return Err(EnrollmentError::CodeRejected),
            }
        }
        self.state.seen_nonces.insert(nonce_key);
        let record = NodeRecord {
            node_id: request.node_id.clone(),
            kind: request.kind,
            wg_public_key: request.wg_public_key.clone(),
            identity_public_key: request.identity_public_key.clone(),
            status: NodeStatus::Neutral,
            enrolled_at_ms: now_ms,
            status_changed_at_ms: now_ms,
        };
        self.state.nodes.insert(record.node_id.clone(), record.clone());
        self.audit(now_ms, &request.node_id, "enroll:neutral".to_owned());
        self.save()?;
        Ok(record)
    }

    /// Operator: change a node's status. A revoked node cannot be re-activated.
    pub fn set_status(&mut self, node_id: &str, status: NodeStatus, actor: &str, now_ms: u64) -> Result<NodeRecord, EnrollmentError> {
        let record = self.state.nodes.get_mut(node_id).ok_or(EnrollmentError::Unknown)?;
        if record.status == NodeStatus::Revoked && status != NodeStatus::Revoked {
            return Err(EnrollmentError::Revoked);
        }
        record.status = status;
        record.status_changed_at_ms = now_ms;
        let snapshot = record.clone();
        self.audit(now_ms, actor, format!("status:{node_id}:{status:?}").to_lowercase());
        self.save()?;
        Ok(snapshot)
    }

    pub fn get(&self, node_id: &str) -> Option<&NodeRecord> {
        self.state.nodes.get(node_id)
    }

    pub fn list(&self) -> Vec<NodeRecord> {
        self.state.nodes.values().cloned().collect()
    }

    pub fn audit_log(&self) -> &[AuditEntry] {
        &self.state.audit
    }

    /// Bytes a node signs to authenticate to C2 for `challenge` (a value C2 chose).
    pub fn auth_signing_bytes(node_id: &str, challenge: &[u8]) -> Vec<u8> {
        let mut out = DOMAIN_AUTH.to_vec();
        out.extend_from_slice(node_id.as_bytes());
        out.push(b'\n');
        out.extend_from_slice(challenge);
        out
    }

    /// Verify a node's signature over a C2 challenge; returns the node's status.
    /// Revoked and unknown nodes never authenticate.
    pub fn verify_node_signature(&self, node_id: &str, challenge: &[u8], signature_b64: &str) -> Result<NodeStatus, EnrollmentError> {
        let record = self.state.nodes.get(node_id).ok_or(EnrollmentError::Unknown)?;
        if record.status == NodeStatus::Revoked {
            return Err(EnrollmentError::Revoked);
        }
        verify(&record.identity_public_key, &Self::auth_signing_bytes(node_id, challenge), signature_b64)?;
        Ok(record.status)
    }

    fn audit(&mut self, at_ms: u64, actor: &str, action: String) {
        self.state.audit.push(AuditEntry { at_ms, actor: actor.to_owned(), action });
    }

    fn save(&self) -> Result<(), EnrollmentError> {
        let Some(path) = &self.path else { return Ok(()) };
        let bytes = serde_json::to_vec_pretty(&self.state).map_err(|e| EnrollmentError::Storage(e.to_string()))?;
        let tmp = path.with_extension("tmp");
        write_private(&tmp, &bytes).map_err(|e| EnrollmentError::Storage(e.to_string()))?;
        std::fs::rename(&tmp, path).map_err(|e| EnrollmentError::Storage(e.to_string()))
    }
}

fn write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

fn validate_node_id(id: &str) -> Result<(), EnrollmentError> {
    let ok = !id.is_empty()
        && id.len() <= 64
        && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_');
    if ok { Ok(()) } else { Err(EnrollmentError::InvalidNodeId) }
}

fn decode_key(value: &str) -> Result<[u8; 32], EnrollmentError> {
    let bytes = B64.decode(value).map_err(|_| EnrollmentError::MalformedKey)?;
    bytes.try_into().map_err(|_| EnrollmentError::MalformedKey)
}

fn verify(public_b64: &str, message: &[u8], signature_b64: &str) -> Result<(), EnrollmentError> {
    let key = VerifyingKey::from_bytes(&decode_key(public_b64)?).map_err(|_| EnrollmentError::MalformedKey)?;
    let sig_bytes: [u8; 64] = B64
        .decode(signature_b64)
        .map_err(|_| EnrollmentError::BadSignature)?
        .try_into()
        .map_err(|_| EnrollmentError::BadSignature)?;
    key.verify(message, &Signature::from_bytes(&sig_bytes)).map_err(|_| EnrollmentError::BadSignature)
}

fn hash_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    const NOW: u64 = 1_800_000_000_000;

    fn key(seed: u8) -> SigningKey {
        SigningKey::from_bytes(&[seed; 32])
    }

    fn pubb64(k: &SigningKey) -> String {
        B64.encode(k.verifying_key().to_bytes())
    }

    fn request(id: &str, seed: u8, nonce: &str, code: Option<&str>, ts: u64) -> EnrollmentRequest {
        let k = key(seed);
        let mut r = EnrollmentRequest {
            node_id: id.to_owned(),
            kind: NodeKind::Qemu,
            wg_public_key: B64.encode([seed.wrapping_add(1); 32]),
            identity_public_key: pubb64(&k),
            timestamp_ms: ts,
            nonce: nonce.to_owned(),
            code: code.map(str::to_owned),
            signature: String::new(),
        };
        r.signature = B64.encode(k.sign(&r.signing_bytes()).to_bytes());
        r
    }

    #[test]
    fn enrolls_as_neutral_and_only_the_operator_changes_status() {
        let mut reg = EnrollmentRegistry::in_memory(false);
        let rec = reg.submit(&request("home1", 7, "n1", None, NOW), NOW).unwrap();
        assert_eq!(rec.status, NodeStatus::Neutral);
        let again = reg.submit(&request("home1", 7, "n2", None, NOW), NOW).unwrap();
        assert_eq!(again.status, NodeStatus::Neutral, "re-submit never upgrades");
        let rec = reg.set_status("home1", NodeStatus::Member, "operator", NOW + 1).unwrap();
        assert_eq!(rec.status, NodeStatus::Member);
        assert!(reg.audit_log().iter().any(|a| a.action == "enroll:neutral"));
        assert!(reg.audit_log().iter().any(|a| a.action == "status:home1:member" && a.actor == "operator"));
    }

    #[test]
    fn rejects_bad_signature_tamper_stale_and_malformed() {
        let mut reg = EnrollmentRegistry::in_memory(false);
        let mut r = request("a", 1, "n", None, NOW);
        r.node_id = "b".into();
        assert_eq!(reg.submit(&r, NOW), Err(EnrollmentError::BadSignature));
        assert_eq!(reg.submit(&request("a", 1, "n", None, NOW - MAX_CLOCK_SKEW_MS - 1), NOW), Err(EnrollmentError::Stale));
        assert_eq!(reg.submit(&request("Bad Id", 1, "n", None, NOW), NOW), Err(EnrollmentError::InvalidNodeId));
        let mut r = request("a", 1, "n", None, NOW);
        r.wg_public_key = "short".into();
        assert_eq!(reg.submit(&r, NOW), Err(EnrollmentError::MalformedKey));
        assert!(reg.list().is_empty());
    }

    #[test]
    fn a_node_id_cannot_be_taken_over_with_other_keys() {
        let mut reg = EnrollmentRegistry::in_memory(false);
        reg.submit(&request("n", 1, "a", None, NOW), NOW).unwrap();
        assert_eq!(reg.submit(&request("n", 2, "b", None, NOW), NOW), Err(EnrollmentError::KeyMismatch));
    }

    #[test]
    fn one_time_code_is_required_single_use_and_expires() {
        let mut reg = EnrollmentRegistry::in_memory(true);
        assert_eq!(reg.submit(&request("n", 1, "a", None, NOW), NOW), Err(EnrollmentError::CodeRequired));
        let code = reg.issue_code(NOW, 1000, "operator").unwrap();
        assert_eq!(reg.submit(&request("n", 1, "a", Some("wrong"), NOW), NOW), Err(EnrollmentError::CodeRejected));
        reg.submit(&request("n", 1, "a", Some(&code), NOW), NOW).unwrap();
        assert_eq!(reg.submit(&request("m", 2, "a", Some(&code), NOW), NOW), Err(EnrollmentError::CodeRejected), "single use");
        let late = reg.issue_code(NOW, 10, "operator").unwrap();
        assert_eq!(reg.submit(&request("k", 3, "a", Some(&late), NOW + 11), NOW + 11), Err(EnrollmentError::CodeRejected));
    }

    #[test]
    fn signature_auth_follows_status_and_revocation_is_final() {
        let mut reg = EnrollmentRegistry::in_memory(false);
        reg.submit(&request("n", 5, "a", None, NOW), NOW).unwrap();
        let k = key(5);
        let challenge = b"challenge-1";
        let sig = B64.encode(k.sign(&EnrollmentRegistry::auth_signing_bytes("n", challenge)).to_bytes());
        assert_eq!(reg.verify_node_signature("n", challenge, &sig), Ok(NodeStatus::Neutral));
        assert_eq!(reg.verify_node_signature("n", b"other", &sig), Err(EnrollmentError::BadSignature));
        assert_eq!(reg.verify_node_signature("x", challenge, &sig), Err(EnrollmentError::Unknown));
        reg.set_status("n", NodeStatus::Revoked, "operator", NOW).unwrap();
        assert_eq!(reg.verify_node_signature("n", challenge, &sig), Err(EnrollmentError::Revoked));
        assert_eq!(reg.set_status("n", NodeStatus::Member, "operator", NOW), Err(EnrollmentError::Revoked));
        assert_eq!(reg.submit(&request("n", 5, "z", None, NOW), NOW), Err(EnrollmentError::Revoked));
    }

    #[test]
    fn persists_atomically_with_private_mode_and_reloads() {
        let dir = std::env::temp_dir().join(format!("hatchery-enroll-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("registry.json");
        {
            let mut reg = EnrollmentRegistry::open(&path, true).unwrap();
            let code = reg.issue_code(NOW, 10_000, "operator").unwrap();
            reg.submit(&request("n", 9, "a", Some(&code), NOW), NOW).unwrap();
            reg.set_status("n", NodeStatus::Trusted, "operator", NOW).unwrap();
            let text = std::fs::read_to_string(&path).unwrap();
            assert!(!text.contains(&code), "the plain code is never stored");
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        }
        let reg = EnrollmentRegistry::open(&path, true).unwrap();
        assert_eq!(reg.get("n").unwrap().status, NodeStatus::Trusted);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
