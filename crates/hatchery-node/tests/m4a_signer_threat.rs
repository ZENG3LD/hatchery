//! Threat model, real processes: a neighbour shell on the same node, holding another session's
//! handle, cannot get anything signed as that session. Uses the real kernel `SO_PEERCRED`,
//! real `/proc`, and the real hq-core signer in-process (the C2 hop is not under test here).
#![cfg(target_os = "linux")]

use std::sync::{Arc, Mutex};

use hatchery_keystore::{default_rights, HumanKey, Signer, RIGHT_SEND};
use hatchery_node::keystore_proxy::{request, serve, CoreSigner, ProxyResponse, SignedReply, SignerProxy};

struct InProcCore(Mutex<Signer>);
impl CoreSigner for InProcCore {
    fn sign(&self, token: &str, op: &str, payload: &[u8]) -> Result<SignedReply, String> {
        let s = self.0.lock().unwrap();
        let sig = s.sign(token, op, payload).map_err(|e| e.to_string())?;
        Ok(SignedReply { nick: sig.nick, public_key_hex: hex::encode(sig.public_key), signature_hex: hex::encode(sig.signature) })
    }
    fn provision(&self, provider: &str, session: &str) -> Result<(String, String), String> {
        let p = self.0.lock().unwrap().provision("box", provider, session, default_rights(), None).map_err(|e| e.to_string())?;
        Ok((p.nick, p.token))
    }
}

const ROLE: &str = "M4A_THREAT_CLIENT";

/// Re-entered as a child process: act as the session-side client and print the outcome.
#[test]
fn client_role() {
    let Ok(sock) = std::env::var(ROLE) else { return };
    let handle = std::env::var("M4A_SESSION_HANDLE").unwrap();
    match request(std::path::Path::new(&sock), &handle, RIGHT_SEND, b"letter").unwrap() {
        ProxyResponse::Signed(s) => println!("RESULT=signed:{}", s.nick),
        ProxyResponse::Refused { reason } => println!("RESULT=refused:{reason}"),
        ProxyResponse::Sent { result } => println!("RESULT=sent:{}", result["from"].as_str().unwrap_or("")),
    }
}

fn outcome(child: std::process::Child) -> String {
    let out = child.wait_with_output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.split("RESULT=").nth(1).and_then(|r| r.split_whitespace().next()).map(str::to_owned).unwrap_or_else(|| format!("no result: {text}"))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn neighbour_shell_cannot_sign_as_another_session() {
    let dir = std::env::temp_dir().join(format!("m4a-threat-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let human = HumanKey::from_bytes([9; 32]);
    let mut signer = Signer::init(dir.join("store"), &human).unwrap();
    signer.register_node("box", &human).unwrap();
    let a = signer.provision("box", "codex", "sess-a", default_rights(), None).unwrap();
    let b = signer.provision("box", "claude-code", "sess-b", default_rights(), None).unwrap();
    let proxy = Arc::new(SignerProxy::new(Arc::new(InProcCore(Mutex::new(signer)))));
    let sock = dir.join("signer.sock");
    let listener = tokio::net::UnixListener::bind(&sock).unwrap();
    tokio::spawn(serve(proxy.clone(), listener));
    let uid = unsafe { libc_getuid() };

    run_scenarios(&proxy, &sock, &a.nick, &a.token, &b.nick, &b.token, uid).await;
}

async fn run_scenarios(proxy: &Arc<SignerProxy>, sock: &std::path::Path, a_nick: &str, a_tok: &str, b_nick: &str, b_tok: &str, uid: u32) {
    // 1. Session A's own process signs as A.
    let (c, pid) = spawn_waiting(sock);
    let ha = proxy.bind("sess-a", a_nick, pid, uid, a_tok.to_owned());
    assert_eq!(release(c, &ha), format!("signed:{a_nick}"));

    // 2. A descendant of session B signs as B.
    let (c, pid) = spawn_waiting_descendant(sock);
    let hb = proxy.bind("sess-b", b_nick, pid, uid, b_tok.to_owned());
    assert_eq!(release(c, &hb), format!("signed:{b_nick}"));

    // 3. Neighbour: a process of session B presents session A's (stolen) handle -> refused.
    let (c_root_a, pid_a) = spawn_waiting(sock);
    let ha = proxy.bind("sess-a", a_nick, pid_a, uid, a_tok.to_owned());
    let (c_b, pid_b) = spawn_waiting(sock);
    let _hb = proxy.bind("sess-b", b_nick, pid_b, uid, b_tok.to_owned());
    let stolen = release(c_b, &ha);
    assert!(stolen.starts_with("refused:NotInSession"), "{stolen}");
    let _ = release(c_root_a, "none");

    // 5. Lazy path as wired at spawn: reserve before the PTY exists (refused until running),
    //    attach the PTY pid, first request provisions the identity at core.
    let h = proxy.reserve("sess-c", "codex", uid);
    let (c, pid) = spawn_waiting(sock);
    let early = proxy.sign_for_peer(&h, pid, uid, RIGHT_SEND, b"x");
    assert!(early.is_err(), "no pid attached yet");
    proxy.attach_pid("sess-c", pid);
    let got = release(c, &h);
    assert!(got.starts_with("signed:codex-"), "{got}");
    assert_ne!(got, format!("signed:{a_nick}"));

    // 4. After stop, even the session's own process is refused.
    let (c, pid) = spawn_waiting(sock);
    let ha = proxy.bind("sess-a", a_nick, pid, uid, a_tok.to_owned());
    proxy.unbind("sess-a");
    assert!(release(c, &ha).starts_with("refused:UnknownHandle"));
}

/// Child waits for its handle on stdin, then becomes the client (exec keeps the pid).
fn spawn_waiting(sock: &std::path::Path) -> (std::process::Child, u32) {
    spawn_with(sock, r#"read h; M4A_SESSION_HANDLE=$h exec "$0" "$@""#)
}
fn spawn_waiting_descendant(sock: &std::path::Path) -> (std::process::Child, u32) {
    spawn_with(sock, r#"read h; M4A_SESSION_HANDLE=$h "$0" "$@""#)
}
fn spawn_with(sock: &std::path::Path, script: &str) -> (std::process::Child, u32) {
    let exe = std::env::current_exe().unwrap();
    let child = std::process::Command::new("sh")
        .arg("-c")
        .arg(script)
        .arg(exe)
        .args(["client_role", "--exact", "--nocapture", "--test-threads=1"])
        .env(ROLE, sock)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let pid = child.id();
    (child, pid)
}
fn release(mut child: std::process::Child, handle: &str) -> String {
    use std::io::Write;
    let mut stdin = child.stdin.take().unwrap();
    writeln!(stdin, "{handle}").unwrap();
    drop(stdin);
    outcome(child)
}

extern "C" {
    #[link_name = "getuid"]
    fn libc_getuid() -> u32;
}
