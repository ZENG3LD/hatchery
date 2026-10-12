//! Enrollment wire and key handling shared by `hatchery-c2` and `hatchery-node`.
//!
//! The node generates an Ed25519 identity key and a WireGuard (X25519) key on its
//! own host and keeps them in its own state directory (files mode 0600). Only the
//! public halves leave the host. Agents run by the node never get the state dir.
//!
//! * [`EnrollmentRequest`] is the signed request sent with a one-time code.
//! * [`EnrollmentResponse`] carries only public data (tunnel address, C2 public key).
//! * Signed call-home: after the usual call-home announce, C2 sends a [`CallHomeChallenge`];
//!   the node answers with an ephemeral key and a signature ([`NodeIdentity::answer_challenge`]).
//!   Both sides derive the same per-connection session token from an X25519 exchange, so the
//!   existing mutual challenge-response of the node wire runs unchanged and no standing
//!   shared secret exists.

use std::io;
use std::path::{Path, PathBuf};

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use x25519_dalek::{PublicKey, StaticSecret};

pub mod isolation;
pub mod call;
pub mod policy;
pub use isolation::{IsolationProfile, KeyStore, KillSwitch, KillSwitchError, TunnelMode};
pub use policy::{Grants, Policy, PolicyError};

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

const DOMAIN_ENROLL: &[u8] = b"hatchery-enroll-v1\n";
const DOMAIN_AUTH: &[u8] = b"hatchery-node-auth-v1\n";
const DOMAIN_TOKEN: &[u8] = b"hatchery-session-token-v1\n";
/// Magic that starts the C2 challenge on a call-home socket (after the announce).
pub const CHALLENGE_MAGIC: &[u8; 4] = b"HSC1";
/// Challenge frame: magic + 32-byte challenge + 32-byte C2 ephemeral public key.
pub const CHALLENGE_LEN: usize = 4 + 32 + 32;
/// Answer frame: 32-byte node ephemeral public key + 64-byte signature.
pub const ANSWER_LEN: usize = 32 + 64;

#[derive(Debug, thiserror::Error, Eq, PartialEq)]
pub enum EnrollError {
    #[error("key material is malformed")]
    MalformedKey,
    #[error("signature is invalid")]
    BadSignature,
    #[error("randomness unavailable")]
    Random,
    #[error("state directory error: {0}")]
    Storage(String),
}

/// Where a node runs; decides which tunnels and isolation apply.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    Local,
    Docker,
    Qemu,
    Remote,
}

impl NodeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Docker => "docker",
            Self::Qemu => "qemu",
            Self::Remote => "remote",
        }
    }
}

/// What a node sends. `signature` covers every other field.
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
    /// One-time code the operator handed out.
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

/// Public answer to a successful enrollment. No secret of any kind.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct EnrollmentResponse {
    pub node_id: String,
    pub status: String,
    /// Address the node uses inside the node tunnel (without prefix length).
    pub tunnel_ip: Option<String>,
    /// C2 end of the node tunnel.
    pub c2_tunnel_ip: Option<String>,
    pub c2_wg_public_key: Option<String>,
    /// `host:port` of the C2 WireGuard listener.
    pub c2_wg_endpoint: Option<String>,
    /// `ip:port` of the call-home listener inside the tunnel.
    pub c2_call_home: Option<String>,
}

/// Bytes a node signs to authenticate to C2 over `material`.
pub fn auth_signing_bytes(node_id: &str, material: &[u8]) -> Vec<u8> {
    let mut out = DOMAIN_AUTH.to_vec();
    out.extend_from_slice(node_id.as_bytes());
    out.push(b'\n');
    out.extend_from_slice(material);
    out
}

pub fn decode_key32(value: &str) -> Result<[u8; 32], EnrollError> {
    B64.decode(value)
        .map_err(|_| EnrollError::MalformedKey)?
        .try_into()
        .map_err(|_| EnrollError::MalformedKey)
}

/// Verify an Ed25519 signature (all values base64).
pub fn verify_signature(
    public_b64: &str,
    message: &[u8],
    signature_b64: &str,
) -> Result<(), EnrollError> {
    let key = VerifyingKey::from_bytes(&decode_key32(public_b64)?)
        .map_err(|_| EnrollError::MalformedKey)?;
    let sig: [u8; 64] = B64
        .decode(signature_b64)
        .map_err(|_| EnrollError::BadSignature)?
        .try_into()
        .map_err(|_| EnrollError::BadSignature)?;
    key.verify(message, &Signature::from_bytes(&sig))
        .map_err(|_| EnrollError::BadSignature)
}

fn random32() -> Result<[u8; 32], EnrollError> {
    let mut bytes = [0_u8; 32];
    getrandom::getrandom(&mut bytes).map_err(|_| EnrollError::Random)?;
    Ok(bytes)
}

fn material(challenge: &[u8; 32], c2_eph: &[u8; 32], node_eph: &[u8; 32]) -> Vec<u8> {
    let mut m = Vec::with_capacity(96);
    m.extend_from_slice(challenge);
    m.extend_from_slice(c2_eph);
    m.extend_from_slice(node_eph);
    m
}

fn session_token(shared: &[u8; 32], material: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(DOMAIN_TOKEN);
    hasher.update(shared);
    hasher.update(material);
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Keys the node holds. Persisted as two 0600 files in the node's own state directory.
pub struct NodeIdentity {
    identity: SigningKey,
    wg: StaticSecret,
}

impl NodeIdentity {
    pub fn generate() -> Result<Self, EnrollError> {
        Ok(Self {
            identity: SigningKey::from_bytes(&random32()?),
            wg: StaticSecret::from(random32()?),
        })
    }

    /// Deterministic construction for tests.
    pub fn from_seeds(identity_seed: [u8; 32], wg_seed: [u8; 32]) -> Self {
        Self {
            identity: SigningKey::from_bytes(&identity_seed),
            wg: StaticSecret::from(wg_seed),
        }
    }

    pub fn identity_public_key(&self) -> String {
        B64.encode(self.identity.verifying_key().to_bytes())
    }

    pub fn wg_public_key(&self) -> String {
        B64.encode(PublicKey::from(&self.wg).to_bytes())
    }

    /// Paths of the key files inside `dir`.
    pub fn identity_path(dir: &Path) -> PathBuf {
        dir.join("identity.key")
    }

    /// The WireGuard private key file, in the base64 format `wg` and the node's
    /// `GATE4AGENT_WG_PRIVATE_KEY` path expect.
    pub fn wg_path(dir: &Path) -> PathBuf {
        dir.join("wg.key")
    }

    /// Load the keys from `dir`, or generate and store them (directory 0700, files 0600).
    pub fn load_or_create(dir: &Path) -> Result<Self, EnrollError> {
        let (ip, wp) = (Self::identity_path(dir), Self::wg_path(dir));
        if ip.exists() && wp.exists() {
            let read = |p: &Path| -> Result<[u8; 32], EnrollError> {
                let text =
                    std::fs::read_to_string(p).map_err(|e| EnrollError::Storage(e.to_string()))?;
                decode_key32(text.trim())
            };
            return Ok(Self::from_seeds(read(&ip)?, read(&wp)?));
        }
        let me = Self::generate()?;
        create_private_dir(dir).map_err(|e| EnrollError::Storage(e.to_string()))?;
        write_private(&ip, B64.encode(me.identity.to_bytes()).as_bytes())
            .map_err(|e| EnrollError::Storage(e.to_string()))?;
        write_private(&wp, B64.encode(me.wg.to_bytes()).as_bytes())
            .map_err(|e| EnrollError::Storage(e.to_string()))?;
        Ok(me)
    }

    /// Load only the signing identity (for a node whose tunnel is set up by a privileged helper
    /// and which therefore never needs, or reads, the WireGuard private key). The returned
    /// identity carries a throwaway WireGuard secret that is never written anywhere.
    pub fn load_signing_only(dir: &Path) -> Result<Self, EnrollError> {
        let text = std::fs::read_to_string(Self::identity_path(dir))
            .map_err(|e| EnrollError::Storage(e.to_string()))?;
        Ok(Self::from_seeds(decode_key32(text.trim())?, random32()?))
    }

    /// Sign arbitrary bytes with the node identity (callers add their own domain prefix).
    pub fn sign_bytes(&self, message: &[u8]) -> String {
        B64.encode(self.identity.sign(message).to_bytes())
    }

    /// Build and sign an enrollment request.
    pub fn sign_request(
        &self,
        node_id: &str,
        kind: NodeKind,
        code: Option<&str>,
        now_ms: u64,
    ) -> Result<EnrollmentRequest, EnrollError> {
        let mut request = EnrollmentRequest {
            node_id: node_id.to_owned(),
            kind,
            wg_public_key: self.wg_public_key(),
            identity_public_key: self.identity_public_key(),
            timestamp_ms: now_ms,
            nonce: random32()?.iter().map(|b| format!("{b:02x}")).collect(),
            code: code.map(str::to_owned),
            signature: String::new(),
        };
        request.signature = B64.encode(self.identity.sign(&request.signing_bytes()).to_bytes());
        Ok(request)
    }

    /// Answer a call-home challenge. Returns the answer frame and the session token.
    pub fn answer_challenge(
        &self,
        node_id: &str,
        challenge_frame: &[u8; CHALLENGE_LEN],
    ) -> Result<([u8; ANSWER_LEN], String), EnrollError> {
        if &challenge_frame[..4] != CHALLENGE_MAGIC {
            return Err(EnrollError::MalformedKey);
        }
        let challenge: [u8; 32] = challenge_frame[4..36]
            .try_into()
            .map_err(|_| EnrollError::MalformedKey)?;
        let c2_eph: [u8; 32] = challenge_frame[36..68]
            .try_into()
            .map_err(|_| EnrollError::MalformedKey)?;
        let eph = StaticSecret::from(random32()?);
        let node_eph = PublicKey::from(&eph).to_bytes();
        let m = material(&challenge, &c2_eph, &node_eph);
        let signature = self
            .identity
            .sign(&auth_signing_bytes(node_id, &m))
            .to_bytes();
        let shared = eph.diffie_hellman(&PublicKey::from(c2_eph)).to_bytes();
        let mut answer = [0_u8; ANSWER_LEN];
        answer[..32].copy_from_slice(&node_eph);
        answer[32..].copy_from_slice(&signature);
        Ok((answer, session_token(&shared, &m)))
    }
}

/// C2 side of one call-home challenge.
pub struct CallHomeChallenge {
    challenge: [u8; 32],
    eph: StaticSecret,
}

/// What C2 learns from a valid answer: the bytes the registry must verify and the token.
pub struct VerifiedMaterial {
    /// Pass to `verify_node_signature` as the challenge.
    pub material: Vec<u8>,
    /// Base64 signature from the node.
    pub signature_b64: String,
    pub session_token: String,
}

impl CallHomeChallenge {
    pub fn new() -> Result<Self, EnrollError> {
        Ok(Self {
            challenge: random32()?,
            eph: StaticSecret::from(random32()?),
        })
    }

    pub fn frame(&self) -> [u8; CHALLENGE_LEN] {
        let mut frame = [0_u8; CHALLENGE_LEN];
        frame[..4].copy_from_slice(CHALLENGE_MAGIC);
        frame[4..36].copy_from_slice(&self.challenge);
        frame[36..].copy_from_slice(&PublicKey::from(&self.eph).to_bytes());
        frame
    }

    /// Combine the node's answer with this challenge. The caller verifies the signature
    /// against the registered identity key before trusting `session_token`.
    pub fn accept_answer(&self, answer: &[u8; ANSWER_LEN]) -> VerifiedMaterial {
        let node_eph: [u8; 32] = answer[..32].try_into().expect("slice length");
        let c2_eph = PublicKey::from(&self.eph).to_bytes();
        let m = material(&self.challenge, &c2_eph, &node_eph);
        let shared = self
            .eph
            .diffie_hellman(&PublicKey::from(node_eph))
            .to_bytes();
        VerifiedMaterial {
            material: m.clone(),
            signature_b64: B64.encode(&answer[32..]),
            session_token: session_token(&shared, &m),
        }
    }
}

fn create_private_dir(dir: &Path) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// Write a file readable by the owner only.
pub fn write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn challenge_answer_agree_on_token_and_signature_verifies() {
        let node = NodeIdentity::from_seeds([1; 32], [2; 32]);
        let c2 = CallHomeChallenge::new().unwrap();
        let (answer, node_token) = node.answer_challenge("n1", &c2.frame()).unwrap();
        let v = c2.accept_answer(&answer);
        assert_eq!(v.session_token, node_token);
        verify_signature(
            &node.identity_public_key(),
            &auth_signing_bytes("n1", &v.material),
            &v.signature_b64,
        )
        .unwrap();
        assert!(verify_signature(
            &node.identity_public_key(),
            &auth_signing_bytes("n2", &v.material),
            &v.signature_b64
        )
        .is_err());
    }

    #[test]
    fn each_connection_gets_a_fresh_token() {
        let node = NodeIdentity::from_seeds([1; 32], [2; 32]);
        let a = CallHomeChallenge::new().unwrap();
        let b = CallHomeChallenge::new().unwrap();
        let (_, ta) = node.answer_challenge("n", &a.frame()).unwrap();
        let (_, tb) = node.answer_challenge("n", &b.frame()).unwrap();
        assert_ne!(ta, tb);
        assert_eq!(ta.len(), 64);
    }

    #[test]
    fn keys_persist_with_private_modes_and_public_halves_are_stable() {
        let dir = std::env::temp_dir().join(format!("hatchery-enroll-keys-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let a = NodeIdentity::load_or_create(&dir).unwrap();
        let b = NodeIdentity::load_or_create(&dir).unwrap();
        assert_eq!(a.identity_public_key(), b.identity_public_key());
        assert_eq!(a.wg_public_key(), b.wg_public_key());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for p in [
                NodeIdentity::identity_path(&dir),
                NodeIdentity::wg_path(&dir),
            ] {
                assert_eq!(
                    std::fs::metadata(p).unwrap().permissions().mode() & 0o777,
                    0o600
                );
            }
            assert_eq!(
                std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
                0o700
            );
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn signed_request_verifies_and_tamper_fails() {
        let n = NodeIdentity::from_seeds([3; 32], [4; 32]);
        let mut r = n.sign_request("x", NodeKind::Qemu, Some("c"), 5).unwrap();
        verify_signature(&r.identity_public_key, &r.signing_bytes(), &r.signature).unwrap();
        r.node_id = "y".into();
        assert!(
            verify_signature(&r.identity_public_key, &r.signing_bytes(), &r.signature).is_err()
        );
    }
}
