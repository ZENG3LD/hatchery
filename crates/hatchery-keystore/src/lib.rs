//! Hatchery keystore: the hq-core authority of the key tree (spec
//! `specs/keystore-placement-2026-10-16.md`).
//!
//! - The crypto (derivation, sealing, non-exportable signing, capabilities) is
//!   [`tesserax_keytree`]. This crate holds hatchery's policy only: accounts, nodes, sessions,
//!   grants per transport, human approval, and delegation of attenuated capabilities to
//!   consumers (m4a, g4a) that run inside hatchery.
//! - Account root = the human's passkey PRF. Another human's passkey opens only their branch.
//! - Session keys are generated here and never leave: no getter, no export.
//! - Child sessions: a NEW identity with grants ⊆ the parent's, only with `spawn_child`.
//!
//! Compatibility with 0.2.15 (`hatchery-m4a-signer`): the salts, labels, sealed layout and the
//! signed message domain are byte-identical ([`legacy_v1`]); the old right names are accepted
//! as aliases of the namespaced grants.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tesserax_keytree as kt;
use zeroize::Zeroize;

pub mod keyring;

/// Byte-identical constants of the 0.2.15 store. Changing any of them makes every sealed
/// store and every browser PRF evaluation unreadable. A v2 needs an explicit re-seal.
pub mod legacy_v1 {
    /// Salt the browser passes to the passkey PRF extension (`prf.eval.first`).
    pub const PRF_SALT: &[u8] = b"hatchery/m4a-account-root/v1";
    pub const PRF_HKDF_SALT: &[u8] = b"hatchery-passkey-prf/v1";
    pub const PRF_HKDF_INFO: &[u8] = b"account-root";
    pub const STORE_SALT: &[u8] = b"hatchery-m4a-signer/v1";
    pub const LABEL_KEK: &[u8] = b"store-kek";
    pub const LABEL_APPROVAL: &[u8] = b"approval";
    pub const LABEL_TREE: &[u8] = b"tree-root";
    pub const SALT_NODE: &[u8] = b"node";
    pub const SALT_SESSION: &[u8] = b"session";
    /// Domain of signed messages (`domain\0nick\0op\0payload`).
    pub const SIGN_DOMAIN: &[u8] = b"hatchery-m4a/v1";
}

pub use legacy_v1::PRF_SALT;

/// Grants, namespaced by the transport they unlock.
pub mod grants {
    //! Names come from the consumers (`m4a-keys`, `gate4agent-keys`); hatchery is the authority.
    pub const M4A_SEND: &str = m4a_keys::grants::SEND;
    pub const M4A_RECV: &str = m4a_keys::grants::RECV;
    pub const SESSION_RUN_CLOSE: &str = gatekeys::grants::SESSION_RUN_CLOSE;
    pub const SESSION_SPAWN_CHILD: &str = gatekeys::grants::SESSION_SPAWN_CHILD;
    pub const NODE_UPLINK: &str = gatekeys::grants::NODE_UPLINK;
    pub const NODE_ENV_CTL: &str = gatekeys::grants::NODE_ENV_CTL;
    pub const NODE_PTY_ATTACH: &str = gatekeys::grants::NODE_PTY_ATTACH;

    /// 0.2.15 right name -> grant.
    pub const LEGACY: &[(&str, &str)] = &[
        ("send_as", M4A_SEND),
        ("recv_inbox", M4A_RECV),
        ("run_close", SESSION_RUN_CLOSE),
        ("spawn_child", SESSION_SPAWN_CHILD),
    ];

    /// Canonical grant for a grant or legacy right name.
    pub fn canonical(name: &str) -> &str {
        LEGACY.iter().find(|(l, _)| *l == name).map(|(_, g)| *g).unwrap_or(name)
    }

    /// Legacy right name of a grant, if it had one.
    pub fn legacy(grant: &str) -> Option<&'static str> {
        LEGACY.iter().find(|(_, g)| *g == grant).map(|(l, _)| *l)
    }

    /// Transport a grant unlocks (its namespace).
    pub fn transport(grant: &str) -> &str {
        canonical(grant).split('.').next().unwrap_or("")
    }

    /// Grants that need the human key per operation.
    pub fn needs_human(grant: &str) -> bool {
        matches!(canonical(grant), SESSION_SPAWN_CHILD | NODE_ENV_CTL)
    }
}

/// Deprecated 0.2.15 names; use [`grants`].
pub const RIGHT_SEND: &str = "send_as";
pub const RIGHT_RECV: &str = "recv_inbox";
pub const RIGHT_RUN_CLOSE: &str = "run_close";
pub const RIGHT_SPAWN_CHILD: &str = "spawn_child";

/// Grants a fresh session gets without any human approval.
pub fn default_rights() -> BTreeSet<String> {
    [grants::M4A_SEND, grants::M4A_RECV, grants::SESSION_RUN_CLOSE].into_iter().map(str::to_owned).collect()
}

fn canon_set(r: &BTreeSet<String>) -> BTreeSet<String> {
    r.iter().map(|g| grants::canonical(g).to_owned()).collect()
}

/// Rights written into a token: each grant plus its legacy alias (old verifiers keep working).
fn token_rights(r: &BTreeSet<String>) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for g in canon_set(r) {
        if let Some(l) = grants::legacy(&g) {
            out.insert(l.to_owned());
        }
        out.insert(g);
    }
    out
}

/// Op string inside signed messages: the legacy name where one exists (wire-compatible).
fn wire_op(op: &str) -> &str {
    let g = grants::canonical(op);
    grants::legacy(g).unwrap_or(g)
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
pub type KeystoreError = SignerError;

impl std::fmt::Display for SignerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for SignerError {}

fn from_kt(e: kt::Error) -> SignerError {
    match e {
        kt::Error::WrongKey => SignerError::WrongHumanKey,
        kt::Error::Truncated => SignerError::Store("truncated".into()),
        kt::Error::Io(k) => SignerError::Store(k),
        kt::Error::Seal => SignerError::Store("seal".into()),
        kt::Error::BadId => SignerError::Denied("bad id".into()),
        kt::Error::BadToken => SignerError::BadToken,
        kt::Error::Denied => SignerError::Denied("not granted".into()),
    }
}

/// The human's account root (from the passkey PRF). Zeroized on drop.
pub struct HumanKey(kt::Secret);

impl HumanKey {
    pub fn from_bytes(b: [u8; 32]) -> Self {
        Self(kt::Secret::from_bytes(b))
    }
    /// From the WebAuthn PRF output of the account passkey (salt [`PRF_SALT`] in the browser).
    pub fn from_passkey_prf(prf_output: &[u8]) -> Self {
        Self(kt::Secret::from_prf(prf_output, legacy_v1::PRF_HKDF_SALT, legacy_v1::PRF_HKDF_INFO))
    }
    fn derive(&self, label: &[u8]) -> kt::Secret {
        self.0.derive(legacy_v1::STORE_SALT, label)
    }
    fn kek(&self) -> kt::Secret {
        self.derive(legacy_v1::LABEL_KEK)
    }
    fn approval_tag(&self) -> String {
        self.0.tag(legacy_v1::STORE_SALT, legacy_v1::LABEL_APPROVAL)
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

/// One account's branch of the keystore (kept as `Signer` for 0.2.15 callers).
pub struct Signer {
    path: PathBuf,
    unlocked: Option<(Store, kt::Secret)>,
    tree: Option<kt::Secret>,
}
pub type Keystore = Signer;

impl Signer {
    pub fn init(path: impl Into<PathBuf>, human: &HumanKey) -> Result<Self, SignerError> {
        let path = path.into();
        if path.exists() {
            return Err(SignerError::Store("store already exists".into()));
        }
        let root = kt::Authority::generate();
        let store = Store {
            account: String::new(),
            nodes: BTreeSet::new(),
            root: Some(root.private_bytes().to_vec()),
            approval_tag: human.approval_tag(),
            sessions: BTreeMap::new(),
            seq: 0,
        };
        let mut s = Self { path, unlocked: Some((store, human.kek())), tree: Some(human.derive(legacy_v1::LABEL_TREE)) };
        s.persist()?;
        Ok(s)
    }

    pub fn open(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into(), unlocked: None, tree: None }
    }

    pub fn is_unlocked(&self) -> bool {
        self.unlocked.is_some()
    }

    pub fn unlock(&mut self, human: &HumanKey) -> Result<(), SignerError> {
        let kek = human.kek();
        let pt = kt::open_file(&self.path, &kek).map_err(from_kt)?;
        let store: Store = serde_json::from_slice(&pt).map_err(|_| SignerError::Store("corrupt".into()))?;
        self.unlocked = Some((store, kek));
        self.tree = Some(human.derive(legacy_v1::LABEL_TREE));
        Ok(())
    }

    pub fn lock(&mut self) {
        self.unlocked = None;
        self.tree = None;
    }

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

    /// Session seed = tree / node / session.
    fn session_seed(&self, node: &str, session: &str) -> Result<[u8; 32], SignerError> {
        let tree = self.tree.as_ref().ok_or(SignerError::Locked)?;
        let s = tree.derive_path(&[(legacy_v1::SALT_NODE, node.as_bytes()), (legacy_v1::SALT_SESSION, session.as_bytes())]);
        Ok(*s.expose_secret())
    }

    fn persist(&mut self) -> Result<(), SignerError> {
        let (store, kek) = self.unlocked.as_ref().ok_or(SignerError::Locked)?;
        let pt = zeroize::Zeroizing::new(serde_json::to_vec(store).map_err(|e| SignerError::Store(e.to_string()))?);
        kt::seal_file(&self.path, kek, &pt).map_err(from_kt)
    }

    fn root(&self) -> Result<kt::Authority, SignerError> {
        let (store, _) = self.unlocked.as_ref().ok_or(SignerError::Locked)?;
        let bytes = store.root.as_ref().ok_or(SignerError::Locked)?;
        kt::Authority::from_private_bytes(bytes).map_err(|_| SignerError::Store("root key".into()))
    }

    fn check_approval(&self, human: Option<&HumanKey>) -> Result<(), SignerError> {
        let (store, _) = self.unlocked.as_ref().ok_or(SignerError::Locked)?;
        match human {
            Some(h) if h.approval_tag() == store.approval_tag => Ok(()),
            Some(_) => Err(SignerError::WrongHumanKey),
            None => Err(SignerError::HumanApprovalRequired),
        }
    }

    /// New identity for a root session. Grants that need a human require the human key.
    pub fn provision(
        &mut self,
        node: &str,
        provider: &str,
        session: &str,
        rights: BTreeSet<String>,
        human: Option<&HumanKey>,
    ) -> Result<Provisioned, SignerError> {
        let rights = canon_set(&rights);
        if rights.iter().any(|g| grants::needs_human(g)) {
            self.check_approval(human)?;
        }
        self.issue(node, provider, session, rights, None)
    }

    /// Child session: NEW identity, grants ⊆ parent's, parent must hold `spawn_child`.
    pub fn provision_child(
        &mut self,
        parent_token: &str,
        node: &str,
        provider: &str,
        child_session: &str,
        rights: BTreeSet<String>,
    ) -> Result<Provisioned, SignerError> {
        let rights = canon_set(&rights);
        let (parent_session, _) = self.verify(parent_token, grants::SESSION_SPAWN_CHILD)?;
        let parent_rights = {
            let (store, _) = self.unlocked.as_ref().ok_or(SignerError::Locked)?;
            canon_set(&store.sessions.get(&parent_session).ok_or(SignerError::UnknownSession)?.rights)
        };
        if !rights.is_subset(&parent_rights) {
            return Err(SignerError::Denied("child rights exceed parent".into()));
        }
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
        let ident = match store.sessions.get_mut(session) {
            Some(id) if id.retired => return Err(SignerError::Retired),
            Some(id) => id,
            None => {
                store.seq += 1;
                let nick = format!("{}-{}", short(provider), store.seq);
                store.sessions.insert(
                    session.to_owned(),
                    Identity {
                        nick,
                        node: node.to_owned(),
                        provider: provider.to_owned(),
                        secret: seed,
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
        let issued = root
            .issue(&[("account", &account), ("session", session), ("nick", &nick), ("node", node)], &nick, &token_rights(&rights))
            .map_err(from_kt)?;
        let public_key = kt::KeyHandle::from_seed(&ident.secret).public_key();
        ident.current = Some(issued.revocation_id);
        self.persist()?;
        Ok(Provisioned { nick, session: session.to_owned(), token: issued.token, public_key })
    }

    /// Verify `token` grants `op` (grant or legacy name) on its own nick and is current.
    fn verify(&self, token: &str, op: &str) -> Result<(String, String), SignerError> {
        let root = self.root()?;
        let g = grants::canonical(op);
        let mut res = root.verify(token, g, "nick", &["session"]);
        if matches!(res, Err(kt::Error::Denied)) {
            if let Some(l) = grants::legacy(g) {
                res = root.verify(token, l, "nick", &["session"]);
            }
        }
        let v = res.map_err(|e| match e {
            kt::Error::Denied => SignerError::Denied(format!("{op} not granted")),
            _ => SignerError::BadToken,
        })?;
        let session = v.facts.get("session").cloned().ok_or(SignerError::BadToken)?;
        let nick = v.facts.get("nick").cloned().ok_or(SignerError::BadToken)?;
        let (store, _) = self.unlocked.as_ref().ok_or(SignerError::Locked)?;
        let ident = store.sessions.get(&session).ok_or(SignerError::UnknownSession)?;
        if ident.retired {
            return Err(SignerError::Retired);
        }
        if ident.nick != nick || ident.current.as_deref() != Some(v.revocation_id.as_str()) {
            return Err(SignerError::Denied("token is not the session's current capability".into()));
        }
        Ok((session, nick))
    }

    /// Sign `payload` as the token's own nick.
    pub fn sign(&self, token: &str, op: &str, payload: &[u8]) -> Result<Signature, SignerError> {
        let (session, nick) = self.verify(token, op)?;
        let (store, _) = self.unlocked.as_ref().ok_or(SignerError::Locked)?;
        let ident = store.sessions.get(&session).ok_or(SignerError::UnknownSession)?;
        let key = kt::KeyHandle::from_seed(&ident.secret);
        let msg = domain_message(&nick, op, payload);
        Ok(Signature { nick, public_key: key.public_key(), signature: key.sign(&msg) })
    }

    /// Attenuated copy of a session token for a consumer running inside hatchery: only the
    /// grants of `transport` (e.g. "m4a") that the token already holds. The consumer can verify
    /// and use it but cannot widen it or mint siblings.
    pub fn delegate(&self, token: &str, transport: &str) -> Result<String, SignerError> {
        let (session, _) = self.verify_any(token)?;
        let (store, _) = self.unlocked.as_ref().ok_or(SignerError::Locked)?;
        let held = canon_set(&store.sessions.get(&session).ok_or(SignerError::UnknownSession)?.rights);
        let ops: BTreeSet<String> = token_rights(&held.into_iter().filter(|g| grants::transport(g) == transport).collect());
        if ops.is_empty() {
            return Err(SignerError::Denied(format!("no {transport} grants")));
        }
        kt::attenuate(token, &self.root()?.public_key(), &ops).map_err(from_kt)
    }

    fn verify_any(&self, token: &str) -> Result<(String, String), SignerError> {
        let (store, _) = self.unlocked.as_ref().ok_or(SignerError::Locked)?;
        let session = kt::unverified_fact(token, "session").map_err(from_kt)?;
        let ident = store.sessions.get(&session).ok_or(SignerError::UnknownSession)?;
        let any = ident.rights.iter().next().cloned().ok_or(SignerError::Denied("no grants".into()))?;
        self.verify(token, &any)
    }

    /// Sign an m4a key-proof statement (`m4a-key-login-v1` / `m4a-key-enroll-v1`) with the
    /// token's own session key, so the session's m4a account is proven by the same key that
    /// never leaves the keystore. Any other message is refused: this cannot sign letters or
    /// arbitrary bytes. The session must live on `node`.
    pub fn sign_m4a_keyproof(&self, node: &str, token: &str, message: &[u8]) -> Result<Signature, SignerError> {
        if !(message.starts_with(b"m4a-key-login-v1\n") || message.starts_with(b"m4a-key-enroll-v1\n")) {
            return Err(SignerError::Denied("not an m4a key proof".into()));
        }
        let (session, nick) = self.verify(token, grants::M4A_SEND)?;
        self.session_on_node(&session, node)?;
        let (store, _) = self.unlocked.as_ref().ok_or(SignerError::Locked)?;
        let ident = store.sessions.get(&session).ok_or(SignerError::UnknownSession)?;
        let key = kt::KeyHandle::from_seed(&ident.secret);
        Ok(Signature { nick, public_key: key.public_key(), signature: key.sign(message) })
    }

    /// The 32-byte master key of this session's m4a client vault (E2E device state, store keys),
    /// derived from the session's keystore secret. Only the session's own token on its own node
    /// gets it; another session's vault cannot be opened with it.
    pub fn m4a_vault_key(&self, node: &str, token: &str) -> Result<zeroize::Zeroizing<[u8; 32]>, SignerError> {
        let (session, nick) = self.verify(token, grants::M4A_SEND)?;
        self.session_on_node(&session, node)?;
        let (store, _) = self.unlocked.as_ref().ok_or(SignerError::Locked)?;
        let ident = store.sessions.get(&session).ok_or(SignerError::UnknownSession)?;
        let k = kt::Secret::from_bytes(ident.secret).derive(b"hatchery/m4a-vault/v1", nick.as_bytes());
        Ok(zeroize::Zeroizing::new(*k.expose_secret()))
    }

    pub fn has_node(&self, node: &str) -> bool {
        self.unlocked.as_ref().is_some_and(|(s, _)| s.nodes.contains(node))
    }

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

    pub fn revoke_session(&mut self, session: &str) -> Result<(), SignerError> {
        let (store, _) = self.unlocked.as_mut().ok_or(SignerError::Locked)?;
        let ident = store.sessions.get_mut(session).ok_or(SignerError::UnknownSession)?;
        ident.current = None;
        self.persist()
    }

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
        Ok(kt::KeyHandle::from_seed(&i.secret).public_key())
    }

    pub fn parent_of(&self, session: &str) -> Result<Option<String>, SignerError> {
        let (store, _) = self.unlocked.as_ref().ok_or(SignerError::Locked)?;
        Ok(store.sessions.get(session).ok_or(SignerError::UnknownSession)?.parent.clone())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// What a signature covers: domain, nick, operation (wire name), payload.
pub fn domain_message(nick: &str, op: &str, payload: &[u8]) -> Vec<u8> {
    kt::domain_message(legacy_v1::SIGN_DOMAIN, &[nick.as_bytes(), wire_op(op).as_bytes(), payload])
}

pub fn verify_signature(sig: &Signature, op: &str, payload: &[u8]) -> bool {
    kt::verify_signature(&sig.public_key, &domain_message(&sig.nick, op, payload), &sig.signature)
}

fn short(provider: &str) -> &str {
    match provider {
        "claude-code" => "claude",
        "kimi-code" => "kimi",
        p => p,
    }
}
