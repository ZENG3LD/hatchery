//! Letter wake for live sessions the node adopted (`/m4a/adopt`).
//!
//! The old standalone mailbox daemon woke a session itself: its listener held the account's
//! push socket and pressed the turn on the provider's leader. The node owns the accounts now,
//! so it runs that listener in-process, one thread per adopted account (mail4agent's library:
//! open the account store, sync/push, new inbound letters). The store dedups by event and never
//! hands over the account's own messages; each letter goes to a per-session queue whose one
//! worker pushes it into the live session through the leader (grok: ACP `session/load` +
//! `session/prompt`, `gate4agent_wake::grok_leader`), with the session id and cwd recorded at
//! adoption, so at most one push runs into a session at a time.
//!
//! Adoptions persist in `<accounts root>/adopted.json` (no secret: provider, session id, cwd,
//! nick, account ref, store root), so a node restart restarts the listeners. Nothing here embeds
//! a host, key or name.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

pub const ADOPTED_FILE: &str = "adopted.json";
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

static GENERATIONS: Mutex<BTreeMap<String, u64>> = Mutex::new(BTreeMap::new());

/// Starts (or restarts) the in-process mailbox listener of `a`.
pub fn watch(accounts: &crate::m4a_accounts::M4aSessionAccounts, a: Adoption) -> bool {
    let _ = accounts;
    let Some(leader) = leader_for(&a.provider) else {
        tracing::warn!(provider = %a.provider, nick = %a.nick, "letter listener not started: no leader for this provider (grok home unknown or provider not built in); letters stay in the inbox");
        return false;
    };
    if a.cwd.is_none() || a.store_root.is_none() {
        tracing::warn!(nick = %a.nick, "letter listener not started: adoption has no cwd or store root (adopt again with --cwd)");
        return false;
    }
    let gen = {
        let mut g = GENERATIONS.lock().unwrap_or_else(|p| p.into_inner());
        let e = g.entry(a.account_ref.clone()).or_insert(0);
        *e += 1;
        *e
    };
    let nick = a.nick.clone();
    match std::thread::Builder::new().name(format!("m4a-wake-{}", a.nick)).spawn(move || listen(a, leader, gen)) {
        Ok(_) => {
            tracing::info!(%nick, "letter listener starting");
            true
        }
        Err(e) => {
            tracing::warn!(%nick, error = %e, "letter listener thread did not start");
            false
        }
    }
}

/// Restarts the listeners of every saved adoption (node start). Returns how many started.
pub fn restore(accounts: &crate::m4a_accounts::M4aSessionAccounts) -> usize {
    let path = accounts.base().join(ADOPTED_FILE);
    let adoptions = match std::fs::read(&path) {
        Ok(b) => match serde_json::from_slice::<Vec<Adoption>>(&b) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(file = %path.display(), error = %e, "adopted sessions file is not readable json; no letter listeners restored");
                return 0;
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            tracing::info!(file = %path.display(), "no adopted sessions to restore");
            return 0;
        }
        Err(e) => {
            tracing::warn!(file = %path.display(), error = %e.kind(), "adopted sessions file not readable; no letter listeners restored");
            return 0;
        }
    };
    let mut started = 0;
    for a in adoptions {
        if watch(accounts, a) {
            started += 1;
        }
    }
    tracing::info!(started, "letter listeners restored from adopted sessions");
    started
}

/// Node start: restore when the node has m4a accounts configured; says why not otherwise.
pub fn restore_from_env() -> usize {
    match crate::m4a_accounts::M4aSessionAccounts::from_env() {
        Some(accounts) => restore(&accounts),
        None => {
            tracing::info!("m4a accounts not configured (agent bin / accounts root unset); no letter listeners restored");
            0
        }
    }
}

/// Whether a listener of this account is currently wanted (started and not replaced).
pub fn is_watched(account_ref: &str) -> bool {
    GENERATIONS.lock().unwrap_or_else(|p| p.into_inner()).contains_key(account_ref)
}

fn current(a: &Adoption, gen: u64) -> bool {
    GENERATIONS.lock().unwrap_or_else(|p| p.into_inner()).get(&a.account_ref) == Some(&gen)
}

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

/// One adopted account: open its store, keep sync/push going, hand each new inbound letter to
/// [`LeaderPush`] (the store dedups by event and never hands over its own messages).
fn listen(a: Adoption, leader: PathBuf, gen: u64) {
    use mail4agent_messenger_shell::{HostSession, MachineClient, ProviderKind, ProviderSession, SessionKind, WakeChain};
    let mut backoff = Duration::from_secs(5);
    let pusher = LeaderPush::start(a.clone(), leader);
    while current(&a, gen) {
        let (Some(url), Some(root), Some(cwd)) = (std::env::var(crate::m4a_accounts::PRODUCT_URL_ENV).ok().filter(|u| !u.is_empty()), a.store_root.clone(), a.cwd.clone()) else {
            tracing::warn!(nick = %a.nick, "m4a letter listener not started: messenger url, store root or cwd unknown");
            return;
        };
        let opened = MachineClient::open(&url, Path::new(&root), vec![HostSession::new(a.nick.clone(), a.account_ref.clone())]);
        let mut client = match opened {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(nick = %a.nick, error = %e, "m4a letter listener: store did not open; retrying");
                std::thread::sleep(backoff);
                backoff = (backoff * 2).min(Duration::from_secs(300));
                continue;
            }
        };
        backoff = Duration::from_secs(5);
        // The session's own `send` tool reaches this open store through the account's send socket.
        if let Err(e) = client.listen_for_sends(&Path::new(&root).join(SEND_SOCK_NAME)) {
            tracing::warn!(nick = %a.nick, error = %e, "m4a send socket not opened");
        }
        let session = ProviderSession { kind: SessionKind::local(ProviderKind::Grok), session_id: a.session.clone(), nick: a.nick.clone(), cwd: Some(PathBuf::from(&cwd)), headless: false };
        let mut chain = WakeChain::new();
        chain.push("node-leader", Box::new(pusher.adapter()));
        match client.session_mut(&a.nick) {
            Ok(store) => store.set_wake_chain(session, chain),
            Err(e) => {
                tracing::warn!(nick = %a.nick, error = %e, "m4a letter listener: session not in the opened store");
                std::thread::sleep(Duration::from_secs(60));
                continue;
            }
        }
        tracing::info!(nick = %a.nick, provider = %a.provider, "m4a letter listener up");
        while current(&a, gen) {
            let report = client.tick(now_ms(), 15);
            for (_, err) in &report.errors {
                tracing::debug!(nick = %a.nick, error = %err, "m4a listener drive error");
            }
            if client.error_streak() > 40 {
                tracing::warn!(nick = %a.nick, "m4a listener: homeserver keeps failing; reopening");
                break;
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    }
    tracing::info!(nick = %a.nick, "m4a letter listener replaced or stopped");
}

/// Per-session push queue: one worker thread, so at most one push into a session at a time.
struct LeaderPush {
    tx: std::sync::mpsc::Sender<String>,
}

impl LeaderPush {
    fn start(a: Adoption, leader: PathBuf) -> Self {
        let (tx, rx) = std::sync::mpsc::channel::<String>();
        let _ = std::thread::Builder::new().name(format!("m4a-push-{}", a.nick)).spawn(move || {
            let Ok(rt) = tokio::runtime::Builder::new_current_thread().enable_io().enable_time().build() else { return };
            let cwd = a.cwd.clone().unwrap_or_default();
            for prompt in rx {
                match rt.block_on(gate4agent_wake::grok_leader::push_into_session(&leader, &a.session, &cwd, &prompt)) {
                    Ok(()) => tracing::info!(nick = %a.nick, "letter pushed into the live session"),
                    Err(e) => tracing::warn!(nick = %a.nick, error = %e, "letter push into the live session failed (letter stays in the mailbox history)"),
                }
            }
        });
        Self { tx }
    }

    fn adapter(&self) -> PushAdapter {
        PushAdapter { tx: self.tx.clone() }
    }
}

struct PushAdapter {
    tx: std::sync::mpsc::Sender<String>,
}

impl mail4agent_messenger_shell::WakeAdapter for PushAdapter {
    fn kind(&self) -> mail4agent_messenger_shell::SessionKind {
        mail4agent_messenger_shell::SessionKind::local(mail4agent_messenger_shell::ProviderKind::Grok)
    }
    fn probe(&self, _s: &mail4agent_messenger_shell::ProviderSession) -> Result<(), mail4agent_messenger_shell::WakeError> {
        Ok(())
    }
    fn wake(&mut self, s: &mail4agent_messenger_shell::ProviderSession, l: &mail4agent_messenger_shell::WakeLetter<'_>) -> Result<mail4agent_messenger_shell::WakeOutcome, mail4agent_messenger_shell::WakeError> {
        let v = serde_json::json!({"from": l.from_nick, "event_id": l.event_id, "room": l.room, "body": l.body, "kind": if l.reaction.is_some() {"reaction"} else {"message"}, "reply_needed": l.reaction.is_none()});
        self.tx.send(letter_prompt(&s.nick, &v)).map_err(|_| mail4agent_messenger_shell::WakeError::Unavailable("push worker gone".into()))?;
        Ok(mail4agent_messenger_shell::WakeOutcome::Delivered)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn adoption(session: &str) -> Adoption {
        Adoption { provider: "grok".into(), session: session.into(), cwd: Some("w".into()), nick: "grok-1".into(), account_ref: "grok-ab".into(), store_root: Some("s".into()) }
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
    fn restart_with_saved_adoptions_starts_their_listeners() {
        let dir = std::env::temp_dir().join(format!("m4a-wake-restore-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let accounts = crate::m4a_accounts::M4aSessionAccounts::new("m4a-agent", &dir);
        assert_eq!(restore(&accounts), 0, "nothing saved");
        let mut a = adoption("s-restore");
        a.account_ref = "ref-restore".into();
        save_adoption(&dir, &a).unwrap();
        let mut b = adoption("s-nocwd");
        b.account_ref = "ref-nocwd".into();
        b.cwd = None;
        save_adoption(&dir, &b).unwrap();
        std::env::set_var("GROK_HOME", dir.join("grok"));
        assert_eq!(restore(&accounts), 1);
        assert!(is_watched("ref-restore"));
        assert!(!is_watched("ref-nocwd"));
        std::fs::write(dir.join(ADOPTED_FILE), b"not json").unwrap();
        assert_eq!(restore(&accounts), 0);
        GENERATIONS.lock().unwrap().remove("ref-restore");
        std::fs::remove_dir_all(&dir).unwrap();
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
        std::fs::write(dir.join("active_sessions.json"), r#"[{"session_id":"a","cwd":"w"},{"session_id":"b","cwd":""}]"#).unwrap();
        assert_eq!(grok_session_cwd(&dir, "a").as_deref(), Some("w"));
        assert_eq!(grok_session_cwd(&dir, "b"), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
