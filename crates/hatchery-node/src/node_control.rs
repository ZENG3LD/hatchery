//! Local control channel of this node (`POST /control/{drain,reload,handoff}`
//! on the ops HTTP listener). Owner-only: a random bearer token written 0600
//! (Unix) into the node's own state directory, a per-user location on Windows.
//! `/health` publishes the token file path and the capabilities, never the token.
//!
//! - drain: mark the node draining (reported in `/health`); new work is the
//!   caller's to stop sending. `?draining=false` clears it.
//! - reload: no hot-reloadable config yet; answers what needs a handoff.
//! - handoff: drain, answer 202, then after `release_after_ms` stop the ops
//!   listener, which runs the node's normal graceful shutdown (its own PTY
//!   sessions stop; nothing it did not spawn is touched) and frees every port
//!   for the successor.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::time::Duration;

pub const CAPABILITIES: [&str; 3] = ["control.drain", "control.reload", "control.handoff"];
const MAX_RELEASE_AFTER_MS: u64 = 10_000;

struct Installed {
    token: String,
    file: PathBuf,
}

static INSTALLED: OnceLock<Installed> = OnceLock::new();
static DRAINING: AtomicBool = AtomicBool::new(false);
static HANDOFF: OnceLock<tokio::sync::Notify> = OnceLock::new();

fn handoff_notify() -> &'static tokio::sync::Notify {
    HANDOFF.get_or_init(tokio::sync::Notify::new)
}

/// Token file beside the node state.
pub fn token_file_for(state_path: &Path) -> PathBuf {
    state_path.parent().map_or_else(|| PathBuf::from("control.token"), |d| d.join("control.token"))
}

/// Create the token and write it owner-only. Call once at start.
pub fn install(file: PathBuf) -> std::io::Result<()> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| std::io::Error::other(e.to_string()))?;
    let token: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    write_owner_only(&file, token.as_bytes())?;
    let _ = INSTALLED.set(Installed { token, file });
    Ok(())
}

pub fn write_owner_only(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("token.tmp");
    let mut options = std::fs::OpenOptions::new();
    options.create(true).write(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&tmp)?;
    file.write_all(contents)?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(&tmp, path)
}

pub fn installed() -> bool {
    INSTALLED.get().is_some()
}

pub fn token_file() -> Option<String> {
    INSTALLED.get().map(|i| i.file.display().to_string())
}

pub fn is_draining() -> bool {
    DRAINING.load(Ordering::SeqCst)
}

pub fn authorized(header: Option<&str>) -> bool {
    let (Some(installed), Some(value)) = (INSTALLED.get(), header) else { return false };
    let Some(given) = value.strip_prefix("Bearer ") else { return false };
    given.len() == installed.token.len()
        && given.bytes().zip(installed.token.bytes()).fold(0u8, |a, (x, y)| a | (x ^ y)) == 0
}

/// Resolves when a handoff asked the ops listener to stop.
pub async fn handoff_requested() {
    handoff_notify().notified().await;
}

/// Execute a verb. Returns (status, JSON body).
pub fn execute(verb: &str, query: &str) -> (u16, serde_json::Value) {
    let param = |name: &str| {
        query.split('&').find_map(|kv| kv.split_once('=').filter(|(k, _)| *k == name).map(|(_, v)| v.to_owned()))
    };
    match verb {
        "drain" => {
            let on = param("draining").map_or(true, |v| v != "false");
            DRAINING.store(on, Ordering::SeqCst);
            tracing::info!(draining = on, "control: drain");
            (200, serde_json::json!({"ok": true, "draining": on}))
        }
        "reload" => (200, serde_json::json!({"ok": true, "applied": [], "needs_handoff_for": ["flags", "env"]})),
        "handoff" => {
            let after = param("release_after_ms").and_then(|v| v.parse().ok()).unwrap_or(300u64).min(MAX_RELEASE_AFTER_MS);
            DRAINING.store(true, Ordering::SeqCst);
            tracing::info!(release_after_ms = after, "control: handoff accepted; node will shut down gracefully");
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(after)).await;
                handoff_notify().notify_waiters();
                handoff_notify().notify_one();
            });
            (202, serde_json::json!({"ok": true, "release_after_ms": after}))
        }
        _ => (404, serde_json::json!({"error": "unknown control verb"})),
    }
}

/// Remove our token file at exit if it is still ours.
pub fn cleanup() {
    if let Some(i) = INSTALLED.get() {
        if std::fs::read_to_string(&i.file).is_ok_and(|t| t.trim() == i.token) {
            let _ = std::fs::remove_file(&i.file);
        }
    }
}
