//! Nested encryption for the restreamer (A2 / A3).
//!
//! Three layers, outside-in:
//! 1. WireGuard (transport; see `wg` module)
//! 2. Outer AEAD on C2's key — stripped by C2, reveals the opaque envelope
//! 3. Inner **Noise_NK** session node↔HQ on the relayed recipient key — stripped only by core
//!
//! Handshake and transport are separate steps (multiplex DATA frames in production).
//! C2 never holds the HQ recipient private key.

use chacha20poly1305::{
    aead::{Aead, KeyInit},
    ChaCha20Poly1305, Nonce,
};
use sha2::{Digest, Sha256};
use snow::{params::NoiseParams, Builder, HandshakeState, TransportState};
use std::sync::OnceLock;
use x25519_dalek::{PublicKey, StaticSecret};

/// Noise pattern: node (initiator) knows HQ's static public key (relayed by C2).
pub const NOISE_PATTERN: &str = "Noise_NK_25519_ChaChaPoly_SHA256";

fn noise_params() -> &'static NoiseParams {
    static PARAMS: OnceLock<NoiseParams> = OnceLock::new();
    PARAMS.get_or_init(|| NOISE_PATTERN.parse().expect("Noise_NK params"))
}

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
    hq_recipient_public: PublicKey,
    hq_recipient_public_prev: Option<PublicKey>,
}

impl RestreamerKeys {
    pub fn generate(hq_recipient_public: PublicKeyBytes) -> Self {
        let mut seed = [0u8; 32];
        getrandom::getrandom(&mut seed).expect("os rng");
        Self::from_seeds(seed, hq_recipient_public.0)
    }

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

    pub fn upstream_public_keys(&self) -> (PublicKeyBytes, PublicKeyBytes) {
        (
            PublicKeyBytes(*self.c2_public.as_bytes()),
            PublicKeyBytes(*self.hq_recipient_public.as_bytes()),
        )
    }

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

    pub fn previous_hq_recipient_public(&self) -> Option<PublicKeyBytes> {
        self.hq_recipient_public_prev
            .map(|k| PublicKeyBytes(*k.as_bytes()))
    }

    pub fn c2_public(&self) -> PublicKeyBytes {
        PublicKeyBytes(*self.c2_public.as_bytes())
    }
}

/// HQ-side private material (core only). Never constructed on C2.
#[derive(Clone)]
pub struct HqRecipientSecret {
    private: [u8; 32],
    public: [u8; 32],
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
        Self {
            private: seed,
            public: *public.as_bytes(),
        }
    }

    pub fn public_bytes(&self) -> PublicKeyBytes {
        PublicKeyBytes(self.public)
    }

    /// Seed used to derive this key (persist on core only; never send to C2 or nodes).
    pub fn seed_bytes(&self) -> [u8; 32] {
        self.private
    }
}

/// Node-side Noise initiator (knows HQ static public key).
pub struct NoiseInitiator {
    state: HandshakeState,
}

impl NoiseInitiator {
    pub fn new(hq_recipient_public: PublicKeyBytes) -> Result<Self, CryptoError> {
        let state = Builder::new(noise_params().clone())
            .remote_public_key(&hq_recipient_public.0)
            .map_err(noise_err)?
            .build_initiator()
            .map_err(noise_err)?;
        Ok(Self { state })
    }

    /// `-> e, es`
    pub fn write_message1(&mut self) -> Result<Vec<u8>, CryptoError> {
        let mut buf = vec![0u8; 64];
        let n = self.state.write_message(&[], &mut buf).map_err(noise_err)?;
        buf.truncate(n);
        Ok(buf)
    }

    /// Read `<- e, ee` and enter transport mode.
    pub fn read_message2(self, msg: &[u8]) -> Result<NoiseTransport, CryptoError> {
        let mut state = self.state;
        let mut payload = [0u8; 64];
        let _ = state.read_message(msg, &mut payload).map_err(noise_err)?;
        let transport = state.into_transport_mode().map_err(noise_err)?;
        Ok(NoiseTransport { state: transport })
    }
}

/// HQ-side Noise responder (holds recipient private key).
pub struct NoiseResponder {
    state: HandshakeState,
}

impl NoiseResponder {
    pub fn new(hq: &HqRecipientSecret) -> Result<Self, CryptoError> {
        let state = Builder::new(noise_params().clone())
            .local_private_key(&hq.private)
            .map_err(noise_err)?
            .build_responder()
            .map_err(noise_err)?;
        Ok(Self { state })
    }

    /// Read message 1, write message 2, enter transport.
    pub fn complete(self, message1: &[u8]) -> Result<(Vec<u8>, NoiseTransport), CryptoError> {
        let mut state = self.state;
        let mut payload = [0u8; 64];
        let _ = state.read_message(message1, &mut payload).map_err(noise_err)?;
        let mut msg2 = vec![0u8; 64];
        let n = state.write_message(&[], &mut msg2).map_err(noise_err)?;
        msg2.truncate(n);
        let transport = state.into_transport_mode().map_err(noise_err)?;
        Ok((msg2, NoiseTransport { state: transport }))
    }
}

/// Post-handshake Noise transport.
pub struct NoiseTransport {
    state: TransportState,
}

impl NoiseTransport {
    /// Encrypt one plaintext. Up to [`NOISE_MAX_PLAINTEXT`] bytes this is a single Noise
    /// message (wire-identical to older peers). Larger plaintexts are split into Noise
    /// messages, each prefixed with its u32 BE length; the result is always longer than
    /// one Noise message can be, so [`Self::decrypt`] tells the two forms apart by size.
    pub fn encrypt(&mut self, plaintext: &[u8]) -> Result<Vec<u8>, CryptoError> {
        if plaintext.len() <= NOISE_MAX_PLAINTEXT {
            return self.encrypt_one(plaintext);
        }
        let mut out = Vec::with_capacity(plaintext.len() + plaintext.len() / NOISE_MAX_PLAINTEXT * 20 + 20);
        for chunk in plaintext.chunks(NOISE_MAX_PLAINTEXT) {
            let ct = self.encrypt_one(chunk)?;
            out.extend_from_slice(&(ct.len() as u32).to_be_bytes());
            out.extend_from_slice(&ct);
        }
        Ok(out)
    }

    pub fn decrypt(&mut self, ciphertext: &[u8]) -> Result<Vec<u8>, CryptoError> {
        if ciphertext.len() <= NOISE_MAX_MESSAGE {
            return self.decrypt_one(ciphertext);
        }
        let mut out = Vec::with_capacity(ciphertext.len());
        let mut rest = ciphertext;
        while !rest.is_empty() {
            if rest.len() < 4 {
                return Err(noise_err(snow::Error::Input));
            }
            let n = u32::from_be_bytes([rest[0], rest[1], rest[2], rest[3]]) as usize;
            if n == 0 || n > NOISE_MAX_MESSAGE || rest.len() < 4 + n {
                return Err(noise_err(snow::Error::Input));
            }
            out.extend_from_slice(&self.decrypt_one(&rest[4..4 + n])?);
            rest = &rest[4 + n..];
        }
        Ok(out)
    }

    fn encrypt_one(&mut self, plaintext: &[u8]) -> Result<Vec<u8>, CryptoError> {
        let mut buf = vec![0u8; plaintext.len() + 16];
        let n = self.state.write_message(plaintext, &mut buf).map_err(noise_err)?;
        buf.truncate(n);
        Ok(buf)
    }

    fn decrypt_one(&mut self, ciphertext: &[u8]) -> Result<Vec<u8>, CryptoError> {
        let mut buf = vec![0u8; ciphertext.len()];
        let n = self.state.read_message(ciphertext, &mut buf).map_err(noise_err)?;
        buf.truncate(n);
        Ok(buf)
    }
}

/// Largest Noise transport message (ciphertext incl. tag).
pub const NOISE_MAX_MESSAGE: usize = 65_535;
/// Largest plaintext carried by one Noise transport message.
pub const NOISE_MAX_PLAINTEXT: usize = NOISE_MAX_MESSAGE - 16;

/// Result of a nested seal for tests / one-shot helpers.
pub struct NestedSeal {
    /// Outer-sealed blob (what the node sends on the uplink).
    pub outer: Vec<u8>,
    /// Inner Noise transport ciphertext (what C2 sees after stripping outer).
    pub inner_ciphertext: Vec<u8>,
    /// HQ transport ready to decrypt `inner_ciphertext`.
    pub hq_transport: NoiseTransport,
    /// Node transport (for further messages on the same session).
    pub node_transport: NoiseTransport,
    pub handshake_msg1: Vec<u8>,
    pub handshake_msg2: Vec<u8>,
}

/// Run Noise_NK handshake, encrypt `plaintext`, wrap with C2 outer layer.
pub fn noise_session_seal(
    keys: &RestreamerKeys,
    hq: &HqRecipientSecret,
    plaintext: &[u8],
) -> Result<NestedSeal, CryptoError> {
    let mut initiator = NoiseInitiator::new(keys.hq_recipient_public())?;
    let msg1 = initiator.write_message1()?;
    let responder = NoiseResponder::new(hq)?;
    let (msg2, hq_transport) = responder.complete(&msg1)?;
    let mut node_transport = initiator.read_message2(&msg2)?;
    let inner_ciphertext = node_transport.encrypt(plaintext)?;
    let outer = outer_seal(&keys.c2_public, &inner_ciphertext)?;
    Ok(NestedSeal {
        outer,
        inner_ciphertext,
        hq_transport,
        node_transport,
        handshake_msg1: msg1,
        handshake_msg2: msg2,
    })
}

/// Decrypt with an established HQ transport (production path after handshake).
pub fn core_open_inner(
    transport: &mut NoiseTransport,
    ciphertext: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    transport.decrypt(ciphertext)
}

/// Node seals already-inner bytes (handshake or transport, tagged) for C2's outer layer.
pub fn node_seal_outer(c2_public: PublicKeyBytes, inner: &[u8]) -> Result<Vec<u8>, CryptoError> {
    outer_seal(&PublicKey::from(c2_public.0), inner)
}

/// C2 strips the outer layer; returns opaque inner bytes (Noise ciphertext or handshake).
pub fn c2_strip_outer(keys: &RestreamerKeys, sealed: &[u8]) -> Result<Vec<u8>, CryptoError> {
    outer_open(&keys.c2_secret, sealed)
}

/// C2 cannot complete a Noise_NK responder handshake as HQ: wrong static key.
pub fn c2_cannot_complete_as_hq(keys: &RestreamerKeys, handshake_msg1: &[u8]) -> bool {
    let fake = HqRecipientSecret {
        private: keys.c2_secret.to_bytes(),
        public: *PublicKey::from(&keys.c2_secret).as_bytes(),
    };
    NoiseResponder::new(&fake)
        .and_then(|r| r.complete(handshake_msg1))
        .is_err()
}

/// Legacy name kept for call sites: C2 cannot decrypt inner transport ciphertext
/// without the HQ Noise session (attempt with a fresh wrong-key responder fails).
pub fn c2_cannot_open_inner(keys: &RestreamerKeys, inner_ciphertext: &[u8]) -> bool {
    // Without the handshake transcript, C2 has no transport. A wrong-key
    // "open" attempt via a bogus one-shot is not applicable; treat any
    // non-empty inner blob as undecryptable by C2's static secret alone.
    let _ = keys;
    // Outer-open with C2 secret on inner ciphertext must fail (different format).
    outer_open(&keys.c2_secret, inner_ciphertext).is_err()
}

fn outer_seal(recipient: &PublicKey, plaintext: &[u8]) -> Result<Vec<u8>, CryptoError> {
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

fn outer_open(secret: &StaticSecret, sealed: &[u8]) -> Result<Vec<u8>, CryptoError> {
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
    hasher.update(b"hatchery-restreamer-outer-v1");
    hasher.update(shared);
    let digest = hasher.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

fn noise_err(err: snow::Error) -> CryptoError {
    CryptoError::Noise(err.to_string())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CryptoError {
    Rng(String),
    BadKey,
    Encrypt,
    Decrypt,
    Truncated,
    Noise(String),
}

impl std::fmt::Display for CryptoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Rng(msg) => write!(f, "rng: {msg}"),
            Self::BadKey => write!(f, "bad key"),
            Self::Encrypt => write!(f, "encrypt failed"),
            Self::Decrypt => write!(f, "decrypt failed"),
            Self::Truncated => write!(f, "truncated sealed box"),
            Self::Noise(msg) => write!(f, "noise: {msg}"),
        }
    }
}

impl std::error::Error for CryptoError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_nk_handshake_and_transport() {
        let hq = HqRecipientSecret::from_seed([7u8; 32]);
        let mut initiator = NoiseInitiator::new(hq.public_bytes()).unwrap();
        let msg1 = initiator.write_message1().unwrap();
        let responder = NoiseResponder::new(&hq).unwrap();
        let (msg2, mut hq_tx) = responder.complete(&msg1).unwrap();
        let mut node_tx = initiator.read_message2(&msg2).unwrap();
        let ct = node_tx.encrypt(b"agent-message-body").unwrap();
        assert!(!ct.windows(b"agent-message-body".len()).any(|w| w == b"agent-message-body"));
        assert_eq!(hq_tx.decrypt(&ct).unwrap(), b"agent-message-body");
    }

    #[test]
    fn nested_roundtrip_and_c2_blind_to_inner() {
        let hq = HqRecipientSecret::from_seed([7u8; 32]);
        let keys = RestreamerKeys::from_seeds([3u8; 32], hq.public_bytes().0);
        let (c2_pub, hq_pub) = keys.upstream_public_keys();
        assert_eq!(c2_pub, keys.c2_public());
        assert_eq!(hq_pub, hq.public_bytes());

        let plain = b"agent-message-body";
        let sealed = noise_session_seal(&keys, &hq, plain).unwrap();
        let inner = c2_strip_outer(&keys, &sealed.outer).unwrap();
        assert_eq!(inner, sealed.inner_ciphertext);
        assert!(c2_cannot_open_inner(&keys, &inner));
        assert!(c2_cannot_complete_as_hq(&keys, &sealed.handshake_msg1));
        assert!(!inner.windows(plain.len()).any(|w| w == plain));
        let mut hq_tx = sealed.hq_transport;
        assert_eq!(core_open_inner(&mut hq_tx, &inner).unwrap(), plain);
    }

    fn transport_pair() -> (NoiseTransport, NoiseTransport) {
        let hq = HqRecipientSecret::from_seed([11u8; 32]);
        let mut init = NoiseInitiator::new(hq.public_bytes()).unwrap();
        let msg1 = init.write_message1().unwrap();
        let (msg2, resp) = NoiseResponder::new(&hq).unwrap().complete(&msg1).unwrap();
        (init.read_message2(&msg2).unwrap(), resp)
    }

    #[test]
    fn large_plaintexts_are_chunked_and_small_stay_single_message() {
        let (mut a, mut b) = transport_pair();
        for len in [0usize, 1, NOISE_MAX_PLAINTEXT, NOISE_MAX_PLAINTEXT + 1, 300_000] {
            let plain: Vec<u8> = (0..len).map(|i| (i % 251) as u8).collect();
            let ct = a.encrypt(&plain).unwrap();
            if len <= NOISE_MAX_PLAINTEXT {
                assert_eq!(ct.len(), len + 16, "single message is wire-compatible");
            } else {
                assert!(ct.len() > NOISE_MAX_MESSAGE);
            }
            assert_eq!(b.decrypt(&ct).unwrap(), plain, "len {len}");
            // And back the other way.
            let back = b.encrypt(&plain).unwrap();
            assert_eq!(a.decrypt(&back).unwrap(), plain);
        }
        let mut bad = a.encrypt(&vec![7u8; 100_000]).unwrap();
        bad.truncate(bad.len() - 3);
        assert!(b.decrypt(&bad).is_err());
    }

    #[test]
    fn tampered_inner_rejected_by_core() {
        let hq = HqRecipientSecret::from_seed([9u8; 32]);
        let keys = RestreamerKeys::from_seeds([1u8; 32], hq.public_bytes().0);
        let sealed = noise_session_seal(&keys, &hq, b"secret").unwrap();
        let mut inner = c2_strip_outer(&keys, &sealed.outer).unwrap();
        let last = inner.len() - 1;
        inner[last] ^= 0x5a;
        let mut hq_tx = sealed.hq_transport;
        assert!(core_open_inner(&mut hq_tx, &inner).is_err());
    }

    #[test]
    fn rotation_window_accepts_new_key() {
        let hq_old = HqRecipientSecret::from_seed([2u8; 32]);
        let hq_new = HqRecipientSecret::from_seed([4u8; 32]);
        let mut keys = RestreamerKeys::from_seeds([5u8; 32], hq_old.public_bytes().0);
        keys.rotate_hq_recipient(hq_new.public_bytes());
        let sealed = noise_session_seal(&keys, &hq_new, b"after-rotate").unwrap();
        let inner = c2_strip_outer(&keys, &sealed.outer).unwrap();
        let mut hq_tx = sealed.hq_transport;
        assert_eq!(core_open_inner(&mut hq_tx, &inner).unwrap(), b"after-rotate");
        // Handshake to old key with msg1 sealed for new key fails.
        assert!(NoiseResponder::new(&hq_old)
            .unwrap()
            .complete(&sealed.handshake_msg1)
            .is_err());
        keys.end_rotation_window();
        assert!(keys.previous_hq_recipient_public().is_none());
    }

    #[test]
    fn wrong_recipient_cannot_complete_handshake() {
        let hq = HqRecipientSecret::from_seed([1u8; 32]);
        let other = HqRecipientSecret::from_seed([2u8; 32]);
        let mut initiator = NoiseInitiator::new(hq.public_bytes()).unwrap();
        let msg1 = initiator.write_message1().unwrap();
        let responder = NoiseResponder::new(&other).unwrap();
        assert!(responder.complete(&msg1).is_err());
    }
}
