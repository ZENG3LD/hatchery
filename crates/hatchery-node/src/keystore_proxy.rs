//! Node-side keystore proxy (was `m4a_signer_proxy`) (option A, `specs/m4a-session-keys-2026-10-16.md`).
//!
//! The node holds NO m4a private keys. For each PTY session it keeps the session's capability
//! (issued by the hq-core signer) in its own memory and gives the PTY only a **handle**
//! (`M4A_SESSION_HANDLE`) plus the nick. A handle is useless on its own: the proxy accepts a
//! request on its UDS only if the kernel-reported peer (`SO_PEERCRED`) has the session's uid
//! and its pid is the session's PTY root process or a descendant of it. A neighbour shell on
//! the same node that steals the handle (env, `/proc/<pid>/environ`, a prompt injection) is
//! refused, and nothing it could steal lets it sign: the capability never enters the PTY and
//! the key never leaves hq-core.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

pub const HANDLE_ENV: &str = "M4A_SESSION_HANDLE";
pub const SOCKET_ENV: &str = "M4A_SIGNER_SOCK";

/// The hq-core signer as seen from the node (in production: a call over the C2 uplink).
pub trait CoreSigner: Send + Sync {
    fn sign(&self, token: &str, op: &str, payload: &[u8]) -> Result<SignedReply, String>;
    /// Identity for a fresh session: (nick, token). Default: not supported.
    fn provision(&self, _provider: &str, _session: &str) -> Result<(String, String), String> {
        Err("provision not supported".into())
    }
    /// Signer-signed letter delivered by core from the token's own nick.
    fn send(&self, _token: &str, _letter: &serde_json::Value) -> Result<serde_json::Value, String> {
        Err("send not supported".into())
    }
    fn revoke(&self, _session: &str) -> Result<(), String> {
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SignedReply {
    pub nick: String,
    pub public_key_hex: String,
    pub signature_hex: String,
}

/// Parent-pid lookup; `/proc` on Linux, injectable for tests.
pub trait ProcTree: Send + Sync {
    fn parent(&self, pid: u32) -> Option<u32>;
}

pub struct LinuxProc;

impl ProcTree for LinuxProc {
    fn parent(&self, pid: u32) -> Option<u32> {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        // `pid (comm) state ppid ...` — comm may contain spaces/parens; split after the last ')'.
        let rest = &stat[stat.rfind(')')? + 1..];
        rest.split_whitespace().nth(1)?.parse().ok()
    }
}

struct Binding {
    session: String,
    provider: String,
    nick: String,
    /// 0 until the PTY child is running (requests before that are refused).
    root_pid: u32,
    uid: u32,
    /// Empty until the first request provisions the identity at core.
    token: Zeroizing<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    UnknownHandle,
    WrongUser,
    NotInSession,
    Upstream(String),
}

pub struct SignerProxy {
    bindings: Mutex<HashMap<String, Binding>>,
    proc_tree: Box<dyn ProcTree>,
    core: Arc<dyn CoreSigner>,
}

impl SignerProxy {
    pub fn new(core: Arc<dyn CoreSigner>) -> Self {
        Self::with_proc(core, Box::new(LinuxProc))
    }

    pub fn with_proc(core: Arc<dyn CoreSigner>, proc_tree: Box<dyn ProcTree>) -> Self {
        Self { bindings: Mutex::new(HashMap::new()), proc_tree, core }
    }

    /// Called by spawn once the PTY child exists. Returns the handle for the PTY env.
    /// A rebind of the same session drops its previous handle.
    pub fn bind(&self, session: &str, nick: &str, root_pid: u32, uid: u32, token: String) -> String {
        let mut raw = [0u8; 24];
        fill_random(&mut raw);
        let handle = hex::encode(raw);
        let mut b = self.bindings.lock().unwrap_or_else(|p| p.into_inner());
        b.retain(|_, v| v.session != session);
        b.insert(
            handle.clone(),
            Binding { session: session.to_owned(), provider: String::new(), nick: nick.to_owned(), root_pid, uid, token: Zeroizing::new(token) },
        );
        handle
    }

    /// At spawn, before the PTY exists: a handle for its env; identity is provisioned lazily.
    pub fn reserve(&self, session: &str, provider: &str, uid: u32) -> String {
        let h = self.bind(session, "", 0, uid, String::new());
        if let Some(b) = self.bindings.lock().unwrap_or_else(|p| p.into_inner()).get_mut(&h) {
            b.provider = provider.to_owned();
        }
        h
    }

    /// The PTY child is running: bind the handle to its process tree.
    pub fn attach_pid(&self, session: &str, pid: u32) {
        for b in self.bindings.lock().unwrap_or_else(|p| p.into_inner()).values_mut() {
            if b.session == session && b.root_pid == 0 {
                b.root_pid = pid;
            }
        }
    }

    pub fn has_session(&self, session: &str) -> bool {
        self.bindings.lock().unwrap_or_else(|p| p.into_inner()).values().any(|b| b.session == session)
    }

    /// Stop: drop the handle and revoke at core.
    pub fn release(&self, session: &str) {
        if self.has_session(session) {
            self.unbind(session);
            if let Err(e) = self.core.revoke(session) {
                tracing::warn!(error = %e, "m4a signer revoke failed");
            }
        }
    }

    /// Authorize the peer and return (token, nick), provisioning at core on first use.
    fn authorize(&self, handle: &str, peer_pid: u32, peer_uid: u32) -> Result<(Zeroizing<String>, String), Refusal> {
        let (session, provider, token, nick) = {
            let b = self.bindings.lock().unwrap_or_else(|p| p.into_inner());
            let v = b.iter().find(|(h, _)| ct_eq(h.as_bytes(), handle.as_bytes())).map(|(_, v)| v).ok_or(Refusal::UnknownHandle)?;
            if v.uid != peer_uid {
                return Err(Refusal::WrongUser);
            }
            if v.root_pid == 0 || !self.in_tree(v.root_pid, peer_pid) {
                return Err(Refusal::NotInSession);
            }
            (v.session.clone(), v.provider.clone(), v.token.clone(), v.nick.clone())
        };
        if !token.is_empty() {
            return Ok((token, nick));
        }
        let (nick, token) = self.core.provision(&provider, &session).map_err(Refusal::Upstream)?;
        let mut b = self.bindings.lock().unwrap_or_else(|p| p.into_inner());
        let v = b.get_mut(handle).ok_or(Refusal::UnknownHandle)?;
        v.token = Zeroizing::new(token.clone());
        v.nick = nick.clone();
        Ok((Zeroizing::new(token), nick))
    }

    /// Send a letter as the peer's own session (core signs and delivers).
    pub fn send_for_peer(&self, handle: &str, peer_pid: u32, peer_uid: u32, letter: &serde_json::Value) -> Result<serde_json::Value, Refusal> {
        let (token, nick) = self.authorize(handle, peer_pid, peer_uid)?;
        let out = self.core.send(&token, letter).map_err(Refusal::Upstream)?;
        if out.get("from").and_then(|v| v.as_str()) != Some(nick.as_str()) {
            return Err(Refusal::Upstream("core answered for another nick".into()));
        }
        Ok(out)
    }

    /// Session stopped: its handle stops working at once.
    pub fn unbind(&self, session: &str) {
        self.bindings.lock().unwrap_or_else(|p| p.into_inner()).retain(|_, v| v.session != session);
    }

    fn in_tree(&self, root: u32, mut pid: u32) -> bool {
        for _ in 0..64 {
            if pid == root {
                return true;
            }
            match self.proc_tree.parent(pid) {
                Some(p) if p != 0 && p != pid => pid = p,
                _ => return false,
            }
        }
        false
    }

    /// Authorize a peer (kernel-reported pid/uid) presenting `handle`, then ask core to sign.
    pub fn sign_for_peer(&self, handle: &str, peer_pid: u32, peer_uid: u32, op: &str, payload: &[u8]) -> Result<SignedReply, Refusal> {
        let (token, nick) = self.authorize(handle, peer_pid, peer_uid)?;
        let reply = self.core.sign(&token, op, payload).map_err(Refusal::Upstream)?;
        if reply.nick != nick {
            return Err(Refusal::Upstream("core answered for another nick".into()));
        }
        Ok(reply)
    }
}

fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn fill_random(buf: &mut [u8]) {
    use std::io::Read;
    std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(buf)).expect("/dev/urandom");
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProxyRequest {
    pub handle: String,
    pub op: String,
    pub payload_hex: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ProxyResponse {
    Signed(SignedReply),
    Sent { result: serde_json::Value },
    Refused { reason: String },
}

/// Serve the proxy on a Unix socket (0600 dir/socket owned by the node user). One JSON line
/// request per connection; peer identity comes from `SO_PEERCRED`, never from the request.
#[cfg(unix)]
pub async fn serve(proxy: Arc<SignerProxy>, listener: tokio::net::UnixListener) {
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
    loop {
        let Ok((stream, _)) = listener.accept().await else { continue };
        let proxy = proxy.clone();
        tokio::spawn(async move {
            let cred = match stream.peer_cred() {
                Ok(c) => c,
                Err(_) => return,
            };
            let (r, mut w) = stream.into_split();
            let mut line = String::new();
            let mut reader = BufReader::new(r.take(64 * 1024));
            if reader.read_line(&mut line).await.is_err() {
                return;
            }
            let resp = match serde_json::from_str::<ProxyRequest>(&line) {
                Err(_) => ProxyResponse::Refused { reason: "bad request".into() },
                Ok(req) => match (hex::decode(&req.payload_hex), cred.pid()) {
                    (Ok(payload), Some(pid)) if pid > 0 => {
                        let proxy = proxy.clone();
                        let uid = cred.uid();
                        let res = tokio::task::spawn_blocking(move || {
                            if req.op == "send" {
                                let letter: serde_json::Value = serde_json::from_slice(&payload).map_err(|_| Refusal::Upstream("bad letter".into()))?;
                                proxy.send_for_peer(&req.handle, pid as u32, uid, &letter).map(|result| ProxyResponse::Sent { result })
                            } else {
                                proxy.sign_for_peer(&req.handle, pid as u32, uid, &req.op, &payload).map(ProxyResponse::Signed)
                            }
                        })
                        .await;
                        match res {
                            Ok(Ok(r)) => r,
                            Ok(Err(e)) => {
                                tracing::warn!(peer_pid = pid, reason = ?e, "m4a signer proxy refused a request");
                                ProxyResponse::Refused { reason: format!("{e:?}") }
                            }
                            Err(_) => ProxyResponse::Refused { reason: "internal".into() },
                        }
                    }
                    _ => ProxyResponse::Refused { reason: "bad request".into() },
                },
            };
            let mut out = serde_json::to_vec(&resp).unwrap_or_default();
            out.push(b'\n');
            let _ = w.write_all(&out).await;
        });
    }
}

/// Blocking client (what the session-side `m4a` tool does).
#[cfg(unix)]
pub fn request(sock: &std::path::Path, handle: &str, op: &str, payload: &[u8]) -> std::io::Result<ProxyResponse> {
    use std::io::{BufRead, Write};
    let mut s = std::os::unix::net::UnixStream::connect(sock)?;
    let mut req = serde_json::to_vec(&ProxyRequest { handle: handle.into(), op: op.into(), payload_hex: hex::encode(payload) })?;
    req.push(b'\n');
    s.write_all(&req)?;
    let mut line = String::new();
    std::io::BufReader::new(s).read_line(&mut line)?;
    serde_json::from_str(&line).map_err(std::io::Error::other)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeTree(HashMap<u32, u32>);
    impl ProcTree for FakeTree {
        fn parent(&self, pid: u32) -> Option<u32> {
            self.0.get(&pid).copied()
        }
    }
    struct Echo;
    impl CoreSigner for Echo {
        fn sign(&self, token: &str, _op: &str, _p: &[u8]) -> Result<SignedReply, String> {
            Ok(SignedReply { nick: token.trim_start_matches("tok-").into(), public_key_hex: String::new(), signature_hex: String::new() })
        }
    }

    #[test]
    fn handle_works_only_inside_its_session_tree_and_for_its_uid() {
        // 100 = PTY root of A, 101 = child of A, 200 = PTY root of B, 201 = neighbour shell in B.
        let tree = FakeTree([(101, 100), (100, 1), (201, 200), (200, 1)].into_iter().collect());
        let p = SignerProxy::with_proc(Arc::new(Echo), Box::new(tree));
        let ha = p.bind("sa", "a", 100, 1000, "tok-a".into());
        let hb = p.bind("sb", "b", 200, 1000, "tok-b".into());
        assert_eq!(p.sign_for_peer(&ha, 100, 1000, "send_as", b"x").unwrap().nick, "a");
        assert_eq!(p.sign_for_peer(&ha, 101, 1000, "send_as", b"x").unwrap().nick, "a");
        assert_eq!(p.sign_for_peer(&ha, 201, 1000, "send_as", b"x"), Err(Refusal::NotInSession));
        assert_eq!(p.sign_for_peer(&ha, 100, 1001, "send_as", b"x"), Err(Refusal::WrongUser));
        assert_eq!(p.sign_for_peer(&hb, 201, 1000, "send_as", b"x").unwrap().nick, "b");
        assert_eq!(p.sign_for_peer("guess", 100, 1000, "send_as", b"x"), Err(Refusal::UnknownHandle));
        p.unbind("sa");
        assert_eq!(p.sign_for_peer(&ha, 100, 1000, "send_as", b"x"), Err(Refusal::UnknownHandle));
        let ha2 = p.bind("sb", "b", 200, 1000, "tok-b".into());
        assert_eq!(p.sign_for_peer(&hb, 200, 1000, "send_as", b"x"), Err(Refusal::UnknownHandle), "rebind kills the old handle");
        assert!(p.sign_for_peer(&ha2, 200, 1000, "send_as", b"x").is_ok());
    }
}

/// Core signer over the node-call channel (Noise over the uplink, opaque to C2; core takes
/// the node id from the stream). Called from blocking threads only.
pub struct UplinkCoreSigner {
    calls: crate::uplink_calls::NodeCalls,
    rt: tokio::runtime::Handle,
}

impl UplinkCoreSigner {
    pub fn new(calls: crate::uplink_calls::NodeCalls, rt: tokio::runtime::Handle) -> Self {
        Self { calls, rt }
    }
    fn call(&self, op: &str, payload: serde_json::Value) -> Result<serde_json::Value, String> {
        self.rt.block_on(self.calls.call(op, &payload)).map_err(|e| format!("{e:?}"))
    }
}

impl CoreSigner for UplinkCoreSigner {
    fn sign(&self, token: &str, op: &str, payload: &[u8]) -> Result<SignedReply, String> {
        let v = self.call("signer.sign", serde_json::json!({ "token": token, "op": op, "payload_hex": hex::encode(payload) }))?;
        serde_json::from_value(v).map_err(|e| e.to_string())
    }
    fn provision(&self, provider: &str, session: &str) -> Result<(String, String), String> {
        let v = self.call("signer.provision", serde_json::json!({ "provider": provider, "session": session }))?;
        Ok((v["nick"].as_str().ok_or("no nick")?.to_owned(), v["token"].as_str().ok_or("no token")?.to_owned()))
    }
    fn send(&self, token: &str, letter: &serde_json::Value) -> Result<serde_json::Value, String> {
        let mut p = letter.clone();
        p["token"] = serde_json::Value::String(token.to_owned());
        self.call("signer.send", p)
    }
    fn revoke(&self, session: &str) -> Result<(), String> {
        self.call("signer.revoke", serde_json::json!({ "session": session })).map(|_| ())
    }
}
