//! Adoption of a provider session the node did not spawn: the node gives it a per-session m4a
//! account and puts the node's own m4a MCP server (`m4a-agent mcp`, stdio) in the provider's
//! config, removing a stale standalone mailbox MCP entry (a loopback HTTP url) with a backup.
//!
//! `POST /m4a/adopt?provider=<id>&session=<provider session id>[&stale=<entry name>]` (owner
//! token, same policy as `/m4a/attach`):
//! 1. provisions the account under the same key `/m4a/attach` uses for that session, so the
//!    MCP server's own attach later gets the same nick;
//! 2. records a pending adoption, so an attach whose caller carries no session id (an MCP child
//!    of a shared provider backend) is bound to this session and not to the backend's pid;
//! 3. for grok: rewrites `<grok home>/config.toml` (backup first) and returns the `mcpServers`
//!    entry an ACP `session/load` can carry, which grok reads when the session is (re)loaded.
//!
//! The MCP entry in the shared config names no account: each MCP process attaches by its own
//! ancestry and session, so every session gets its own nick from one config.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use toml_edit::{value, Array, DocumentMut, Item, Table};

pub const NODE_ENTRY: &str = "m4a";
pub const DEFAULT_STALE_ENTRY: &str = "mail4agent";
const PENDING_TTL: Duration = Duration::from_secs(30 * 60);

/// Is `url` a loopback HTTP endpoint (a standalone local mailbox daemon)?
pub fn is_loopback_http(url: &str) -> bool {
    let rest = url.strip_prefix("http://").or_else(|| url.strip_prefix("https://"));
    let Some(rest) = rest else { return false };
    let host = rest.split(['/', '?']).next().unwrap_or("");
    let host = if host.starts_with('[') { host.split(']').next().map(|h| &h[1..]).unwrap_or("") } else { host.rsplit_once(':').map_or(host, |(h, _)| h) };
    host == "localhost" || host == "::1" || host.starts_with("127.")
}

/// MCP server args the node's own entry uses (no account, no secret: identity comes from attach).
pub fn mcp_args(provider: &str, node_ops: &str, session: Option<&str>) -> Vec<String> {
    let mut a = vec!["mcp".to_owned(), "--provider".to_owned(), provider.to_owned(), "--node-ops".to_owned(), node_ops.to_owned()];
    if let Some(s) = session {
        a.extend(["--provider-session".to_owned(), s.to_owned()]);
    }
    a
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct ConfigEdit {
    pub removed: Vec<String>,
    pub added: bool,
    pub changed: bool,
}

/// Pure edit of a grok `config.toml`: drops `mcp_servers.<stale>` when it is a loopback HTTP
/// url, and sets `mcp_servers.m4a` to the node's stdio server. Other content is kept verbatim.
pub fn rewrite_grok_config(text: &str, stale: &str, bin: &str, args: &[String]) -> Result<(String, ConfigEdit), String> {
    let mut doc: DocumentMut = text.parse().map_err(|_| "config.toml is not valid toml".to_string())?;
    let mut edit = ConfigEdit::default();
    if doc.get("mcp_servers").is_none() {
        let mut t = Table::new();
        t.set_implicit(true);
        doc.insert("mcp_servers", Item::Table(t));
    }
    let servers = doc["mcp_servers"].as_table_mut().ok_or("mcp_servers is not a table")?;
    let stale_url = servers.get(stale).and_then(|e| e.get("url")).and_then(Item::as_str).map(str::to_owned);
    if stale != NODE_ENTRY && stale_url.as_deref().is_some_and(is_loopback_http) {
        servers.remove(stale);
        edit.removed.push(stale.to_owned());
    }
    let mut arr = Array::new();
    args.iter().for_each(|a| arr.push(a.as_str()));
    let same = servers.get(NODE_ENTRY).is_some_and(|e| {
        e.as_table_like().is_some_and(|t| t.iter().all(|(k, _)| k == "command" || k == "args"))
            && e.get("command").and_then(Item::as_str) == Some(bin)
            && e.get("args").and_then(Item::as_array).is_some_and(|x| x.iter().filter_map(|v| v.as_str()).eq(args.iter().map(String::as_str)))
    });
    if !same {
        let mut entry = Table::new();
        entry.insert("command", value(bin));
        entry.insert("args", value(arr));
        servers.insert(NODE_ENTRY, Item::Table(entry));
        edit.added = true;
    }
    edit.changed = edit.added || !edit.removed.is_empty();
    Ok((doc.to_string(), edit))
}

/// Applies [`rewrite_grok_config`] to a file: writes `<file>.bak-<unix secs>` first, then the new
/// text through a temp file and rename. A missing file is created (nothing to back up).
pub fn apply_grok_config(path: &Path, stale: &str, bin: &str, args: &[String]) -> Result<(ConfigEdit, Option<PathBuf>), String> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("read config: {}", e.kind())),
    };
    let (new_text, edit) = rewrite_grok_config(&text, stale, bin, args)?;
    if !edit.changed {
        return Ok((edit, None));
    }
    let mut backup = None;
    if path.exists() {
        let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let b = PathBuf::from(format!("{}.bak-{secs}", path.display()));
        std::fs::copy(path, &b).map_err(|e| format!("backup config: {}", e.kind()))?;
        backup = Some(b);
    } else if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("config dir: {}", e.kind()))?;
    }
    let tmp = PathBuf::from(format!("{}.m4a-tmp", path.display()));
    std::fs::write(&tmp, new_text).map_err(|e| format!("write config: {}", e.kind()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("replace config: {}", e.kind()))?;
    Ok((edit, backup))
}

/// `<grok home>/config.toml`: `GROK_HOME`, else `~/.grok`.
pub fn grok_config_path() -> Option<PathBuf> {
    if let Some(h) = std::env::var_os("GROK_HOME").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(h).join("config.toml"));
    }
    let home = std::env::var_os("USERPROFILE").filter(|v| !v.is_empty()).or_else(|| std::env::var_os("HOME"))?;
    Some(PathBuf::from(home).join(".grok").join("config.toml"))
}

/// Session id named on a provider command line (`--resume <id>`, `--session <id>`, `=` forms).
pub fn session_from_cmdline(args: &[String]) -> Option<String> {
    const FLAGS: &[&str] = &["--resume", "-r", "--session", "--session-id", "--load"];
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if let Some((k, v)) = a.split_once('=') {
            if FLAGS.contains(&k) && !v.is_empty() {
                return Some(v.to_owned());
            }
        } else if FLAGS.contains(&a.as_str()) {
            if let Some(v) = it.next().filter(|v| !v.starts_with('-')) {
                return Some(v.clone());
            }
        }
    }
    None
}

/// Command line of a live process (argv), best effort.
#[cfg(target_os = "linux")]
pub fn process_cmdline(pid: u32) -> Vec<String> {
    std::fs::read(format!("/proc/{pid}/cmdline"))
        .map(|b| b.split(|c| *c == 0).filter(|s| !s.is_empty()).map(|s| String::from_utf8_lossy(s).into_owned()).collect())
        .unwrap_or_default()
}

#[cfg(windows)]
pub fn process_cmdline(pid: u32) -> Vec<String> {
    use std::os::windows::process::CommandExt;
    let script = format!("(Get-CimInstance Win32_Process -Filter 'ProcessId={pid}').CommandLine");
    let out = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .creation_flags(0x0800_0000)
        .output();
    let Ok(out) = out else { return Vec::new() };
    split_windows_cmdline(String::from_utf8_lossy(&out.stdout).trim())
}

#[cfg(not(any(target_os = "linux", windows)))]
pub fn process_cmdline(_pid: u32) -> Vec<String> {
    Vec::new()
}

/// Minimal Windows command-line split (quotes group, backslashes kept).
pub fn split_windows_cmdline(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let (mut quoted, mut any) = (false, false);
    for c in s.chars() {
        match c {
            '"' => { quoted = !quoted; any = true; }
            c if c.is_whitespace() && !quoted => {
                if any { out.push(std::mem::take(&mut cur)); any = false; }
            }
            c => { cur.push(c); any = true; }
        }
    }
    if any { out.push(cur); }
    out
}

struct Pending {
    provider: String,
    session: String,
    at: Instant,
}

static PENDING: Mutex<Vec<Pending>> = Mutex::new(Vec::new());

pub fn add_pending(provider: &str, session: &str) {
    let mut p = PENDING.lock().unwrap_or_else(|e| e.into_inner());
    p.retain(|x| x.at.elapsed() < PENDING_TTL && !(x.provider == provider && x.session == session));
    p.push(Pending { provider: provider.to_owned(), session: session.to_owned(), at: Instant::now() });
}

/// Takes the oldest live pending adoption of `provider` (one attach binds one session).
pub fn claim_pending(provider: &str) -> Option<String> {
    let mut p = PENDING.lock().unwrap_or_else(|e| e.into_inner());
    p.retain(|x| x.at.elapsed() < PENDING_TTL);
    let i = p.iter().position(|x| x.provider == provider)?;
    Some(p.remove(i).session)
}

/// Session key resolution for an attach: explicit > provider command line > pending adoption.
pub fn resolve_session(explicit: Option<String>, provider: &str, provider_pid: u32, cmdline: impl Fn(u32) -> Vec<String>) -> Option<String> {
    resolve_session_in(explicit, provider, provider_pid, cmdline, None)
}

/// [`resolve_session`] with the node's saved adoptions: when the provider has exactly one
/// adopted session, an attach with no session of its own is that session (any number of
/// reloads, any time after the adoption, across node restarts). This keeps adopt and attach on
/// one account (`attach-<provider>-<session>`).
pub fn resolve_session_in(explicit: Option<String>, provider: &str, provider_pid: u32, cmdline: impl Fn(u32) -> Vec<String>, accounts_base: Option<&std::path::Path>) -> Option<String> {
    explicit
        .or_else(|| session_from_cmdline(&cmdline(provider_pid)))
        .or_else(|| {
            let sole = accounts_base.and_then(|b| crate::m4a_wake::sole_adoption(b, provider))?;
            let mut p = PENDING.lock().unwrap_or_else(|e| e.into_inner());
            p.retain(|x| !(x.provider == provider && x.session == sole));
            Some(sole)
        })
        .or_else(|| claim_pending(provider))
}

/// Handles `/m4a/adopt`.
pub fn handle(query: &str, accounts: Option<&crate::m4a_accounts::M4aSessionAccounts>, node_ops: Option<&str>, config_path: Option<PathBuf>) -> (u16, Value) {
    use crate::m4a_attach::{policy_allows, query_get, valid_session};
    let Some(provider) = query_get(query, "provider") else { return (400, json!({"error": "provider is required"})) };
    let Some(m4a_provider) = crate::m4a_accounts::m4a_provider(&provider) else { return (400, json!({"error": "unknown provider"})) };
    if let Err(reason) = policy_allows(&provider) {
        return (403, json!({"error": reason}));
    }
    let Some(session) = query_get(query, "session").filter(|s| valid_session(s)) else {
        return (400, json!({"error": "a valid provider session id is required"}));
    };
    let stale = query_get(query, "stale").unwrap_or_else(|| DEFAULT_STALE_ENTRY.to_owned());
    let Some(accounts) = accounts else { return (503, json!({"error": "m4a accounts are not configured on this node"})) };
    let Some(node_ops) = node_ops else { return (503, json!({"error": "node ops listener address unknown"})) };
    let account = match accounts.provision(m4a_provider, &format!("attach-{provider}-{session}")) {
        Ok(a) => a,
        Err(error) => {
            tracing::warn!(%provider, %error, "m4a adopt provision failed");
            return (500, json!({"error": "account provision failed"}));
        }
    };
    add_pending(&provider, &session);
    let bin = accounts.bin().to_string_lossy().into_owned();
    let mut reply = json!({"nick": account.nick, "session_id": account.session_id, "provider": provider});
    // Letter wake: the node listens on this account and presses the turn in the live session.
    let cwd = query_get(query, "cwd").filter(|c| !c.is_empty()).or_else(|| match provider.as_str() {
        "grok" => gate4agent_wake::grok_leader::grok_home().and_then(|h| crate::m4a_wake::grok_session_cwd(&h, &session)),
        _ => None,
    });
    let adoption = crate::m4a_wake::Adoption {
        provider: provider.clone(),
        session: session.clone(),
        cwd: cwd.clone(),
        nick: account.nick.clone(),
        account_ref: account.session_id.clone(),
        store_root: account.store_root.clone(),
    };
    if let Err(error) = crate::m4a_wake::save_adoption(accounts.base(), &adoption) {
        tracing::warn!(%error, "adoption not saved");
    }
    reply["wake"] = match &cwd {
        Some(_) => json!("listener started: letters to this nick press a turn in the session"),
        None => json!("no working directory known for this session (pass --cwd); letters stay in the inbox"),
    };
    if cwd.is_some() {
        let _ = crate::m4a_wake::watch(accounts, adoption);
    }
    match provider.as_str() {
        "grok" => {
            let shared_args = mcp_args(&provider, node_ops, None);
            let Some(path) = config_path else { return (500, json!({"error": "grok config path unknown"})) };
            match apply_grok_config(&path, &stale, &bin, &shared_args) {
                Ok((edit, backup)) => {
                    reply["config"] = json!({"path": path, "removed": edit.removed, "added": edit.added, "backup": backup});
                }
                Err(error) => return (500, json!({"error": error})),
            }
            // Per-session entry for an ACP `session/load` (carries the session id itself).
            let args = mcp_args(&provider, node_ops, Some(&session));
            reply["acp_mcp_server"] = json!({"name": NODE_ENTRY, "command": bin, "args": args, "env": []});
            reply["reload"] = json!("resume this session by id; grok reads MCP servers when a session loads");
        }
        _ => {
            reply["config"] = Value::Null;
            reply["reload"] = json!("no shared-config edit for this provider; its m4a-agent mcp attaches with --provider-session");
            reply["acp_mcp_server"] = json!({"name": NODE_ENTRY, "command": bin, "args": mcp_args(&provider, node_ops, Some(&session)), "env": []});
        }
    }
    tracing::info!(provider = %reply["provider"], nick = %account.nick, "live session adopted");
    (200, reply)
}

fn http(addr: &str, method: &str, path: &str, bearer: Option<&str>) -> Result<(u16, String), String> {
    use std::io::{Read, Write};
    let mut s = std::net::TcpStream::connect(addr).map_err(|e| format!("node unreachable: {}", e.kind()))?;
    let _ = s.set_read_timeout(Some(Duration::from_secs(60)));
    let auth = bearer.map(|t| format!("Authorization: Bearer {t}\r\n")).unwrap_or_default();
    let req = format!("{method} {path} HTTP/1.1\r\nHost: {addr}\r\n{auth}Content-Length: 0\r\nConnection: close\r\n\r\n");
    s.write_all(req.as_bytes()).map_err(|e| e.kind().to_string())?;
    let mut buf = String::new();
    s.read_to_string(&mut buf).map_err(|e| e.kind().to_string())?;
    let status = buf.split_whitespace().nth(1).and_then(|c| c.parse().ok()).ok_or("bad http reply")?;
    Ok((status, buf.split_once("\r\n\r\n").map(|(_, b)| b.to_owned()).unwrap_or_default()))
}

/// Percent-encodes one query value (spaces, backslashes, colons, non-ASCII).
pub fn query_escape(v: &str) -> String {
    v.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

/// Client side (`gate4agent-node m4a-adopt`): owner token from the file `/health` names.
pub fn client_adopt(node_ops: &str, query: &str) -> Result<String, String> {
    let (_, health) = http(node_ops, "GET", "/health", None)?;
    let health: Value = serde_json::from_str(&health).map_err(|_| "node health is not json")?;
    let file = health.get("control_token_file").and_then(Value::as_str).ok_or("node has no owner control token")?;
    let token = std::fs::read_to_string(file).map_err(|e| format!("owner token not readable: {}", e.kind()))?;
    let (status, body) = http(node_ops, "POST", &format!("/m4a/adopt?{query}"), Some(token.trim()))?;
    if status == 200 { Ok(body) } else { Err(format!("adopt refused ({status}): {body}")) }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CFG: &str = "model = \"x\"\n\n[mcp_servers.mail4agent]\nurl = \"http://127.0.0.1:18301/mcp\"\n\n[mcp_servers.other]\nurl = \"https://example.invalid/mcp\"\n";

    fn args() -> Vec<String> { mcp_args("grok", "127.0.0.1:1", None) }

    #[test]
    fn stale_loopback_entry_is_removed_and_node_entry_added() {
        let (out, e) = rewrite_grok_config(CFG, "mail4agent", "/bin/m4a-agent", &args()).unwrap();
        assert_eq!(e.removed, ["mail4agent"]);
        assert!(e.added && e.changed);
        assert!(!out.contains("18301"));
        assert!(out.contains("[mcp_servers.other]") && out.contains("model = \"x\""));
        let doc: DocumentMut = out.parse().unwrap();
        assert_eq!(doc["mcp_servers"]["m4a"]["command"].as_str(), Some("/bin/m4a-agent"));
        assert_eq!(doc["mcp_servers"]["m4a"]["args"].as_array().unwrap().len(), 5);
        let (_, again) = rewrite_grok_config(&out, "mail4agent", "/bin/m4a-agent", &args()).unwrap();
        assert!(!again.changed, "second run is a no-op");
    }

    #[test]
    fn existing_node_entry_is_normalized() {
        let cfg = "[mcp_servers.m4a]\ncommand = \"old\"\nargs = [\"mcp\", \"--provider-session\", \"x\"]\nenv = { A = \"1\" }\n";
        let (out, e) = rewrite_grok_config(cfg, "mail4agent", "m4a-agent", &args()).unwrap();
        assert!(e.changed && !out.contains("--provider-session") && !out.contains("env"));
        let (_, again) = rewrite_grok_config(&out, "mail4agent", "m4a-agent", &args()).unwrap();
        assert!(!again.changed);
    }

    #[test]
    fn remote_entry_with_stale_name_is_kept() {
        let cfg = "[mcp_servers.mail4agent]\nurl = \"https://example.invalid/mcp\"\n";
        let (out, e) = rewrite_grok_config(cfg, "mail4agent", "m4a-agent", &args()).unwrap();
        assert!(e.removed.is_empty() && out.contains("example.invalid"));
        let (out, _) = rewrite_grok_config("", "mail4agent", "m4a-agent", &args()).unwrap();
        assert!(out.contains("[mcp_servers.m4a]"));
    }

    #[test]
    fn apply_backs_up_then_replaces() {
        let dir = std::env::temp_dir().join(format!("m4a-adopt-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("config.toml");
        std::fs::write(&p, CFG).unwrap();
        let (e, b) = apply_grok_config(&p, "mail4agent", "m4a-agent", &args()).unwrap();
        assert!(e.changed);
        assert_eq!(std::fs::read_to_string(b.unwrap()).unwrap(), CFG);
        assert!(!std::fs::read_to_string(&p).unwrap().contains("18301"));
        let (e, b) = apply_grok_config(&p, "mail4agent", "m4a-agent", &args()).unwrap();
        assert!(!e.changed && b.is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn loopback_detection() {
        assert!(is_loopback_http("http://127.0.0.1:18301/mcp"));
        assert!(is_loopback_http("http://localhost:1/x"));
        assert!(is_loopback_http("http://[::1]:5/"));
        assert!(!is_loopback_http("https://example.invalid/mcp"));
        assert!(!is_loopback_http("m4a-agent"));
    }

    #[test]
    fn query_values_survive_spaces_and_backslashes() {
        let cwd = r"dir with space\sub:x";
        let q = format!("provider=grok&cwd={}", query_escape(cwd));
        assert!(!q.contains(' ') && !q.contains('\\'));
        assert_eq!(crate::m4a_attach::query_get(&q, "cwd").as_deref(), Some(cwd));
    }

    #[test]
    fn per_session_resolution() {
        let cl = |_| vec!["grok".to_string(), "--resume".into(), "s-1".into()];
        assert_eq!(resolve_session(Some("x".into()), "kimi", 1, cl).as_deref(), Some("x"));
        assert_eq!(resolve_session(None, "kimi", 1, cl).as_deref(), Some("s-1"));
        // Shared backend: no session on its command line -> each pending adoption binds once.
        let none = |_| vec!["grok".to_string(), "leader".into()];
        add_pending("claude", "a");
        add_pending("claude", "b");
        assert_eq!(resolve_session(None, "claude", 1, none).as_deref(), Some("a"));
        assert_eq!(resolve_session(None, "claude", 1, none).as_deref(), Some("b"));
        assert_eq!(resolve_session(None, "claude", 1, none), None);
        assert_eq!(session_from_cmdline(&["g".into(), "--session=z".into()]).as_deref(), Some("z"));
        assert_eq!(split_windows_cmdline(r#""tools dir\grok.exe" --resume 01a"#), vec![r"tools dir\grok.exe", "--resume", "01a"]);
    }
}
