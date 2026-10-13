//! Nested encryption for the restreamer (A2 / A3).
//!
//! Three layers, outside-in:
//! 1. WireGuard (transport; not implemented here)
//! 2. Outer AEAD on C2's key — stripped by C2, reveals the opaque envelope
//! 3. Inner Noise-style AEAD on HQ's recipient key — stripped only by core
//!
//! C2 hands the node two public keys at registration and only holds the private
//! half of its own. The HQ recipient private key never lives on C2.

use chacha20poly1305::{
    aead::{Aead, KeyInit},
    ChaCha20Poly1305, Nonce,
};
use sha2::{Digest, Sha256};
use x25519_dalek::{PublicKey, StaticSecret};

/// 32-byte public key as handed to a node (upstream keys from C2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PublicKeyBytes(pub [u8; 32]);

impl PublicKeyBytes {
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

/// Key material C2 holds and relays.
#[derive(Clone)]
pub struct RestreamerKeys {
    c2_secret: StaticSecret,
    c2_public: PublicKey,
    /// HQ recipient public key (relayed only; C2 has no private half).
    hq_recipient_public: PublicKey,
    /// Optional previous HQ key during rotation window.
    hq_recipient_public_prev: Option<PublicKey>,
}

impl RestreamerKeys {
    pub fn generate(hq_recipient_public: PublicKeyBytes) -> Self {
        let mut seed = [0u8; 32];
        getrandom::getrandom(&mut seed).expect("os rng");
        let c2_secret = StaticSecret::from(seed);
        let c2_public = PublicKey::from(&c2_secret);
        Self {
            c2_secret,
            c2_public,
            hq_recipient_public: PublicKey::from(hq_recipient_public.0),
            hq_recipient_public_prev: None,
        }
    }

    /// Deterministic constructor for tests (no OS coupling beyond caller-supplied seeds).
    pub fn from_seeds(c2_seed: [u8; 32], hq_public: [u8; 32]) -> Self {
        let c2_secret = StaticSecret::from(c2_seed);
        let c2_public = PublicKey::from(&c2_secret);
        Self {
            c2_secret,
            c2_public,
            hq_recipient_public: PublicKey::from(hq_public),
            hq_recipient_public_prev: None,
        }
    }

    /// The two public keys C2 gives a node: (c2_key, hq_recipient_key).
    pub fn upstream_public_keys(&self) -> (PublicKeyBytes, PublicKeyBytes) {
        (
            PublicKeyBytes(*self.c2_public.as_bytes()),
            PublicKeyBytes(*self.hq_recipient_public.as_bytes()),
        )
    }

    /// Rotate HQ recipient public key; previous remains accepted until [`Self::end_rotation_window`].
    pub fn rotate_hq_recipient(&mut self, new_public: PublicKeyBytes) {
        self.hq_recipient_public_prev = Some(self.hq_recipient_public);
        self.hq_recipient_public = PublicKey::from(new_public.0);
    }

    pub fn end_rotation_window(&mut self) {
        self.hq_recipient_public_prev = None;
    }

    pub fn hq_recipient_public(&self) -> PublicKeyBytes {
        PublicKeyBytes(*self.hq_recipient_public.as_bytes())
    }

    pub fn c2_public(&self) -> PublicKeyBytes {
        PublicKeyBytes(*self.c2_public.as_bytes())
    }
}

/// HQ-side private material (core only). Never constructed on C2.
#[derive(Clone)]
pub struct HqRecipientSecret {
    secret: StaticSecret,
    public: PublicKey,
}

impl HqRecipientSecret {
    pub fn generate() -> Self {
        let mut seed = [0u8; 32];
        getrandom::getrandom(&mut seed).expect("os rng");
        Self::from_seed(seed)
    }

    pub fn from_seed(seed: [u8; 32]) -> Self {
        let secret = StaticSecret::from(seed);
        let public = PublicKey::from(&secret);
        Self { secret, public }
    }

    pub fn public_bytes(&self) -> PublicKeyBytes {
        PublicKeyBytes(*self.public.as_bytes())
    }
}

/// Ephemeral X25519 + ChaCha20-Poly1305 sealed box.
/// Wire: `eph_public (32) ‖ nonce (12) ‖ ciphertext+tag`.
fn seal(recipient: &PublicKey, plaintext: &[u8]) -> Result<Vec<u8>, CryptoError> {
    let mut eph_seed = [0u8; 32];
    getrandom::getrandom(&mut eph_seed).map_err(|e| CryptoError::Rng(e.to_string()))?;
    let eph = StaticSecret::from(eph_seed);
    let eph_pub = PublicKey::from(&eph);
    let shared = eph.diffie_hellman(recipient);
    let key = kdf(shared.as_bytes());
    let cipher = ChaCha20Poly1305::new_from_slice(&key).map_err(|_| CryptoError::BadKey)?;
    let mut nonce_bytes = [0u8; 12];
    getrandom::getrandom(&mut nonce_bytes).map_err(|e| CryptoError::Rng(e.to_string()))?;
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ct = cipher
        .encrypt(nonce, plaintext)
        .map_err(|_| CryptoError::Encrypt)?;
    let mut out = Vec::with_capacity(32 + 12 + ct.len());
    out.extend_from_slice(eph_pub.as_bytes());
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ct);
    Ok(out)
}

fn open(secret: &StaticSecret, sealed: &[u8]) -> Result<Vec<u8>, CryptoError> {
    if sealed.len() < 32 + 12 + 16 {
        return Err(CryptoError::Truncated);
    }
    let mut eph_bytes = [0u8; 32];
    eph_bytes.copy_from_slice(&sealed[..32]);
    let eph_pub = PublicKey::from(eph_bytes);
    let nonce = Nonce::from_slice(&sealed[32..44]);
    let ct = &sealed[44..];
    let shared = secret.diffie_hellman(&eph_pub);
    let key = kdf(shared.as_bytes());
    let cipher = ChaCha20Poly1305::new_from_slice(&key).map_err(|_| CryptoError::BadKey)?;
    cipher
        .decrypt(nonce, ct)
        .map_err(|_| CryptoError::Decrypt)
}

fn kdf(shared: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"hatchery-restreamer-v1");
    hasher.update(shared);
    let digest = hasher.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CryptoError {
    Rng(String),
    BadKey,
    Encrypt,
    Decrypt,
    Truncated,
}

impl std::fmt::Display for CryptoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Rng(msg) => write!(f, "rng: {msg}"),
            Self::BadKey => write!(f, "bad key"),
            Self::Encrypt => write!(f, "encrypt failed"),
            Self::Decrypt => write!(f, "decrypt failed"),
            Self::Truncated => write!(f, "truncated sealed box"),
        }
    }
}

impl std::error::Error for CryptoError {}

/// Node sealing: inner for HQ, then outer for C2.
pub fn node_seal(
    keys: &RestreamerKeys,
    plaintext: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    let inner = seal(&keys.hq_recipient_public, plaintext)?;
    seal(&keys.c2_public, &inner)
}

/// C2 strips the outer layer and returns the inner ciphertext unchanged.
pub fn c2_strip_outer(
    keys: &RestreamerKeys,
    sealed: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    open(&keys.c2_secret, sealed)
}

/// Core opens the inner layer with the HQ recipient secret.
pub fn core_open_inner(
    hq: &HqRecipientSecret,
    inner_ciphertext: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    open(&hq.secret, inner_ciphertext)
}

/// Prove C2 cannot open the inner layer: attempting with C2's secret must fail.
pub fn c2_cannot_open_inner(keys: &RestreamerKeys, inner_ciphertext: &[u8]) -> bool {
    open(&keys.c2_secret, inner_ciphertext).is_err()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_roundtrip_and_c2_blind_to_inner() {
        let hq = HqRecipientSecret::from_seed([7u8; 32]);
        let keys = RestreamerKeys::from_seeds([3u8; 32], hq.public_bytes().0);
        let (c2_pub, hq_pub) = keys.upstream_public_keys();
        assert_eq!(c2_pub, keys.c2_public());
        assert_eq!(hq_pub, hq.public_bytes());

        let plain = b"agent-message-body";
        let outer = node_seal(&keys, plain).unwrap();
        let inner = c2_strip_outer(&keys, &outer).unwrap();
        assert!(c2_cannot_open_inner(&keys, &inner));
        // known plaintext must not appear after stripping the outer layer
        assert!(!inner.windows(plain.len()).any(|w| w == plain));
        let opened = core_open_inner(&hq, &inner).unwrap();
        assert_eq!(opened, plain);
    }

    #[test]
    fn tampered_inner_rejected_by_core() {
        let hq = HqRecipientSecret::from_seed([9u8; 32]);
        let keys = RestreamerKeys::from_seeds([1u8; 32], hq.public_bytes().0);
        let outer = node_seal(&keys, b"secret").unwrap();
        let mut inner = c2_strip_outer(&keys, &outer).unwrap();
        let last = inner.len() - 1;
        inner[last] ^= 0x5a;
        assert!(core_open_inner(&hq, &inner).is_err());
    }

    #[test]
    fn rotation_window_accepts_new_key() {
        let hq_old = HqRecipientSecret::from_seed([2u8; 32]);
        let hq_new = HqRecipientSecret::from_seed([4u8; 32]);
        let mut keys = RestreamerKeys::from_seeds([5u8; 32], hq_old.public_bytes().0);
        keys.rotate_hq_recipient(hq_new.public_bytes());
        let sealed = node_seal(&keys, b"after-rotate").unwrap();
        let inner = c2_strip_outer(&keys, &sealed).unwrap();
        assert_eq!(core_open_inner(&hq_new, &inner).unwrap(), b"after-rotate");
        keys.end_rotation_window();
        assert!(keys.hq_recipient_public_prev.is_none());
    }
}
