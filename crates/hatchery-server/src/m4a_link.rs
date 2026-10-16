//! Delivery of keystore-signed letters into the real m4a (the mail4agent messenger that holds the
//! rooms and the human/agent accounts). There is no second messenger for sessions: a session's
//! letter is posted by that session's own m4a account.
//!
//! - The session's m4a account is proven by the session's keystore key (m4a key-proof login,
//!   `m4a-key-login-v1`), so the key never leaves the keystore and there is no password.
//! - The first letter enrolls the account: an admin invite for the session's nick from the m4a
//!   product server, then `m4a-key-enroll-v1` signed by the same key.
//! - Letters go as a 1:1 room (`is_direct`) with the recipient, reused afterwards.
//! - [`M4aLink::deliver_e2e`] sends end-to-end encrypted (Olm/Megolm) through the m4a client
//!   engine; its vault master key and identity key are rooted in the keystore, per session.
//!
//! Configuration (env, no defaults that name a deployment):
//! `HATCHERY_M4A_PRODUCT_URL` (http base of the m4a product server) and
//! `HATCHERY_M4A_ADMIN_TOKEN_VAR` (NAME of the env var that holds the product admin bearer).

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub const PRODUCT_URL_ENV: &str = "HATCHERY_M4A_PRODUCT_URL";
pub const ADMIN_TOKEN_VAR_ENV: &str = "HATCHERY_M4A_ADMIN_TOKEN_VAR";

#[derive(Debug)]
pub struct LinkError(pub u16, pub String);

fn e(code: u16, m: impl Into<String>) -> LinkError {
    LinkError(code, m.into())
}

#[derive(Serialize, Deserialize, Default, Clone)]
struct Account {
    m4a_nick: String,
    key_id: String,
    /// recipient user id -> direct room id
    #[serde(default)]
    rooms: BTreeMap<String, String>,
}

#[derive(Serialize, Deserialize, Default)]
struct State {
    /// keystore nick -> m4a account
    accounts: BTreeMap<String, Account>,
}

#[derive(Clone)]
pub struct M4aLink {
    host: String,
    admin_var: String,
    state_path: PathBuf,
}


pub fn key_id_of(public_key: &[u8]) -> String {
    format!("k{}", &hex::encode(Sha256::digest(public_key))[..32])
}

pub fn b64(b: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(b)
}

pub fn login_message(audience: &str, key_id: &str, challenge_id: &str, nonce: &str, expires_ms: i64) -> Vec<u8> {
    format!("m4a-key-login-v1\n{audience}\n{key_id}\n{challenge_id}\n{nonce}\n{expires_ms}\n").into_bytes()
}

pub fn enroll_message(audience: &str, invite: &str, public_key_b64: &str, key_id: &str) -> Vec<u8> {
    let ih = hex::encode(Sha256::digest(invite.as_bytes()));
    format!("m4a-key-enroll-v1\n{audience}\n{ih}\n{key_id}\n{public_key_b64}\n").into_bytes()
}

impl M4aLink {
    pub fn from_env(data_dir: &Path) -> Option<Self> {
        let url = std::env::var(PRODUCT_URL_ENV).ok().filter(|v| !v.is_empty())?;
        let host = url.trim_end_matches('/').strip_prefix("http://")?.to_owned();
        let admin_var = std::env::var(ADMIN_TOKEN_VAR_ENV).ok().filter(|v| !v.is_empty())?;
        Some(Self { host, admin_var, state_path: data_dir.join("m4a-link.json") })
    }

    fn load(&self) -> State {
        std::fs::read(&self.state_path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    fn save(&self, s: &State) -> Result<(), LinkError> {
        let tmp = self.state_path.with_extension("tmp");
        let mut o = std::fs::OpenOptions::new();
        o.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            o.mode(0o600);
        }
        let bytes = serde_json::to_vec(s).map_err(|x| e(500, x.to_string()))?;
        o.open(&tmp).and_then(|mut f| f.write_all(&bytes)).map_err(|x| e(500, x.kind().to_string()))?;
        std::fs::rename(&tmp, &self.state_path).map_err(|x| e(500, x.kind().to_string()))
    }

    fn call(&self, method: &str, path: &str, bearer: Option<&str>, body: Option<&Value>) -> Result<(u16, Value), LinkError> {
        let mut s = std::net::TcpStream::connect(&self.host).map_err(|x| e(502, format!("m4a unreachable: {}", x.kind())))?;
        s.set_read_timeout(Some(std::time::Duration::from_secs(20))).ok();
        let payload = body.map(|b| b.to_string()).unwrap_or_default();
        let mut req = format!("{method} {path} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nAccept: application/json\r\n", self.host);
        if let Some(t) = bearer {
            req.push_str(&format!("Authorization: Bearer {t}\r\n"));
        }
        if body.is_some() {
            req.push_str(&format!("Content-Type: application/json\r\nContent-Length: {}\r\n", payload.len()));
        }
        req.push_str("\r\n");
        req.push_str(&payload);
        s.write_all(req.as_bytes()).map_err(|x| e(502, x.kind().to_string()))?;
        let mut raw = Vec::new();
        s.read_to_end(&mut raw).map_err(|x| e(502, x.kind().to_string()))?;
        let split = raw.windows(4).position(|w| w == b"\r\n\r\n").ok_or_else(|| e(502, "bad m4a answer"))?;
        let head = String::from_utf8_lossy(&raw[..split]).to_string();
        let mut body = raw[split + 4..].to_vec();
        if head.to_ascii_lowercase().contains("transfer-encoding: chunked") {
            body = dechunk(&body);
        }
        let status: u16 = head.split_whitespace().nth(1).and_then(|c| c.parse().ok()).unwrap_or(502);
        let v = serde_json::from_slice(&body).unwrap_or(Value::Null);
        Ok((status, v))
    }

    fn ok(&self, method: &str, path: &str, bearer: Option<&str>, body: Option<&Value>) -> Result<Value, LinkError> {
        let (st, v) = self.call(method, path, bearer, body)?;
        if (200..300).contains(&st) {
            Ok(v)
        } else {
            let msg = v.get("error").and_then(Value::as_str).unwrap_or("m4a refused").to_owned();
            Err(e(st, format!("{method} {}: {msg}", path.split('?').next().unwrap_or(path))))
        }
    }

}

/// Signs a key-proof statement as the session, inside the keystore (never exported).
pub type ExternalSign = Box<dyn Fn(&[u8]) -> Result<[u8; 64], String> + Send + Sync>;

/// The session's m4a client vault: a file vault under a master key derived from the session's
/// keystore secret, plus the session's identity key, which stays in the keystore and only signs.
struct HostVault {
    file: m4a_agent::FileVault,
    session: String,
    public_key: [u8; 32],
    sign: ExternalSign,
}

impl m4a_agent::KeyVault for HostVault {
    fn get(&self, label: &str) -> m4a_agent::error::Result<Option<zeroize::Zeroizing<Vec<u8>>>> {
        self.file.get(label)
    }
    fn put(&self, label: &str, value: &[u8]) -> m4a_agent::error::Result<()> {
        self.file.put(label, value)
    }
    fn delete(&self, label: &str) -> m4a_agent::error::Result<bool> {
        self.file.delete(label)
    }
    fn labels(&self) -> m4a_agent::error::Result<Vec<String>> {
        self.file.labels()
    }
    fn kind(&self) -> &'static str {
        "file+keystore"
    }
    fn external_identity(&self, session_id: &str) -> Option<[u8; 32]> {
        (session_id == self.session).then_some(self.public_key)
    }
    fn sign_external(&self, session_id: &str, msg: &[u8]) -> m4a_agent::error::Result<[u8; 64]> {
        if session_id != self.session {
            return Err(m4a_agent::AgentError::Vault("not this session's key".into()));
        }
        (self.sign)(msg).map_err(m4a_agent::AgentError::Vault)
    }
}

impl M4aLink {
    fn sessions_root(&self) -> PathBuf {
        self.state_path.parent().unwrap_or(Path::new(".")).join("m4a-sessions")
    }

    /// End-to-end encrypted letter (Olm/Megolm, the m4a client engine) from the session `nick`
    /// to `to`. Each session has its own root, vault and device: one session never opens another
    /// session's keys. Returns (m4a nick, room, event).
    pub fn deliver_e2e(&self, nick: &str, public_key: [u8; 32], vault_key: zeroize::Zeroizing<[u8; 32]>, sign: ExternalSign, to: &str, text: &str) -> Result<(String, String, String), LinkError> {
        use mail4agent_messenger_shell::{store_key, HostSession, MachineClient};
        use m4a_agent::{BackendKind, IdentityStore, KeyVault};
        let url = format!("http://{}", self.host);
        let root = self.sessions_root().join(nick);
        std::fs::create_dir_all(root.join(".m4a-agent")).map_err(|x| e(500, x.kind().to_string()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700));
        }
        let file = m4a_agent::FileVault::open(root.join(".m4a-agent").join("vault.enc"), Box::new(m4a_agent::StaticMasterKey(zeroize::Zeroizing::new(vault_key.to_vec()))))
            .map_err(|x| e(500, format!("session vault: {x}")))?;
        let vault: std::sync::Arc<dyn KeyVault> = std::sync::Arc::new(HostVault { file, session: nick.to_owned(), public_key, sign });
        store_key::install_vault(&root, vault.clone());
        let result = (|| {
            let ids = IdentityStore::new(vault.clone());
            let mut id = ids.resolve(nick, BackendKind::Server, &url).map_err(|x| e(500, format!("identity: {x}")))?;
            let mut st = self.load();
            // An account this session already enrolled with the same keystore key is reused.
            if !id.enrolled {
                if let Some(a) = st.accounts.get(nick).filter(|a| a.key_id == id.key_id).cloned() {
                    id.enrolled = true;
                    ids.adopt_nick(&mut id, &a.m4a_nick).map_err(|x| e(500, x.to_string()))?;
                    ids.save(&id).map_err(|x| e(500, x.to_string()))?;
                }
            }
            let mut hs = HostSession::new(nick, nick).with_tier(BackendKind::Server);
            hs.store_root = Some(root.clone());
            if !id.enrolled {
                let admin = std::env::var(&self.admin_var).ok().filter(|v| !v.is_empty()).ok_or_else(|| e(503, "m4a admin bearer not configured"))?;
                let inv = self.ok("POST", "/product/v1/admin/invite", Some(&admin), Some(&json!({ "nick": nick })))?;
                let code = inv.get("invite").and_then(Value::as_str).ok_or_else(|| e(502, "no invite"))?.to_owned();
                hs = hs.with_invite(code);
            }
            let mut client = MachineClient::open(&url, &root, vec![hs]).map_err(|x| e(502, format!("m4a client: {x}")))?;
            let reply = client.send_blocking(nick, to, text, std::time::Duration::from_secs(120));
            drop(client);
            let id = ids.resolve(nick, BackendKind::Server, &url).map_err(|x| e(500, format!("identity: {x}")))?;
            let m4a_nick = id.nick.clone().unwrap_or_else(|| nick.to_owned());
            if !st.accounts.contains_key(nick) && id.enrolled {
                st.accounts.insert(nick.to_owned(), Account { m4a_nick: m4a_nick.clone(), key_id: id.key_id.clone(), rooms: BTreeMap::new() });
                self.save(&st)?;
            }
            if !reply.ok {
                return Err(e(502, reply.error.unwrap_or_else(|| "m4a send failed".into())));
            }
            Ok((m4a_nick, reply.room.unwrap_or_default(), reply.event_id.unwrap_or_default()))
        })();
        store_key::remove_vault(&root);
        result
    }
}

fn dechunk(b: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let Some(nl) = b[i..].windows(2).position(|w| w == b"\r\n") else { break };
        let size = usize::from_str_radix(String::from_utf8_lossy(&b[i..i + nl]).trim(), 16).unwrap_or(0);
        i += nl + 2;
        if size == 0 || i + size > b.len() {
            break;
        }
        out.extend_from_slice(&b[i..i + size]);
        i += size + 2;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statements_match_the_m4a_contract() {
        // Same text as m4a_seam::keyproof (fixed UTF-8 lines, newline-terminated).
        assert_eq!(login_message("a", "k1", "c", "n", 5), b"m4a-key-login-v1\na\nk1\nc\nn\n5\n".to_vec());
        let m = enroll_message("a", "inv", "PK", "k1");
        assert!(m.starts_with(b"m4a-key-enroll-v1\na\n"));
        assert!(m.ends_with(b"\nk1\nPK\n"));
        assert_eq!(key_id_of(&[0u8; 32]).len(), 33);
        assert_eq!(dechunk(b"3\r\nabc\r\n0\r\n\r\n"), b"abc");
    }
}
