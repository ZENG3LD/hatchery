//! m4a signer (hq-core), option A of `specs/m4a-session-keys-2026-10-16.md`.
//!
//! - Every session identity (nick) has an ed25519 key that is generated here and **never
//!   leaves**: there is no getter, no export, no serialization outside the sealed store.
//! - The store is sealed (XChaCha20-Poly1305) under a key derived from the **human key**.
//!   Locked signer = no provision, no sign.
//! - Sessions hold a Biscuit capability (`session`, `nick`, `right(op, nick)`), issued by the
//!   signer's root key. The node proxy keeps it in its own memory; the PTY only gets a
//!   process-bound handle.
//! - Child sessions: only the signer issues them, as a **new** identity with rights ⊆ the
//!   parent's, and only if the parent holds `spawn_child`. The parent never sees the child's
//!   key or token.
//! - Granting `spawn_child` or retiring keys requires the human key per operation.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use biscuit_auth::macros::{authorizer, biscuit};
use biscuit_auth::{Biscuit, KeyPair, PrivateKey};
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use ed25519_dalek::{Signer as _, SigningKey, VerifyingKey};
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, Zeroizing};

/// Salt the browser passes to the passkey PRF extension (`prf.eval.first`).
pub const PRF_SALT: &[u8] = b"hatchery/m4a-account-root/v1";

pub mod keyring;

pub const RIGHT_SEND: &str = "send_as";
pub const RIGHT_RECV: &str = "recv_inbox";
pub const RIGHT_RUN_CLOSE: &str = "run_close";
pub const RIGHT_SPAWN_CHILD: &str = "spawn_child";

/// Rights a fresh session gets without any human approval.
pub fn default_rights() -> BTreeSet<String> {
    [RIGHT_SEND, RIGHT_RECV, RIGHT_RUN_CLOSE].into_iter().map(str::to_owned).collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignerError {
    Locked,
    WrongHumanKey,
    Store(String),
    UnknownSession,
    Retired,
    Denied(String),
    HumanApprovalRequired,
    BadToken,
}

impl std::fmt::Display for SignerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for SignerError {}

/// The operator's key (32 bytes, e.g. from a passkey PRF or an age identity). Zeroized on drop.
pub struct HumanKey(Zeroizing<[u8; 32]>);

impl HumanKey {
    pub fn from_bytes(b: [u8; 32]) -> Self {
        Self(Zeroizing::new(b))
    }
    /// From the WebAuthn PRF output of the human's account passkey (evaluated by the
    /// authenticator in the browser with salt [`PRF_SALT`]). The signer never sees the passkey,
    /// only this derived secret, and only for the unlock call.
    pub fn from_passkey_prf(prf_output: &[u8]) -> Self {
        let hk = hkdf::Hkdf::<sha2::Sha256>::new(Some(b"hatchery-passkey-prf/v1"), prf_output);
        let mut out = Zeroizing::new([0u8; 32]);
        hk.expand(b"account-root", &mut *out).expect("hkdf length");
        Self(out)
    }
    fn derive(&self, label: &[u8]) -> Zeroizing<[u8; 32]> {
        let hk = hkdf::Hkdf::<sha2::Sha256>::new(Some(b"hatchery-m4a-signer/v1"), &*self.0);
        let mut out = Zeroizing::new([0u8; 32]);
        hk.expand(label, &mut *out).expect("hkdf length");
        out
    }
    fn kek(&self) -> Zeroizing<[u8; 32]> {
        let hk = hkdf::Hkdf::<sha2::Sha256>::new(Some(b"hatchery-m4a-signer/v1"), &*self.0);
        let mut out = Zeroizing::new([0u8; 32]);
        hk.expand(b"store-kek", &mut *out).expect("hkdf length");
        out
    }
    fn approval_tag(&self) -> String {
        let hk = hkdf::Hkdf::<sha2::Sha256>::new(Some(b"hatchery-m4a-signer/v1"), &*self.0);
        let mut out = [0u8; 32];
        hk.expand(b"approval", &mut out).expect("hkdf length");
        hex::encode(out)
    }
}

#[derive(Serialize, Deserialize)]
struct Identity {
    nick: String,
    node: String,
    provider: String,
    secret: [u8; 32],
    rights: BTreeSet<String>,
    parent: Option<String>,
    retired: bool,
    /// Revocation id of the only valid token (respawn rotates it; stop clears it).
    current: Option<String>,
}

impl Drop for Identity {
    fn drop(&mut self) {
        self.secret.zeroize();
    }
}

#[derive(Serialize, Deserialize, Default)]
struct Store {
    #[serde(default)]
    account: String,
    #[serde(default)]
    nodes: BTreeSet<String>,
    root: Option<Vec<u8>>,
    approval_tag: String,
    /// session id -> identity
    sessions: BTreeMap<String, Identity>,
    seq: u64,
}

impl Drop for Store {
    fn drop(&mut self) {
        if let Some(r) = self.root.as_mut() {
            r.zeroize();
        }
    }
}

/// What the node receives at provision. `token` is held by the node proxy, never put in PTY.
#[derive(Debug, Clone)]
pub struct Provisioned {
    pub nick: String,
    pub session: String,
    pub token: String,
    pub public_key: [u8; 32],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signature {
    pub nick: String,
    pub public_key: [u8; 32],
    pub signature: [u8; 64],
}

pub struct Signer {
    path: PathBuf,
    unlocked: Option<(Store, Zeroizing<[u8; 32]>)>,
    /// Account root of the key tree (account -> node -> session), in memory while unlocked.
    tree: Option<Zeroizing<[u8; 32]>>,
}

impl Signer {
    /// Create a new sealed store at `path` (fails if one exists).
    pub fn init(path: impl Into<PathBuf>, human: &HumanKey) -> Result<Self, SignerError> {
        let path = path.into();
        if path.exists() {
            return Err(SignerError::Store("store already exists".into()));
        }
        let root = KeyPair::new();
        let store = Store {
            account: String::new(),
            nodes: BTreeSet::new(),
            root: Some(root.private().to_bytes().to_vec()),
            approval_tag: human.approval_tag(),
            sessions: BTreeMap::new(),
            seq: 0,
        };
        let mut s = Self { path, unlocked: Some((store, human.kek())), tree: Some(human.derive(b"tree-root")) };
        s.persist()?;
        Ok(s)
    }

    /// Open a sealed store; it stays locked until [`Signer::unlock`].
    pub fn open(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into(), unlocked: None, tree: None }
    }

    pub fn is_unlocked(&self) -> bool {
        self.unlocked.is_some()
    }

    pub fn unlock(&mut self, human: &HumanKey) -> Result<(), SignerError> {
        let kek = human.kek();
        let raw = std::fs::read(&self.path).map_err(|e| SignerError::Store(e.kind().to_string()))?;
        if raw.len() < 24 {
            return Err(SignerError::Store("truncated".into()));
        }
        let (nonce, ct) = raw.split_at(24);
        let pt = XChaCha20Poly1305::new((&*kek).into())
            .decrypt(XNonce::from_slice(nonce), ct)
            .map_err(|_| SignerError::WrongHumanKey)?;
        let pt = Zeroizing::new(pt);
        let store: Store = serde_json::from_slice(&pt).map_err(|_| SignerError::Store("corrupt".into()))?;
        self.unlocked = Some((store, kek));
        self.tree = Some(human.derive(b"tree-root"));
        Ok(())
    }

    pub fn lock(&mut self) {
        self.unlocked = None;
        self.tree = None;
    }

    /// Bind this store to an account id (once, at enrollment).
    pub fn set_account(&mut self, account: &str) -> Result<(), SignerError> {
        let (store, _) = self.unlocked.as_mut().ok_or(SignerError::Locked)?;
        if !store.account.is_empty() && store.account != account {
            return Err(SignerError::Denied("store belongs to another account".into()));
        }
        store.account = account.to_owned();
        self.persist()
    }

    pub fn account(&self) -> Result<String, SignerError> {
        Ok(self.unlocked.as_ref().ok_or(SignerError::Locked)?.0.account.clone())
    }

    /// Add a node branch under this account (human key per operation).
    pub fn register_node(&mut self, node: &str, human: &HumanKey) -> Result<(), SignerError> {
        self.check_approval(Some(human))?;
        let (store, _) = self.unlocked.as_mut().ok_or(SignerError::Locked)?;
        store.nodes.insert(node.to_owned());
        self.persist()
    }

    fn require_node(&self, node: &str) -> Result<(), SignerError> {
        let (store, _) = self.unlocked.as_ref().ok_or(SignerError::Locked)?;
        if store.nodes.contains(node) {
            Ok(())
        } else {
            Err(SignerError::Denied(format!("node {node} is not in this account's tree")))
        }
    }

    /// Session key = HKDF(HKDF(account root, node), session): the tree account -> node -> session.
    fn session_seed(&self, node: &str, session: &str) -> Result<[u8; 32], SignerError> {
        let tree = self.tree.as_ref().ok_or(SignerError::Locked)?;
        let node_key = {
            let hk = hkdf::Hkdf::<sha2::Sha256>::new(Some(b"node"), &**tree);
            let mut k = Zeroizing::new([0u8; 32]);
            hk.expand(node.as_bytes(), &mut *k).expect("hkdf");
            k
        };
        let hk = hkdf::Hkdf::<sha2::Sha256>::new(Some(b"session"), &*node_key);
        let mut out = [0u8; 32];
        hk.expand(session.as_bytes(), &mut out).expect("hkdf");
        Ok(out)
    }

    fn persist(&mut self) -> Result<(), SignerError> {
        let (store, kek) = self.unlocked.as_ref().ok_or(SignerError::Locked)?;
        let pt = Zeroizing::new(serde_json::to_vec(store).map_err(|e| SignerError::Store(e.to_string()))?);
        let mut nonce = [0u8; 24];
        OsRng.fill_bytes(&mut nonce);
        let ct = XChaCha20Poly1305::new((&**kek).into())
            .encrypt(XNonce::from_slice(&nonce), pt.as_slice())
            .map_err(|_| SignerError::Store("seal".into()))?;
        let tmp = self.path.with_extension("tmp");
        let mut out = nonce.to_vec();
        out.extend_from_slice(&ct);
        write_private(&tmp, &out)?;
        std::fs::rename(&tmp, &self.path).map_err(|e| SignerError::Store(e.kind().to_string()))
    }

    fn root(&self) -> Result<KeyPair, SignerError> {
        let (store, _) = self.unlocked.as_ref().ok_or(SignerError::Locked)?;
        let bytes = store.root.as_ref().ok_or(SignerError::Locked)?;
        let pk = PrivateKey::from_bytes(bytes, biscuit_auth::Algorithm::Ed25519).map_err(|_| SignerError::Store("root key".into()))?;
        Ok(KeyPair::from(&pk))
    }

    fn check_approval(&self, human: Option<&HumanKey>) -> Result<(), SignerError> {
        let (store, _) = self.unlocked.as_ref().ok_or(SignerError::Locked)?;
        match human {
            Some(h) if h.approval_tag() == store.approval_tag => Ok(()),
            Some(_) => Err(SignerError::WrongHumanKey),
            None => Err(SignerError::HumanApprovalRequired),
        }
    }

    /// New identity for a root session (spawned by the operator / router, not by an agent).
    /// `spawn_child` in `rights` needs the human key.
    pub fn provision(
        &mut self,
        node: &str,
        provider: &str,
        session: &str,
        rights: BTreeSet<String>,
        human: Option<&HumanKey>,
    ) -> Result<Provisioned, SignerError> {
        if rights.contains(RIGHT_SPAWN_CHILD) {
            self.check_approval(human)?;
        }
        self.issue(node, provider, session, rights, None)
    }

    /// Child session requested by `parent_token`'s holder. Issues a NEW identity; rights must
    /// be a subset of the parent's and the parent must hold `spawn_child`. The result goes to
    /// the node proxy, never back to the parent session.
    pub fn provision_child(
        &mut self,
        parent_token: &str,
        node: &str,
        provider: &str,
        child_session: &str,
        rights: BTreeSet<String>,
    ) -> Result<Provisioned, SignerError> {
        let (parent_session, parent_nick) = self.verify(parent_token, RIGHT_SPAWN_CHILD)?;
        let parent_rights = {
            let (store, _) = self.unlocked.as_ref().ok_or(SignerError::Locked)?;
            store.sessions.get(&parent_session).ok_or(SignerError::UnknownSession)?.rights.clone()
        };
        if !rights.is_subset(&parent_rights) {
            return Err(SignerError::Denied("child rights exceed parent".into()));
        }
        let _ = parent_nick;
        self.issue(node, provider, child_session, rights, Some(parent_session))
    }

    fn issue(
        &mut self,
        node: &str,
        provider: &str,
        session: &str,
        rights: BTreeSet<String>,
        parent: Option<String>,
    ) -> Result<Provisioned, SignerError> {
        self.require_node(node)?;
        let seed = self.session_seed(node, session)?;
        let root = self.root()?;
        let (store, _) = self.unlocked.as_mut().ok_or(SignerError::Locked)?;
        let account = store.account.clone();
        // Respawn of the same session: keep identity, rotate token (old one revoked).
        let ident = match store.sessions.get_mut(session) {
            Some(id) if id.retired => return Err(SignerError::Retired),
            Some(id) => id,
            None => {
                store.seq += 1;
                let secret = seed;
                let nick = format!("{}-{}", short(provider), store.seq);
                store.sessions.insert(
                    session.to_owned(),
                    Identity {
                        nick,
                        node: node.to_owned(),
                        provider: provider.to_owned(),
                        secret,
                        rights: BTreeSet::new(),
                        parent: parent.clone(),
                        retired: false,
                        current: None,
                    },
                );
                store.sessions.get_mut(session).expect("inserted")
            }
        };
        ident.rights = rights.clone();
        let nick = ident.nick.clone();
        let mut b = biscuit!(
            r#"account({account}); session({session}); nick({nick}); node({node});"#,
            account = account.as_str(),
            session = session,
            nick = nick.as_str(),
            node = node
        );
        for r in &rights {
            b = b.fact(biscuit_auth::macros::fact!(r#"right({r}, {nick})"#, r = r.as_str(), nick = nick.as_str())).map_err(|_| SignerError::BadToken)?;
        }
        let token = b.build(&root).map_err(|_| SignerError::BadToken)?;
        let rev = hex::encode(token.revocation_identifiers().first().ok_or(SignerError::BadToken)?);
        let token = token.to_base64().map_err(|_| SignerError::BadToken)?;
        let public_key = SigningKey::from_bytes(&ident.secret).verifying_key().to_bytes();
        // Only the newest token works: the respawned (or stolen older) handle dies.
        ident.current = Some(rev);
        self.persist()?;
        Ok(Provisioned { nick, session: session.to_owned(), token, public_key })
    }

    /// Verify `token` grants `op` on its own nick and is the session's current token.
    fn verify(&self, token: &str, op: &str) -> Result<(String, String), SignerError> {
        let root = self.root()?;
        let (store, _) = self.unlocked.as_ref().ok_or(SignerError::Locked)?;
        let t = Biscuit::from_base64(token, root.public()).map_err(|_| SignerError::BadToken)?;
        let rev = hex::encode(t.revocation_identifiers().first().ok_or(SignerError::BadToken)?);
        let mut auth = authorizer!(r#"data($s, $n) <- session($s), nick($n);"#).build(&t).map_err(|_| SignerError::BadToken)?;
        let rows: Vec<(String, String)> = auth.query("data($s, $n) <- session($s), nick($n)").map_err(|_| SignerError::BadToken)?;
        let (session, nick) = rows.into_iter().next().ok_or(SignerError::BadToken)?;
        let ident = store.sessions.get(&session).ok_or(SignerError::UnknownSession)?;
        if ident.retired {
            return Err(SignerError::Retired);
        }
        if ident.nick != nick || ident.current.as_deref() != Some(rev.as_str()) {
            return Err(SignerError::Denied("token is not the session's current capability".into()));
        }
        let mut az = authorizer!(r#"allow if right({op}, {nick});"#, op = op, nick = nick.as_str())
            .build(&t)
            .map_err(|_| SignerError::BadToken)?;
        az.authorize().map_err(|_| SignerError::Denied(format!("{op} not granted")))?;
        Ok((session, nick))
    }

    /// Sign `payload` as the token's own nick. There is no way to name another nick: the
    /// identity is taken from the capability, not from the caller.
    pub fn sign(&self, token: &str, op: &str, payload: &[u8]) -> Result<Signature, SignerError> {
        let (session, nick) = self.verify(token, op)?;
        let (store, _) = self.unlocked.as_ref().ok_or(SignerError::Locked)?;
        let ident = store.sessions.get(&session).ok_or(SignerError::UnknownSession)?;
        let key = SigningKey::from_bytes(&ident.secret);
        let msg = domain_message(&nick, op, payload);
        Ok(Signature { nick, public_key: key.verifying_key().to_bytes(), signature: key.sign(&msg).to_bytes() })
    }

    pub fn has_node(&self, node: &str) -> bool {
        self.unlocked.as_ref().is_some_and(|(s, _)| s.nodes.contains(node))
    }

    /// [`Signer::sign`], and the session must live on `node` (the node the call came from,
    /// as authenticated by the transport): a node cannot use another node's capability.
    pub fn sign_from_node(&self, node: &str, token: &str, op: &str, payload: &[u8]) -> Result<Signature, SignerError> {
        let (session, _) = self.verify(token, op)?;
        self.session_on_node(&session, node)?;
        self.sign(token, op, payload)
    }

    pub fn session_on_node(&self, session: &str, node: &str) -> Result<(), SignerError> {
        let (store, _) = self.unlocked.as_ref().ok_or(SignerError::Locked)?;
        let i = store.sessions.get(session).ok_or(SignerError::UnknownSession)?;
        if i.node == node { Ok(()) } else { Err(SignerError::Denied("session is on another node".into())) }
    }

    /// Session stopped: its capability stops working (identity kept for history).
    pub fn revoke_session(&mut self, session: &str) -> Result<(), SignerError> {
        let (store, _) = self.unlocked.as_mut().ok_or(SignerError::Locked)?;
        let ident = store.sessions.get_mut(session).ok_or(SignerError::UnknownSession)?;
        ident.current = None;
        self.persist()
    }

    /// Retire an identity for good (human key required).
    pub fn retire(&mut self, session: &str, human: &HumanKey) -> Result<(), SignerError> {
        self.check_approval(Some(human))?;
        let (store, _) = self.unlocked.as_mut().ok_or(SignerError::Locked)?;
        let ident = store.sessions.get_mut(session).ok_or(SignerError::UnknownSession)?;
        ident.retired = true;
        ident.current = None;
        self.persist()
    }

    pub fn public_key_of(&self, session: &str) -> Result<[u8; 32], SignerError> {
        let (store, _) = self.unlocked.as_ref().ok_or(SignerError::Locked)?;
        let i = store.sessions.get(session).ok_or(SignerError::UnknownSession)?;
        Ok(SigningKey::from_bytes(&i.secret).verifying_key().to_bytes())
    }

    pub fn parent_of(&self, session: &str) -> Result<Option<String>, SignerError> {
        let (store, _) = self.unlocked.as_ref().ok_or(SignerError::Locked)?;
        Ok(store.sessions.get(session).ok_or(SignerError::UnknownSession)?.parent.clone())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// What a signature covers: domain, nick, operation, payload.
pub fn domain_message(nick: &str, op: &str, payload: &[u8]) -> Vec<u8> {
    let mut m = b"hatchery-m4a/v1\0".to_vec();
    m.extend_from_slice(nick.as_bytes());
    m.push(0);
    m.extend_from_slice(op.as_bytes());
    m.push(0);
    m.extend_from_slice(payload);
    m
}

pub fn verify_signature(sig: &Signature, op: &str, payload: &[u8]) -> bool {
    let Ok(vk) = VerifyingKey::from_bytes(&sig.public_key) else { return false };
    vk.verify_strict(&domain_message(&sig.nick, op, payload), &ed25519_dalek::Signature::from_bytes(&sig.signature)).is_ok()
}

fn short(provider: &str) -> &str {
    match provider {
        "claude-code" => "claude",
        "kimi-code" => "kimi",
        p => p,
    }
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<(), SignerError> {
    use std::io::Write;
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    o.open(path).and_then(|mut f| f.write_all(bytes)).map_err(|e| SignerError::Store(e.kind().to_string()))
}
