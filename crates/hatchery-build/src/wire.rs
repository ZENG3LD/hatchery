//! Wire types: the node's signed request, the host request C2 forwards, and the signed receipt.

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use ed25519_dalek::{Signature, Signer as _, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};

pub const REQUEST_DOMAIN: &[u8] = b"hatchery-build-request-v1\n";
pub const RECEIPT_DOMAIN: &[u8] = b"hatchery-build-receipt-v1\n";
/// How far a node's request timestamp may be from the C2 clock.
pub const MAX_SKEW_MS: u64 = 60_000;

pub fn valid_name(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'_' | b'.'))
}

pub fn valid_source_ref(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && !value.contains("..")
        && value.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'/'))
}

/// What a node sends to C2 (signed with its enrolled identity key).
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct SignedBuildRequest {
    pub node_id: String,
    pub recipe: String,
    #[serde(default)]
    pub source_ref: Option<String>,
    pub ts_ms: u64,
    pub nonce: String,
    pub signature: String,
}

impl SignedBuildRequest {
    pub fn signing_bytes(node_id: &str, recipe: &str, source_ref: Option<&str>, ts_ms: u64, nonce: &str) -> Vec<u8> {
        let mut out = REQUEST_DOMAIN.to_vec();
        for part in [node_id, recipe, source_ref.unwrap_or(""), &ts_ms.to_string(), nonce] {
            out.extend_from_slice(part.as_bytes());
            out.push(b'\n');
        }
        out
    }

    pub fn well_formed(&self) -> bool {
        valid_name(&self.node_id, 64)
            && valid_name(&self.recipe, 64)
            && self.source_ref.as_deref().is_none_or(valid_source_ref)
            && !self.nonce.is_empty()
            && self.nonce.len() <= 64
            && self.nonce.bytes().all(|b| b.is_ascii_alphanumeric())
    }
}

/// What C2 forwards to the build host (after its own admission check).
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct HostRequest {
    pub node_id: String,
    pub recipe: String,
    #[serde(default)]
    pub source_ref: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct BuildReceipt {
    pub node_id: String,
    pub recipe: String,
    pub source_ref: Option<String>,
    pub artifact_sha256: String,
    pub artifact_size: u64,
    pub built_at_ms: u64,
    /// Public half of the build host's signing key. The private half never leaves the host.
    pub signer_public_key: String,
    pub signature: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct BuildResponse {
    pub receipt: BuildReceipt,
    pub artifact_b64: String,
}

#[derive(Debug, thiserror::Error, Eq, PartialEq)]
pub enum ReceiptError {
    #[error("receipt is signed by a different key than the trusted signer")]
    UntrustedSigner,
    #[error("receipt signature is invalid")]
    BadSignature,
    #[error("artifact does not match the receipt hash")]
    ArtifactMismatch,
}

impl BuildReceipt {
    pub fn signing_bytes(&self) -> Vec<u8> {
        let mut out = RECEIPT_DOMAIN.to_vec();
        for part in [
            self.node_id.as_str(),
            self.recipe.as_str(),
            self.source_ref.as_deref().unwrap_or(""),
            self.artifact_sha256.as_str(),
            &self.artifact_size.to_string(),
            &self.built_at_ms.to_string(),
        ] {
            out.extend_from_slice(part.as_bytes());
            out.push(b'\n');
        }
        out
    }

    pub fn sign(mut self, key: &SigningKey) -> Self {
        self.signer_public_key = B64.encode(key.verifying_key().to_bytes());
        self.signature = B64.encode(key.sign(&self.signing_bytes()).to_bytes());
        self
    }

    /// Verify against the operator-provided public key of the build host, and (when given)
    /// that the delivered artifact has the signed hash.
    pub fn verify(&self, trusted_public_key: &str, artifact: Option<&[u8]>) -> Result<(), ReceiptError> {
        if self.signer_public_key != trusted_public_key {
            return Err(ReceiptError::UntrustedSigner);
        }
        let key = hatchery_enroll::decode_key32(trusted_public_key).map_err(|_| ReceiptError::BadSignature)?;
        let key = VerifyingKey::from_bytes(&key).map_err(|_| ReceiptError::BadSignature)?;
        let sig = B64.decode(&self.signature).map_err(|_| ReceiptError::BadSignature)?;
        let sig = Signature::from_slice(&sig).map_err(|_| ReceiptError::BadSignature)?;
        key.verify(&self.signing_bytes(), &sig).map_err(|_| ReceiptError::BadSignature)?;
        if let Some(bytes) = artifact {
            use sha2::{Digest, Sha256};
            let digest = hex(&Sha256::digest(bytes));
            if digest != self.artifact_sha256 || bytes.len() as u64 != self.artifact_size {
                return Err(ReceiptError::ArtifactMismatch);
            }
        }
        Ok(())
    }
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
