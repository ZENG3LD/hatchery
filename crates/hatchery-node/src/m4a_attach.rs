//! `POST /m4a/attach`: on-demand m4a account for a provider session the node did not spawn
//! (spec live-session-m4a-attach).
//!
//! Caller: `m4a-agent mcp` running inside that provider session. Checks, all required:
//! 1. owner: the ops listener's owner-only control token (a file only the node's OS user can
//!    read) -- checked by the router before this module runs;
//! 2. policy: `HATCHERY_M4A_ATTACH=auto` (default `off`: nothing is adopted) and the provider in
//!    `HATCHERY_M4A_ATTACH_PROVIDERS` (default: all built-in providers);
//! 3. process: the given pid is alive and descends (within a few generations) from a live
//!    process of that provider.
//! Then the node provisions a per-session account (own registry entry, own store root and key)
//! keyed by the provider session id (or the provider's pid when none is known) and returns
//! nick, session id and a fresh capability. The capability is returned once, never logged.

use serde_json::{json, Value};

pub const POLICY_ENV: &str = "HATCHERY_M4A_ATTACH";
pub const PROVIDERS_ENV: &str = "HATCHERY_M4A_ATTACH_PROVIDERS";
const MAX_GENERATIONS: usize = 12;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proc {
    pub pid: u32,
    pub ppid: u32,
    pub name: String,
}

/// Process image base names that count as `provider`.
pub fn provider_images(provider: &str) -> &'static [&'static str] {
    match provider {
        "grok" => &["grok"],
        "codex" => &["codex"],
        "kimi" => &["kimi", "kimi-code"],
        "claude" => &["claude"],
        _ => &[],
    }
}

fn base_name(name: &str) -> String {
    let n = name.rsplit(['/', '\\']).next().unwrap_or(name).to_ascii_lowercase();
    n.strip_suffix(".exe").map(str::to_owned).unwrap_or(n)
}

/// Walks from `pid` up the parent chain; returns the nearest process whose image is the provider.
pub fn provider_ancestor(table: &[Proc], pid: u32, provider: &str) -> Option<u32> {
    let images = provider_images(provider);
    let mut current = pid;
    for _ in 0..MAX_GENERATIONS {
        let p = table.iter().find(|p| p.pid == current)?;
        let name = base_name(&p.name);
        if images.iter().any(|i| name == *i || name.starts_with(&format!("{i}-"))) {
            return Some(p.pid);
        }
        if p.ppid == 0 || p.ppid == p.pid {
            return None;
        }
        current = p.ppid;
    }
    None
}

pub fn policy_allows(provider: &str) -> Result<(), &'static str> {
    let mode = std::env::var(POLICY_ENV).unwrap_or_default().trim().to_ascii_lowercase();
    if mode != "auto" {
        return Err("adoption policy is off on this node");
    }
    if let Ok(list) = std::env::var(PROVIDERS_ENV) {
        if !list.split(',').map(str::trim).any(|p| p == provider) {
            return Err("provider is not allowed by the adoption policy");
        }
    }
    Ok(())
}

pub fn query_get(query: &str, key: &str) -> Option<String> {
    query.split('&').find_map(|kv| {
        let (k, v) = kv.split_once('=')?;
        (k == key).then(|| percent_decode(v))
    })
}

fn percent_decode(v: &str) -> String {
    let b = v.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(x) = u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or(""), 16) {
                out.push(x);
                i += 3;
                continue;
            }
        }
        out.push(if b[i] == b'+' { b' ' } else { b[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn valid_session(s: &str) -> bool {
    !s.is_empty() && s.len() <= 128 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.:".contains(&b))
}

/// Handles the request; `table` is the current process table (injected for tests).
pub fn handle(
    query: &str,
    accounts: Option<&crate::m4a_accounts::M4aSessionAccounts>,
    table: &[Proc],
) -> (u16, Value) {
    let Some(provider) = query_get(query, "provider") else {
        return (400, json!({"error": "provider is required"}));
    };
    let Some(m4a_provider) = crate::m4a_accounts::m4a_provider(&provider) else {
        return (400, json!({"error": "unknown provider"}));
    };
    if let Err(reason) = policy_allows(&provider) {
        tracing::info!(%provider, "m4a attach refused by policy");
        return (403, json!({"error": reason, "state": "pending"}));
    }
    let Some(pid) = query_get(query, "pid").and_then(|p| p.parse::<u32>().ok()) else {
        return (400, json!({"error": "pid is required"}));
    };
    let Some(root) = provider_ancestor(table, pid, &provider) else {
        tracing::warn!(%provider, pid, "m4a attach refused: caller is not inside a live provider process");
        return (403, json!({"error": "caller is not inside a live process of that provider"}));
    };
    let session = query_get(query, "session");
    if session.as_deref().is_some_and(|s| !valid_session(s)) {
        return (400, json!({"error": "invalid session id"}));
    }
    let Some(accounts) = accounts else {
        return (503, json!({"error": "m4a accounts are not configured on this node"}));
    };
    // Per-session binding: an MCP child of a shared backend has no session of its own, so the
    // node reads it from the provider's command line or takes a pending adoption.
    let session = crate::m4a_adopt::resolve_session(session, &provider, root, crate::m4a_adopt::process_cmdline)
        .filter(|s| valid_session(s));
    let key = match &session {
        Some(s) => format!("attach-{provider}-{s}"),
        None => format!("attach-{provider}-pid{root}"),
    };
    match accounts.provision(m4a_provider, &key) {
        Ok(account) => {
            tracing::info!(%provider, provider_pid = root, nick = %account.nick, "m4a account attached to a live session");
            (200, account.attach_reply())
        }
        Err(error) => {
            tracing::warn!(%provider, %error, "m4a attach provision failed");
            (500, json!({"error": "account provision failed"}))
        }
    }
}

/// The live process table.
#[cfg(target_os = "linux")]
pub fn process_table() -> Vec<Proc> {
    let mut out = Vec::new();
    let Ok(dir) = std::fs::read_dir("/proc") else { return out };
    for e in dir.flatten() {
        let Some(pid) = e.file_name().to_str().and_then(|s| s.parse::<u32>().ok()) else { continue };
        let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else { continue };
        let (Some(l), Some(r)) = (stat.find('('), stat.rfind(')')) else { continue };
        let ppid = stat[r + 1..].split_whitespace().nth(1).and_then(|s| s.parse().ok()).unwrap_or(0);
        // comm is truncated to 15 bytes; prefer the executable name.
        let name = std::fs::read_link(format!("/proc/{pid}/exe"))
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_else(|| stat[l + 1..r].to_owned());
        out.push(Proc { pid, ppid, name });
    }
    out
}

#[cfg(windows)]
pub fn process_table() -> Vec<Proc> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
    };
    let mut out = Vec::new();
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE {
            return out;
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut ok = Process32FirstW(snap, &mut entry) != 0;
        while ok {
            let len = entry.szExeFile.iter().position(|c| *c == 0).unwrap_or(entry.szExeFile.len());
            out.push(Proc {
                pid: entry.th32ProcessID,
                ppid: entry.th32ParentProcessID,
                name: String::from_utf16_lossy(&entry.szExeFile[..len]),
            });
            ok = Process32NextW(snap, &mut entry) != 0;
        }
        CloseHandle(snap);
    }
    out
}

#[cfg(not(any(target_os = "linux", windows)))]
pub fn process_table() -> Vec<Proc> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> Vec<Proc> {
        vec![
            Proc { pid: 1, ppid: 0, name: "init".into() },
            Proc { pid: 10, ppid: 1, name: "Grok.EXE".into() },
            Proc { pid: 11, ppid: 10, name: "cmd.exe".into() },
            Proc { pid: 12, ppid: 11, name: "m4a-agent.exe".into() },
            Proc { pid: 20, ppid: 1, name: "bash".into() },
            Proc { pid: 21, ppid: 20, name: "m4a-agent".into() },
        ]
    }

    #[test]
    fn ancestry_finds_the_provider_and_refuses_strangers() {
        let t = table();
        assert_eq!(provider_ancestor(&t, 12, "grok"), Some(10));
        assert_eq!(provider_ancestor(&t, 12, "codex"), None);
        assert_eq!(provider_ancestor(&t, 21, "grok"), None);
        assert_eq!(provider_ancestor(&t, 999, "grok"), None);
    }

    #[test]
    fn query_parsing_decodes_and_validates() {
        assert_eq!(query_get("provider=grok&session=a%3Ab", "session").as_deref(), Some("a:b"));
        assert!(valid_session("0aa-b_c.d:e"));
        assert!(!valid_session("../x"));
        assert!(!valid_session(""));
    }

    // One test owns the policy env var (tests in this binary run in parallel).
    #[test]
    fn policy_and_process_gates_come_before_any_account() {
        std::env::remove_var(POLICY_ENV);
        let (s, v) = handle("provider=grok&pid=12", None, &table());
        assert_eq!((s, v["state"].as_str()), (403, Some("pending")));
        std::env::set_var(POLICY_ENV, "auto");
        std::env::set_var(PROVIDERS_ENV, "codex");
        assert_eq!(handle("provider=grok&pid=12", None, &table()).0, 403);
        std::env::remove_var(PROVIDERS_ENV);
        assert_eq!(handle("provider=grok&pid=21", None, &table()).0, 403);
        assert_eq!(handle("provider=grok&pid=12&session=../x", None, &table()).0, 400);
        assert_eq!(handle("provider=grok&pid=12", None, &table()).0, 503);
        assert_eq!(handle("provider=nope&pid=12", None, &table()).0, 400);
        std::env::remove_var(POLICY_ENV);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn live_table_contains_this_process() {
        let me = std::process::id();
        assert!(process_table().iter().any(|p| p.pid == me));
    }
}
