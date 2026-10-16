//! Delivery of keystore-signed letters into the real m4a (the mail4agent messenger that holds the
//! rooms and the human/agent accounts). There is no second messenger for sessions: a session's
//! letter is posted by that session's own m4a account.
//!
//! - The session's m4a account is proven by the session's keystore key (m4a key-proof login,
//!   `m4a-key-login-v1`), so the key never leaves the keystore and there is no password.
//! - The first letter enrolls the account: an admin invite for the session's nick from the m4a
//!   product server, then `m4a-key-enroll-v1` signed by the same key.
//! - Letters go as a 1:1 room (`is_direct`) with the recipient, reused afterwards.
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

pub struct M4aLink {
    host: String,
    admin_var: String,
    state_path: PathBuf,
}

/// What the keystore must do for the link: sign a key-proof message as the session.
pub trait ProofSigner {
    fn sign_proof(&self, message: &[u8]) -> Result<[u8; 64], LinkError>;
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

    /// Log in (enrolling first if needed) as the session's own m4a account. Returns (m4a nick, token).
    fn session_login(&self, st: &mut State, nick: &str, public_key: &[u8; 32], signer: &dyn ProofSigner) -> Result<(String, String), LinkError> {
        let key_id = key_id_of(public_key);
        if let Some(a) = st.accounts.get(nick).filter(|a| a.key_id == key_id) {
            let c = self.ok("POST", "/product/v1/login/key/challenge", None, Some(&json!({ "key_id": key_id })))?;
            let s = |k: &str| c.get(k).and_then(Value::as_str).unwrap_or("").to_owned();
            let msg = login_message(&s("audience"), &key_id, &s("challenge_id"), &s("nonce"), c.get("expires_ms").and_then(Value::as_i64).unwrap_or(0));
            let sig = signer.sign_proof(&msg)?;
            let v = self.ok("POST", "/product/v1/login/key", None, Some(&json!({ "key_id": key_id, "challenge_id": s("challenge_id"), "signature": b64(&sig) })))?;
            let token = v.get("token").and_then(Value::as_str).ok_or_else(|| e(502, "no token"))?.to_owned();
            return Ok((a.m4a_nick.clone(), token));
        }
        let admin = std::env::var(&self.admin_var).ok().filter(|v| !v.is_empty()).ok_or_else(|| e(503, "m4a admin bearer not configured"))?;
        let inv = self.ok("POST", "/product/v1/admin/invite", Some(&admin), Some(&json!({ "nick": nick })))?;
        let code = inv.get("invite").and_then(Value::as_str).ok_or_else(|| e(502, "no invite"))?.to_owned();
        let audience = inv.get("audience").and_then(Value::as_str).unwrap_or("").to_owned();
        let pk = b64(public_key);
        let sig = signer.sign_proof(&enroll_message(&audience, &code, &pk, &key_id))?;
        let v = self.ok("POST", "/product/v1/enroll", None, Some(&json!({ "invite": code, "public_key": pk, "signature": b64(&sig), "label": "hatchery-session", "nick": nick })))?;
        let m4a_nick = v.get("nick").and_then(Value::as_str).ok_or_else(|| e(502, "no nick"))?.to_owned();
        let token = v.get("token").and_then(Value::as_str).ok_or_else(|| e(502, "no token"))?.to_owned();
        st.accounts.insert(nick.to_owned(), Account { m4a_nick: m4a_nick.clone(), key_id, rooms: BTreeMap::new() });
        self.save(st)?;
        Ok((m4a_nick, token))
    }

    /// Post `text` from the session `nick` to the m4a account `to`. Returns (m4a nick, room, event).
    pub fn deliver(&self, nick: &str, public_key: &[u8; 32], signer: &dyn ProofSigner, to: &str, text: &str) -> Result<(String, String, String), LinkError> {
        let mut st = self.load();
        let (m4a_nick, token) = self.session_login(&mut st, nick, public_key, signer)?;
        let who = self.ok("GET", "/_matrix/client/v3/account/whoami", Some(&token), None)?;
        let me = who.get("user_id").and_then(Value::as_str).ok_or_else(|| e(502, "whoami"))?.to_owned();
        let server = me.split_once(':').map(|(_, s)| s.to_owned()).ok_or_else(|| e(502, "user id"))?;
        let bare = to.trim_start_matches('@');
        let target = if bare.contains(':') { format!("@{bare}") } else { format!("@{bare}:{server}") };
        let known = st.accounts.get(nick).and_then(|a| a.rooms.get(&target).cloned());
        let room = match known {
            Some(r) => r,
            None => {
                let v = self.ok("POST", "/_matrix/client/v3/createRoom", Some(&token), Some(&json!({ "is_direct": true, "invite": [target], "preset": "trusted_private_chat" })))?;
                let r = v.get("room_id").and_then(Value::as_str).ok_or_else(|| e(502, "no room"))?.to_owned();
                let path = format!("/_matrix/client/v3/user/{}/account_data/m.direct", urlenc(&me));
                let mut direct = self.call("GET", &path, Some(&token), None).ok().filter(|(s, _)| *s == 200).map(|(_, v)| v).unwrap_or_else(|| json!({}));
                if let Some(obj) = direct.as_object_mut() {
                    let list = obj.entry(target.clone()).or_insert_with(|| json!([]));
                    if let Some(a) = list.as_array_mut() {
                        a.push(json!(r));
                    }
                }
                let _ = self.call("PUT", &path, Some(&token), Some(&direct));
                if let Some(a) = st.accounts.get_mut(nick) {
                    a.rooms.insert(target.clone(), r.clone());
                }
                self.save(&st)?;
                r
            }
        };
        let txn = hex::encode(Sha256::digest(format!("{nick}\0{room}\0{text}\0{}", now_ms()).as_bytes()));
        let path = format!("/_matrix/client/v3/rooms/{}/send/m.room.message/{}", urlenc(&room), &txn[..24]);
        let v = self.ok("PUT", &path, Some(&token), Some(&json!({ "msgtype": "m.text", "body": text })))?;
        let event = v.get("event_id").and_then(Value::as_str).unwrap_or("").to_owned();
        let _ = self.call("POST", "/product/v1/logout", Some(&token), Some(&json!({})));
        Ok((m4a_nick, room, event))
    }
}

fn now_ms() -> u128 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis()
}

fn urlenc(s: &str) -> String {
    s.bytes()
        .map(|b| if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') { (b as char).to_string() } else { format!("%{b:02X}") })
        .collect()
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
        assert_eq!(urlenc("!r:s"), "%21r%3As");
        assert_eq!(dechunk(b"3\r\nabc\r\n0\r\n\r\n"), b"abc");
    }
}
