//! Per-session grok leader isolation.
//!
//! grok's interactive UI may attach to a shared, long-lived "leader" backend (`use_leader` in
//! its config). The leader then runs every tool call with *its own* process environment, so
//! the node's per-session environment (for example the m4a session identity) never reaches
//! the tools. A node-spawned grok session therefore gets its own `--leader-socket`: the backend
//! it starts is that session's alone and inherits that session's environment. The grok home
//! and its login are unchanged.
//!
//! The leader outlives its UI by design, so the node retires it when the session ends: it
//! stops only processes whose command line carries this session's unique socket tag. Another
//! leader (an operator's shared one included) never carries that tag and is never touched.

use std::ffi::OsString;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

/// Set to `0` / `off` to let node-spawned grok sessions use grok's own leader configuration.
pub const SESSION_LEADER_ENV: &str = "HATCHERY_GROK_SESSION_LEADER";

/// File-name prefix of every per-session socket; the tag the retire step matches on.
pub const SOCKET_PREFIX: &str = "hatchery-grok-leader-";

pub fn disabled() -> bool {
    std::env::var(SESSION_LEADER_ENV)
        .map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "0" | "off" | "false" | "no"))
        .unwrap_or(false)
}

/// Short, stable tag for one session key (keeps Unix socket paths well under their limit).
pub fn session_tag(session_key: &str) -> String {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    session_key.hash(&mut h);
    format!("{SOCKET_PREFIX}{:016x}", h.finish())
}

/// `<root>/grok-leaders/<tag>.sock`; the directory is created best effort.
pub fn socket_path(root: &Path, session_key: &str) -> PathBuf {
    let dir = root.join("grok-leaders");
    let _ = std::fs::create_dir_all(&dir);
    dir.join(format!("{}.sock", session_tag(session_key)))
}

pub fn leader_socket_args(socket: &Path) -> Vec<OsString> {
    vec![OsString::from("--leader-socket"), socket.as_os_str().to_owned()]
}

/// The tag `retire` matches on, or `None` for a path that is not a per-session socket (so a
/// foreign path can never widen the match).
pub fn retire_tag(socket: &Path) -> Option<String> {
    let stem = socket.file_stem()?.to_str()?;
    let hex = stem.strip_prefix(SOCKET_PREFIX)?;
    (hex.len() == 16 && hex.bytes().all(|b| b.is_ascii_hexdigit())).then(|| stem.to_owned())
}

/// Stops this session's own leader (and only it), then removes its socket file.
pub fn retire(socket: &Path) {
    let Some(tag) = retire_tag(socket) else { return };
    let result = stop_tagged_processes(&tag);
    if let Err(error) = result {
        tracing::warn!(%error, "per-session grok leader retire failed");
    } else {
        tracing::info!("per-session grok leader retired");
    }
    let _ = std::fs::remove_file(socket);
}

#[cfg(windows)]
fn stop_tagged_processes(tag: &str) -> Result<(), String> {
    // `tag` is validated `[A-Za-z0-9-]` only, so it is inert inside the filter string.
    let script = format!(
        "Get-CimInstance Win32_Process | Where-Object {{ $_.CommandLine -and $_.CommandLine.Contains('{tag}') -and $_.ProcessId -ne $PID }} | ForEach-Object {{ Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }}"
    );
    let status = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|e| e.kind().to_string())?;
    status.success().then_some(()).ok_or_else(|| format!("powershell exit {status}"))
}

#[cfg(not(windows))]
fn stop_tagged_processes(tag: &str) -> Result<(), String> {
    let status = std::process::Command::new("pkill")
        .args(["-TERM", "-f", tag])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|e| e.kind().to_string())?;
    // pkill: 0 = signalled, 1 = nothing matched; both are fine.
    matches!(status.code(), Some(0 | 1)).then_some(()).ok_or_else(|| format!("pkill exit {status}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn socket_is_per_session_and_tagged() {
        let root = std::env::temp_dir().join("hatchery-grok-leader-test");
        let a = socket_path(&root, "inc-1");
        let b = socket_path(&root, "inc-2");
        assert_ne!(a, b);
        assert!(a.starts_with(root.join("grok-leaders")));
        assert!(retire_tag(&a).is_some());
        assert_eq!(leader_socket_args(&a)[0], OsString::from("--leader-socket"));
    }

    #[test]
    fn retire_refuses_foreign_sockets() {
        assert!(retire_tag(Path::new("/home/user/.grok/leader.sock")).is_none());
        assert!(retire_tag(Path::new("/tmp/hatchery-grok-leader-zz.sock")).is_none());
        assert!(retire_tag(Path::new("/tmp/hatchery-grok-leader-0123456789abcdef'x.sock")).is_none());
    }

    #[cfg(unix)]
    #[test]
    fn retire_stops_only_the_tagged_process() {
        let tag = session_tag("retire-test");
        let mut mine = std::process::Command::new("sh")
            .args(["-c", &format!("sleep 30; : {tag}")])
            .spawn()
            .unwrap();
        let mut other = std::process::Command::new("sleep").arg("30").spawn().unwrap();
        retire(&std::env::temp_dir().join(format!("{tag}.sock")));
        let mut stopped = false;
        for _ in 0..50 {
            if mine.try_wait().unwrap().is_some() {
                stopped = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        assert!(stopped, "tagged process must be stopped");
        assert!(other.try_wait().unwrap().is_none());
        let _ = other.kill();
        let _ = other.wait();
    }
}
