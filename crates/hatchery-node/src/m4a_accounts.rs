//! Per-session m4a accounts assigned by the node at spawn (owner model: session = nick = one key
//! = one role). When `HATCHERY_M4A_AGENT_BIN` and `HATCHERY_M4A_ACCOUNTS_ROOT` are set, every PTY
//! spawn gets its own account: the node runs `m4a-agent account provision` (registry and vaults
//! stay under the node's accounts root, outside the session sandbox), reads the fresh capability
//! from a one-shot 0600 file it removes at once, and injects ONLY `M4A_NICK` and
//! `M4A_SESSION_CAP` into that session's child environment. The capability is never logged.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use gate4agent_catalog::EnvMutation;

pub const AGENT_BIN_ENV: &str = "HATCHERY_M4A_AGENT_BIN";
pub const ACCOUNTS_ROOT_ENV: &str = "HATCHERY_M4A_ACCOUNTS_ROOT";

#[derive(Debug, Clone)]
pub struct M4aSessionAccounts {
    bin: PathBuf,
    base: PathBuf,
}

/// One session's account as handed to its child: nick and capability only.
pub struct SessionAccount {
    pub nick: String,
    cap: String,
}

impl Drop for SessionAccount {
    fn drop(&mut self) {
        // Best effort wipe of the capability copy held by the node.
        let bytes = unsafe { self.cap.as_bytes_mut() };
        bytes.iter_mut().for_each(|b| *b = 0);
    }
}

impl std::fmt::Debug for SessionAccount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionAccount").field("nick", &self.nick).finish_non_exhaustive()
    }
}

impl SessionAccount {
    /// The only variables the session gets from the node for m4a.
    pub fn env_mutations(&self) -> Vec<EnvMutation> {
        vec![
            EnvMutation { key: OsString::from("M4A_NICK"), value: Some(OsString::from(&self.nick)) },
            EnvMutation { key: OsString::from("M4A_SESSION_CAP"), value: Some(OsString::from(&self.cap)) },
        ]
    }
}

impl M4aSessionAccounts {
    pub fn new(bin: impl Into<PathBuf>, base: impl Into<PathBuf>) -> Self {
        Self { bin: bin.into(), base: base.into() }
    }

    pub fn from_env() -> Option<Self> {
        let get = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
        Some(Self::new(get(AGENT_BIN_ENV)?, get(ACCOUNTS_ROOT_ENV)?))
    }

    pub fn base(&self) -> &Path {
        &self.base
    }

    /// The account of `(provider, session_key)`, created if missing, with a fresh capability
    /// (a respawn of the same session invalidates the previous process's key).
    pub fn provision(&self, provider: &str, session_key: &str) -> Result<SessionAccount, String> {
        std::fs::create_dir_all(&self.base).map_err(|e| format!("accounts root: {}", e.kind()))?;
        let mut rand = [0u8; 12];
        getrandom(&mut rand)?;
        let tag: String = rand.iter().map(|b| format!("{b:02x}")).collect();
        let cap_file = self.base.join(format!(".spawn-{tag}.cap"));
        let out = Command::new(&self.bin)
            .args(["account", "provision", "--provider", provider, "--session", session_key, "--rotate", "--cap-out"])
            .arg(&cap_file)
            .env("M4A_ACCOUNTS_ROOT", &self.base)
            .env_remove("M4A_SESSION_CAP")
            .env_remove("M4A_PRODUCT_INVITE")
            .output()
            .map_err(|e| format!("m4a-agent: {}", e.kind()))?;
        let cap = std::fs::read_to_string(&cap_file);
        let _ = std::fs::remove_file(&cap_file);
        if !out.status.success() {
            return Err(format!("m4a-agent account provision failed ({})", out.status.code().unwrap_or(-1)));
        }
        let v: serde_json::Value = serde_json::from_slice(&out.stdout).map_err(|_| "m4a-agent answer is not json".to_string())?;
        let nick = v["nick"].as_str().ok_or("m4a-agent answer has no nick")?.to_string();
        let cap = cap.map_err(|_| "no capability issued".to_string())?;
        if cap.trim().is_empty() || cap.contains('\n') {
            return Err("capability is malformed".into());
        }
        Ok(SessionAccount { nick, cap })
    }
}

fn getrandom(buf: &mut [u8]) -> Result<(), String> {
    use std::io::Read;
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(buf))
        .or_else(|_| {
            // Non-unix fallback: time + address entropy is enough for a unique temp name.
            let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
            for (i, b) in buf.iter_mut().enumerate() {
                *b = (t >> ((i % 16) * 8)) as u8 ^ (i as u8);
            }
            Ok::<(), std::io::Error>(())
        })
        .map_err(|e| e.to_string())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn fake_agent(dir: &Path) -> PathBuf {
        // Stands in for m4a-agent: one nick per --session, the cap written to --cap-out.
        let p = dir.join("fake-m4a-agent");
        std::fs::write(
            &p,
            "#!/bin/sh\nwhile [ $# -gt 0 ]; do case \"$1\" in --session) s=$2; shift;; --cap-out) o=$2; shift;; esac; shift; done\n\
             umask 077; printf 'cap-%s-%s' \"$s\" \"$$\" > \"$o\"; printf '{\"nick\":\"codex-%s\",\"cap_issued\":true}\\n' \"$s\"\n",
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p
    }

    #[test]
    fn each_spawn_gets_its_own_account_and_only_nick_and_cap_are_injected() {
        let dir = std::env::temp_dir().join(format!("hatch-m4a-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let acc = M4aSessionAccounts::new(fake_agent(&dir), dir.join("accounts"));
        let a = acc.provision("codex", "inc-1").unwrap();
        let b = acc.provision("codex", "inc-2").unwrap();
        assert_ne!(a.nick, b.nick);
        let env = a.env_mutations();
        let keys: Vec<_> = env.iter().map(|m| m.key.to_string_lossy().into_owned()).collect();
        assert_eq!(keys, ["M4A_NICK", "M4A_SESSION_CAP"]);
        assert_ne!(env[1].value, b.env_mutations()[1].value);
        // The one-shot cap file is gone and the cap is not in Debug output.
        let left: Vec<_> = std::fs::read_dir(acc.base()).unwrap().filter_map(Result::ok).filter(|e| e.file_name().to_string_lossy().ends_with(".cap")).collect();
        assert!(left.is_empty());
        assert!(!format!("{a:?}").contains("cap-"));
    }

    #[test]
    fn a_failing_agent_injects_nothing() {
        let acc = M4aSessionAccounts::new("/nonexistent/m4a-agent", std::env::temp_dir().join("hatch-m4a-none"));
        assert!(acc.provision("codex", "x").is_err());
    }
}
