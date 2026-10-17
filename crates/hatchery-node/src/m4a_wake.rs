//! Letter wake for live sessions the node adopted (`/m4a/adopt`).
//!
//! The old standalone mailbox daemon woke a session itself: its listener held the account's
//! push socket and, on a letter, pressed the turn on the provider's leader. The node owns the
//! accounts now, so it runs that listener itself, one per adopted account:
//!
//! - child: `m4a-agent node-client` with the account's own store (no capability: the listener
//!   only receives and serves the account's local send socket), supervised and restarted by
//!   the node;
//! - on a letter, m4a runs its letter hook (`M4A_LETTER_HOOK`), which is this binary in hook
//!   mode ([`hook_main`]): it reads the letter from stdin, starts a detached pusher and exits at
//!   once (m4a gives a hook 10 s);
//! - the pusher sends the framed letter into the live session through the provider's leader
//!   (grok: ACP `session/load` + `session/prompt`, `gate4agent_wake::grok_leader`), with the
//!   provider session id and working directory recorded at adoption.
//!
//! Adoptions persist in `<accounts root>/adopted.json` (no secret: provider, session id, cwd,
//! nick, account ref, store root), so a node restart restarts the listeners. Nothing here embeds
//! a host, key or name.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

pub const ADOPTED_FILE: &str = "adopted.json";
const HOOK_ENV: &str = "HATCHERY_LETTER_HOOK";
const PUSH_ENV: &str = "HATCHERY_LETTER_PUSH";
const WAKE_PROVIDER_ENV: &str = "HATCHERY_WAKE_PROVIDER";
const WAKE_SESSION_ENV: &str = "HATCHERY_WAKE_SESSION";
const WAKE_CWD_ENV: &str = "HATCHERY_WAKE_CWD";
const WAKE_LEADER_ENV: &str = "HATCHERY_WAKE_LEADER";
const WAKE_NICK_ENV: &str = "HATCHERY_WAKE_NICK";
/// Send socket name the session's `m4a-agent send` looks for under its store root.
const SEND_SOCK_NAME: &str = "web-client.sock";

/// One adopted live session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Adoption {
    pub provider: String,
    pub session: String,
    #[serde(default)]
    pub cwd: Option<String>,
    pub nick: String,
    pub account_ref: String,
    #[serde(default)]
    pub store_root: Option<String>,
}

pub fn load_adoptions(base: &Path) -> Vec<Adoption> {
    std::fs::read(base.join(ADOPTED_FILE)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

/// Inserts or replaces the adoption of `(provider, session)`.
pub fn save_adoption(base: &Path, a: &Adoption) -> Result<(), String> {
    let mut all = load_adoptions(base);
    all.retain(|x| !(x.provider == a.provider && x.session == a.session));
    all.push(a.clone());
    let tmp = base.join(format!("{ADOPTED_FILE}.tmp"));
    std::fs::write(&tmp, serde_json::to_vec_pretty(&all).map_err(|e| e.to_string())?).map_err(|e| format!("adopted: {}", e.kind()))?;
    std::fs::rename(&tmp, base.join(ADOPTED_FILE)).map_err(|e| format!("adopted: {}", e.kind()))
}

/// The provider session an attach with no session of its own belongs to: the only adoption of
/// that provider. Several adoptions are ambiguous (`None`).
pub fn sole_adoption(base: &Path, provider: &str) -> Option<String> {
    let mut it = load_adoptions(base).into_iter().filter(|a| a.provider == provider);
    let first = it.next()?;
    it.next().is_none().then_some(first.session)
}

/// Working directory of a grok session from the provider's own index (`active_sessions.json`
/// rows `{"session_id","cwd"}`), falling back to the session file's `cwd` field.
pub fn grok_session_cwd(grok_home: &Path, session: &str) -> Option<String> {
    let rows: serde_json::Value = std::fs::read(grok_home.join("active_sessions.json")).ok().and_then(|b| serde_json::from_slice(&b).ok())?;
    let rows = rows.as_array().cloned().or_else(|| rows.get("sessions").and_then(|s| s.as_array().cloned()))?;
    rows.iter()
        .find(|r| r.get("session_id").and_then(|v| v.as_str()) == Some(session))
        .and_then(|r| r.get("cwd").and_then(|v| v.as_str()))
        .filter(|c| !c.is_empty())
        .map(str::to_owned)
}

/// Leader endpoint of `provider` (grok only for now).
pub fn leader_for(provider: &str) -> Option<PathBuf> {
    match provider {
        "grok" => {
            let home = gate4agent_wake::grok_leader::grok_home()?;
            Some(gate4agent_wake::grok_leader::leader_socket(&home, std::env::var_os("GROK_LEADER_SOCKET").as_deref()))
        }
        _ => None,
    }
}

/// Environment of the listener child for one adoption.
pub fn listener_env(a: &Adoption, product_url: &str, leader: &Path, hook: &Path) -> Result<Vec<(String, String)>, String> {
    let store_root = a.store_root.as_deref().ok_or("adoption has no store root")?;
    let cwd = a.cwd.as_deref().ok_or("adoption has no working directory")?;
    let send_sock = Path::new(store_root).join(SEND_SOCK_NAME);
    let leader = leader.to_string_lossy().into_owned();
    Ok(vec![
        ("M4A_PRODUCT_URL".into(), product_url.to_owned()),
        ("M4A_SESSION_ID".into(), a.account_ref.clone()),
        ("M4A_STORE_ROOT".into(), store_root.to_owned()),
        ("M4A_BOT_NAME".into(), a.nick.clone()),
        ("M4A_NICK".into(), a.nick.clone()),
        ("M4A_LEADER_SOCK".into(), leader.clone()),
        ("M4A_LEADER_CWD".into(), cwd.to_owned()),
        ("M4A_SEND_SOCK".into(), send_sock.to_string_lossy().into_owned()),
        ("M4A_LETTER_HOOK".into(), hook.to_string_lossy().into_owned()),
        (HOOK_ENV.into(), "1".into()),
        (WAKE_PROVIDER_ENV.into(), a.provider.clone()),
        (WAKE_SESSION_ENV.into(), a.session.clone()),
        (WAKE_CWD_ENV.into(), cwd.to_owned()),
        (WAKE_LEADER_ENV.into(), leader),
        (WAKE_NICK_ENV.into(), a.nick.clone()),
    ])
}

struct Slot {
    adoption: Adoption,
    child: Option<Child>,
    next_start: Instant,
    backoff: Duration,
}

static SLOTS: Mutex<BTreeMap<String, Slot>> = Mutex::new(BTreeMap::new());
static STARTED: std::sync::Once = std::sync::Once::new();

/// Starts (or restarts) the listener of `a`, and the supervisor thread on first use.
pub fn watch(accounts: &crate::m4a_accounts::M4aSessionAccounts, a: Adoption) {
    if leader_for(&a.provider).is_none() {
        tracing::info!(provider = %a.provider, nick = %a.nick, "letter wake for this provider is not built in yet; letters stay in the inbox");
        return;
    }
    {
        let mut slots = SLOTS.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(mut old) = slots.remove(&a.account_ref) {
            stop(&mut old);
        }
        slots.insert(a.account_ref.clone(), Slot { adoption: a, child: None, next_start: Instant::now(), backoff: Duration::from_secs(5) });
    }
    let (bin, base) = (accounts.bin().to_path_buf(), accounts.base().to_path_buf());
    STARTED.call_once(move || {
        std::thread::Builder::new()
            .name("m4a-wake".into())
            .spawn(move || loop {
                supervise(&bin, &base);
                std::thread::sleep(Duration::from_secs(2));
            })
            .ok();
    });
}

/// Restarts the listeners of every saved adoption (node start).
pub fn restore(accounts: &crate::m4a_accounts::M4aSessionAccounts) {
    for a in load_adoptions(accounts.base()) {
        watch(accounts, a);
    }
}

fn stop(slot: &mut Slot) {
    if let Some(mut c) = slot.child.take() {
        let _ = c.kill();
        let _ = c.wait();
    }
}

fn supervise(bin: &Path, base: &Path) {
    let product_url = std::env::var(crate::m4a_accounts::PRODUCT_URL_ENV).ok().filter(|u| !u.is_empty());
    let hook = std::env::current_exe().ok();
    let mut slots = SLOTS.lock().unwrap_or_else(|p| p.into_inner());
    for slot in slots.values_mut() {
        if let Some(c) = slot.child.as_mut() {
            match c.try_wait() {
                Ok(None) => continue,
                Ok(Some(status)) => {
                    tracing::warn!(nick = %slot.adoption.nick, code = ?status.code(), "m4a letter listener exited; restarting");
                    slot.child = None;
                    slot.next_start = Instant::now() + slot.backoff;
                    slot.backoff = (slot.backoff * 2).min(Duration::from_secs(300));
                    continue;
                }
                Err(_) => continue,
            }
        }
        if Instant::now() < slot.next_start {
            continue;
        }
        let (Some(url), Some(hook), Some(leader)) = (product_url.as_deref(), hook.as_deref(), leader_for(&slot.adoption.provider)) else {
            slot.next_start = Instant::now() + Duration::from_secs(300);
            tracing::warn!(nick = %slot.adoption.nick, "m4a letter listener not started: messenger url, node binary or leader unknown");
            continue;
        };
        let env = match listener_env(&slot.adoption, url, &leader, hook) {
            Ok(env) => env,
            Err(why) => {
                tracing::warn!(nick = %slot.adoption.nick, %why, "m4a letter listener not started");
                slot.next_start = Instant::now() + Duration::from_secs(300);
                continue;
            }
        };
        let logs = base.join("logs");
        let _ = std::fs::create_dir_all(&logs);
        let log = std::fs::OpenOptions::new().create(true).append(true).open(logs.join(format!("{}.listen.log", slot.adoption.nick)));
        let mut cmd = Command::new(bin);
        cmd.arg("node-client").stdin(Stdio::null());
        for k in ["M4A_SESSION_CAP", "M4A_PRODUCT_INVITE", "M4A_ROUTINE_URL", "M4A_ROUTINE_BEARER", "M4A_HOMESERVER_URL"] {
            cmd.env_remove(k);
        }
        cmd.envs(env);
        match log.and_then(|f| Ok((f.try_clone()?, f))) {
            Ok((out, err)) => {
                cmd.stdout(out).stderr(err);
            }
            Err(_) => {
                cmd.stdout(Stdio::null()).stderr(Stdio::null());
            }
        }
        match cmd.spawn() {
            Ok(child) => {
                tracing::info!(nick = %slot.adoption.nick, provider = %slot.adoption.provider, pid = child.id(), "m4a letter listener started");
                slot.child = Some(child);
                slot.backoff = Duration::from_secs(5);
            }
            Err(e) => {
                tracing::warn!(nick = %slot.adoption.nick, error = %e.kind(), "m4a letter listener spawn failed");
                slot.next_start = Instant::now() + slot.backoff;
                slot.backoff = (slot.backoff * 2).min(Duration::from_secs(300));
            }
        }
    }
}

/// The prompt a woken session gets: headers, how to answer, then the letter verbatim.
pub fn letter_prompt(nick: &str, letter: &serde_json::Value) -> String {
    let s = |k: &str| letter.get(k).and_then(|v| v.as_str()).unwrap_or("").to_owned();
    let mut head = format!("[m4a letter]\nkind: {}\nfrom: {}\nto: {nick}\n", if s("kind").is_empty() { "message".into() } else { s("kind") }, s("from"));
    if !s("room").is_empty() {
        head.push_str(&format!("room: {}\n", s("room")));
    }
    head.push_str(&format!("event: {}\n", s("event_id")));
    if letter.get("reply_needed").and_then(|v| v.as_bool()).unwrap_or(true) {
        head.push_str(&format!("reply: your m4a MCP tool `send` with to = \"{}\"\n", s("from")));
    }
    format!("{head}---\n{}", s("body"))
}

/// Hook / pusher entry. `None`: this process is the ordinary node. `Some(code)`: exit with it.
pub fn hook_main() -> Option<i32> {
    if std::env::var(PUSH_ENV).is_ok_and(|v| v == "1") {
        return Some(push_main());
    }
    if !std::env::var(HOOK_ENV).is_ok_and(|v| v == "1") {
        return None;
    }
    use std::io::{Read, Write};
    let mut letter = Vec::new();
    if std::io::stdin().take(1 << 20).read_to_end(&mut letter).is_err() {
        return Some(2);
    }
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(&letter) else { return Some(2) };
    let nick = std::env::var(WAKE_NICK_ENV).ok().or_else(|| v.get("nick").and_then(|n| n.as_str()).map(str::to_owned)).unwrap_or_default();
    let prompt = letter_prompt(&nick, &v);
    let Ok(exe) = std::env::current_exe() else { return Some(2) };
    // Detached pusher: the turn can take minutes; the hook must answer within seconds.
    let mut cmd = Command::new(exe);
    cmd.env_remove(HOOK_ENV).env(PUSH_ENV, "1").stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    let Ok(mut child) = cmd.spawn() else { return Some(3) };
    if let Some(mut stdin) = child.stdin.take() {
        if stdin.write_all(prompt.as_bytes()).is_err() {
            return Some(3);
        }
    }
    Some(0)
}

fn push_main() -> i32 {
    use std::io::Read;
    let mut prompt = String::new();
    if std::io::stdin().take(1 << 20).read_to_string(&mut prompt).is_err() || prompt.trim().is_empty() {
        return 2;
    }
    let get = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
    let (Some(provider), Some(session), Some(cwd), Some(leader)) = (get(WAKE_PROVIDER_ENV), get(WAKE_SESSION_ENV), get(WAKE_CWD_ENV), get(WAKE_LEADER_ENV)) else {
        return 2;
    };
    if provider != "grok" {
        return 4;
    }
    let Ok(rt) = tokio::runtime::Builder::new_current_thread().enable_io().enable_time().build() else { return 5 };
    match rt.block_on(gate4agent_wake::grok_leader::push_into_session(Path::new(&leader), &session, &cwd, &prompt)) {
        Ok(()) => 0,
        Err(_) => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn adoption(session: &str) -> Adoption {
        Adoption { provider: "grok".into(), session: session.into(), cwd: Some("/w".into()), nick: "grok-1".into(), account_ref: "grok-ab".into(), store_root: Some("/s/grok-ab".into()) }
    }

    #[test]
    fn adoptions_persist_replace_and_resolve_a_sole_session() {
        let dir = std::env::temp_dir().join(format!("m4a-wake-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(sole_adoption(&dir, "grok"), None);
        save_adoption(&dir, &adoption("s-1")).unwrap();
        save_adoption(&dir, &adoption("s-1")).unwrap();
        assert_eq!(load_adoptions(&dir).len(), 1);
        assert_eq!(sole_adoption(&dir, "grok").as_deref(), Some("s-1"));
        assert_eq!(sole_adoption(&dir, "kimi"), None);
        save_adoption(&dir, &adoption("s-2")).unwrap();
        assert_eq!(sole_adoption(&dir, "grok"), None, "two adoptions are ambiguous");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn listener_env_carries_no_secret_and_points_the_hook_at_the_session() {
        let env = listener_env(&adoption("s-1"), "https://example.invalid", Path::new("/g/leader.sock"), Path::new("/bin/node")).unwrap();
        let m: BTreeMap<_, _> = env.into_iter().collect();
        assert!(!m.contains_key("M4A_SESSION_CAP"));
        assert_eq!(m["M4A_SESSION_ID"], "grok-ab");
        assert_eq!(m[WAKE_SESSION_ENV], "s-1");
        assert_eq!(m[WAKE_CWD_ENV], "/w");
        assert_eq!(m["M4A_LETTER_HOOK"], "/bin/node");
        assert!(m["M4A_SEND_SOCK"].ends_with(SEND_SOCK_NAME));
        let mut no_cwd = adoption("s-1");
        no_cwd.cwd = None;
        assert!(listener_env(&no_cwd, "u", Path::new("l"), Path::new("h")).is_err());
    }

    #[test]
    fn prompt_names_sender_and_reply_tool() {
        let p = letter_prompt("grok-1", &serde_json::json!({"from":"web","event_id":"$e","room":"!r","body":"hi"}));
        assert!(p.contains("from: web") && p.contains("to: grok-1") && p.contains("to = \"web\"") && p.ends_with("---\nhi"));
    }

    #[test]
    fn grok_cwd_comes_from_the_index() {
        let dir = std::env::temp_dir().join(format!("m4a-wake-idx-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("active_sessions.json"), r#"[{"session_id":"a","cwd":"C:\\w"},{"session_id":"b","cwd":""}]"#).unwrap();
        assert_eq!(grok_session_cwd(&dir, "a").as_deref(), Some("C:\\w"));
        assert_eq!(grok_session_cwd(&dir, "b"), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
