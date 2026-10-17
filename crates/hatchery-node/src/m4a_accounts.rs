//! Per-session m4a accounts assigned by the node at spawn (owner model: session = nick = one key
//! = one role). Every PTY spawn gets its own account when the node can find `m4a-agent` (env
//! `HATCHERY_M4A_AGENT_BIN`, else next to this binary, PATH, or `~/.cargo/bin`) and an accounts
//! root (env `HATCHERY_M4A_ACCOUNTS_ROOT`, else `<enroll-state>/m4a-accounts`). Product URL comes
//! from env or the enrollment reply (`m4a_product_url`). The node runs `m4a-agent account provision`
//! (registry and vaults
//! stay under the node's accounts root, outside the session sandbox), reads the fresh capability
//! from a one-shot 0600 file it removes at once, and injects ONLY `M4A_NICK` and
//! `M4A_SESSION_CAP` into that session's child environment. The capability is never logged.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use gate4agent_catalog::EnvMutation;

/// g4a provider id -> the provider name `m4a-agent` accepts. g4a's built-in catalog is
/// `claude`, `codex`, `grok`, `kimi`; anything else (custom agents) gets no account.
pub fn m4a_provider(g4a_agent_id: &str) -> Option<&'static str> {
    match g4a_agent_id {
        "claude" => Some("claude-code"),
        "codex" => Some("codex"),
        "grok" => Some("grok"),
        "kimi" => Some("kimi-code"),
        _ => None,
    }
}

pub const AGENT_BIN_ENV: &str = "HATCHERY_M4A_AGENT_BIN";
/// Messenger product base URL; with it (and an enrolled node) the node registers its sessions'
/// accounts on the messenger itself, signed with the node key.
pub const PRODUCT_URL_ENV: &str = "HATCHERY_M4A_PRODUCT_URL";
/// The product's audience name the node signs for (default `m4a-product`).
pub const PRODUCT_AUDIENCE_ENV: &str = "HATCHERY_M4A_PRODUCT_AUDIENCE";

static NODE_SIGNER: std::sync::OnceLock<(String, std::sync::Arc<hatchery_enroll::NodeIdentity>)> = std::sync::OnceLock::new();

/// Installs this node's identity (enrolled, trusted) as the signer of account registrations.
/// An unenrolled node never has one, so it registers nothing.
pub fn set_node_signer(node_id: String, identity: std::sync::Arc<hatchery_enroll::NodeIdentity>) {
    let _ = NODE_SIGNER.set((node_id, identity));
}

/// Exact bytes the messenger product verifies (`m4a-node-account-v1`).
pub fn node_account_message(audience: &str, node_id: &str, nick: Option<&str>, ts_ms: i64, nonce: &str) -> Vec<u8> {
    format!("m4a-node-account-v1\n{audience}\n{node_id}\n{}\n{ts_ms}\n{nonce}\n", nick.unwrap_or("")).into_bytes()
}
pub const ACCOUNTS_ROOT_ENV: &str = "HATCHERY_M4A_ACCOUNTS_ROOT";

#[derive(Debug, Clone)]
pub struct M4aSessionAccounts {
    bin: PathBuf,
    base: PathBuf,
}

/// One session's account as handed to its child: nick and capability only.
pub struct SessionAccount {
    pub nick: String,
    /// The provider session key the account is bound to (also given to the session as
    /// `M4A_SESSION_ID`, so its own inbox commands address the right account).
    pub session_id: String,
    /// The account's own store root (`M4A_STORE_ROOT`), when m4a-agent reported it.
    pub store_root: Option<String>,
    /// Messenger product URL the session's client uses (`M4A_PRODUCT_URL`), from the node env.
    pub product_url: Option<String>,
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
    /// The one-time `/m4a/attach` reply (the capability travels only in this body).
    pub fn attach_reply(&self) -> serde_json::Value {
        serde_json::json!({"nick": self.nick, "session_id": self.session_id, "cap": self.cap, "store_root": self.store_root, "product_url": self.product_url})
    }

    pub fn env_mutations(&self) -> Vec<EnvMutation> {
        vec![
            EnvMutation { key: OsString::from("M4A_NICK"), value: Some(OsString::from(&self.nick)) },
            EnvMutation { key: OsString::from("M4A_SESSION_CAP"), value: Some(OsString::from(&self.cap)) },
            EnvMutation { key: OsString::from("M4A_SESSION_ID"), value: Some(OsString::from(&self.session_id)) },
            EnvMutation { key: OsString::from("M4A_BOT_NAME"), value: Some(OsString::from(&self.nick)) },
        ]
        .into_iter()
        .chain(self.store_root.iter().map(|r| EnvMutation { key: OsString::from("M4A_STORE_ROOT"), value: Some(OsString::from(r)) }))
        .chain(self.product_url.iter().map(|u| EnvMutation { key: OsString::from("M4A_PRODUCT_URL"), value: Some(OsString::from(u)) }))
        .collect()
    }
}

impl M4aSessionAccounts {
    /// The `m4a-agent` binary the node runs (also the command of the node's MCP entry).
    pub fn bin(&self) -> &Path {
        &self.bin
    }

    /// Fallback identity channel: `<root>/sessions/<nick>.session` (owner-only), the same three
    /// keys as [`SessionAccount::env_mutations`], for a provider whose tools run in a process
    /// that did not inherit the session environment (`m4a-agent --session-file <path> ...`).
    pub fn session_file_path(&self, nick: &str) -> Option<PathBuf> {
        let ok = !nick.is_empty() && nick.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
        ok.then(|| self.base.join("sessions").join(format!("{nick}.session")))
    }

    pub fn write_session_file(&self, account: &SessionAccount) -> Result<PathBuf, String> {
        let path = self.session_file_path(&account.nick).ok_or("invalid nick")?;
        let dir = path.parent().ok_or("no dir")?;
        std::fs::create_dir_all(dir).map_err(|e| format!("sessions dir: {}", e.kind()))?;
        let mut body = String::new();
        for m in account.env_mutations() {
            if let Some(v) = m.value {
                body.push_str(&format!("{}={}\n", m.key.to_string_lossy(), v.to_string_lossy()));
            }
        }
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        use std::io::Write;
        opts.open(&path)
            .and_then(|mut f| f.write_all(body.as_bytes()))
            .map_err(|e| format!("session file: {}", e.kind()))?;
        body.zeroize_best_effort();
        Ok(path)
    }

    /// Provider flags that give one spawned session its own m4a MCP server (`m4a-agent
    /// --session-file <file> mcp`), without touching any user config. Empty for providers with
    /// no verified per-session MCP flag (grok TUI, kimi PTY): those use attach (`/m4a/attach`).
    pub fn mcp_launch_args(&self, g4a_provider: &str, nick: &str) -> Result<Vec<OsString>, String> {
        let session_file = self.session_file_path(nick).ok_or("invalid nick")?;
        let bin = self.bin.to_string_lossy().into_owned();
        let sf = session_file.to_string_lossy().into_owned();
        let provider = g4a_provider.to_owned();
        match g4a_provider {
            "claude" => {
                let config = serde_json::json!({"mcpServers": {"m4a": {
                    "type": "stdio",
                    "command": bin,
                    "args": ["--session-file", sf, "mcp", "--provider", provider],
                }}});
                let path = self.base.join("sessions").join(format!("{nick}.mcp.json"));
                std::fs::create_dir_all(self.base.join("sessions")).map_err(|e| format!("sessions dir: {}", e.kind()))?;
                std::fs::write(&path, config.to_string()).map_err(|e| format!("mcp config: {}", e.kind()))?;
                Ok(vec![OsString::from("--mcp-config"), path.into_os_string()])
            }
            "codex" => {
                // TOML literal strings: no escapes, no double quotes. On Windows codex runs through
                // a `cmd /C <codex.cmd>` wrapper: an argument with whitespace gets quoted, and more
                // than two quotes make cmd strip the wrapper's own quotes (spawn fails). So the
                // values must be whitespace-free; a binary in a spaced directory is named by its
                // file name (found on PATH), otherwise the session falls back to attach.
                let bin = codex_safe_command(&self.bin).ok_or("m4a-agent path has whitespace and is not on PATH")?;
                if !codex_safe_value(&sf) {
                    return Err("session file path is not safe for codex -c".into());
                }
                Ok(vec![
                    OsString::from("-c"),
                    OsString::from(format!("mcp_servers.m4a.command='{bin}'")),
                    OsString::from("-c"),
                    OsString::from(format!("mcp_servers.m4a.args=['--session-file','{sf}','mcp','--provider','codex']")),
                ])
            }
            _ => Ok(Vec::new()),
        }
    }

    /// Registers the account on the messenger with this node's key, then the account's own client
    /// enrolls its own key (`m4a-agent account node-enroll`). The invite never reaches the node.
    fn node_enroll(&self, account: &SessionAccount) -> Result<(), String> {
        let url = account.product_url.as_deref().ok_or("no messenger product url (HATCHERY_M4A_PRODUCT_URL)")?;
        let store_root = account.store_root.as_deref().ok_or("m4a-agent reported no store root")?;
        let (node_id, identity) = NODE_SIGNER.get().ok_or("this node is not enrolled: it registers no accounts")?;
        let audience = std::env::var(PRODUCT_AUDIENCE_ENV).ok().filter(|a| !a.is_empty()).unwrap_or_else(|| "m4a-product".into());
        let ts_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0);
        let mut raw = [0u8; 16];
        getrandom(&mut raw)?;
        let nonce: String = raw.iter().map(|b| format!("{b:02x}")).collect();
        let signature = identity.sign_bytes(&node_account_message(&audience, node_id, Some(&account.nick), ts_ms, &nonce));
        let out = Command::new(&self.bin)
            .args(["account", "node-enroll", "--node-id", node_id, "--ts", &ts_ms.to_string(), "--nonce", &nonce, "--signature", &signature, "--nick", &account.nick])
            .env("M4A_PRODUCT_URL", url)
            .env("M4A_SESSION_ID", &account.session_id)
            .env("M4A_STORE_ROOT", store_root)
            .env("M4A_BOT_NAME", &account.nick)
            .env("M4A_ACCOUNTS_ROOT", &self.base)
            .env_remove("M4A_SESSION_CAP")
            .env_remove("M4A_PRODUCT_INVITE")
            .output()
            .map_err(|e| format!("m4a-agent: {}", e.kind()))?;
        if !out.status.success() {
            let why = String::from_utf8_lossy(&out.stderr).lines().last().unwrap_or("").chars().take(200).collect::<String>();
            return Err(format!("node-enroll failed: {why}"));
        }
        tracing::info!(nick = %account.nick, node_id = %node_id, "m4a account registered on the messenger by this node");
        Ok(())
    }

    pub fn remove_session_file(&self, nick: &str) {
        if let Some(path) = self.session_file_path(nick) {
            let _ = std::fs::remove_file(path);
        }
        let _ = std::fs::remove_file(self.base.join("sessions").join(format!("{nick}.mcp.json")));
    }

    pub fn new(bin: impl Into<PathBuf>, base: impl Into<PathBuf>) -> Self {
        Self { bin: bin.into(), base: base.into() }
    }

    /// The node's letter-wake token (`<root>/.node-wake-token`, 0600), created on first use.
    pub fn wake_token(&self) -> Result<String, String> {
        let path = self.base.join(".node-wake-token");
        if let Ok(t) = std::fs::read_to_string(&path) {
            if t.trim().len() >= 32 {
                return Ok(t.trim().to_owned());
            }
        }
        std::fs::create_dir_all(&self.base).map_err(|e| format!("accounts root: {}", e.kind()))?;
        let mut raw = [0u8; 24];
        getrandom(&mut raw)?;
        let token: String = raw.iter().map(|b| format!("{b:02x}")).collect();
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        use std::io::Write;
        opts.open(&path).and_then(|mut f| f.write_all(token.as_bytes())).map_err(|e| format!("wake token: {}", e.kind()))?;
        Ok(token)
    }

    /// Resolve `m4a-agent`: env override, then next to this binary, then PATH, then `~/.cargo/bin`.
    pub fn find_agent_bin() -> Option<PathBuf> {
        if let Some(p) = std::env::var_os(AGENT_BIN_ENV).filter(|v| !v.is_empty()).map(PathBuf::from) {
            return Some(p);
        }
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                for name in ["m4a-agent", "m4a-agent.exe"] {
                    let p = dir.join(name);
                    if p.is_file() {
                        return Some(p);
                    }
                }
            }
        }
        which("m4a-agent").or_else(|| which("m4a-agent.exe")).or_else(|| {
            let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
            let p = PathBuf::from(home).join(".cargo").join("bin").join(if cfg!(windows) { "m4a-agent.exe" } else { "m4a-agent" });
            p.is_file().then_some(p)
        })
    }

    /// Accounts root: env, else `<enroll_dir>/m4a-accounts`.
    pub fn default_accounts_root(enroll_dir: Option<&Path>) -> Option<PathBuf> {
        if let Some(p) = std::env::var_os(ACCOUNTS_ROOT_ENV).filter(|v| !v.is_empty()).map(PathBuf::from) {
            return Some(p);
        }
        enroll_dir.map(|d| d.join("m4a-accounts"))
    }

    /// Product URL: env override, else `m4a_product_url` from enrollment.json under enroll_dir.
    pub fn resolve_product_url(enroll_dir: Option<&Path>) -> Option<String> {
        if let Some(u) = std::env::var(PRODUCT_URL_ENV).ok().filter(|u| !u.is_empty()) {
            return Some(u);
        }
        let dir = enroll_dir?;
        let text = std::fs::read_to_string(dir.join("enrollment.json")).ok()?;
        let v: serde_json::Value = serde_json::from_str(&text).ok()?;
        v.get("response")
            .and_then(|r| r.get("m4a_product_url"))
            .or_else(|| v.get("m4a_product_url"))
            .and_then(|u| u.as_str())
            .filter(|u| !u.is_empty())
            .map(str::to_owned)
    }

    /// Open from env and defaults. Logs clearly when m4a cannot start.
    pub fn from_env() -> Option<Self> {
        Self::from_enroll_dir(enroll_dir_hint().as_deref())
    }

    pub fn from_enroll_dir(enroll_dir: Option<&Path>) -> Option<Self> {
        let bin = match Self::find_agent_bin() {
            Some(p) => p,
            None => {
                tracing::warn!(
                    "m4a-agent not found (set HATCHERY_M4A_AGENT_BIN, put it next to gate4agent-node, on PATH, or in ~/.cargo/bin); m4a off"
                );
                return None;
            }
        };
        let base = match Self::default_accounts_root(enroll_dir) {
            Some(p) => p,
            None => {
                tracing::warn!("m4a accounts root unknown (set HATCHERY_M4A_ACCOUNTS_ROOT or pass --enroll-state); m4a off");
                return None;
            }
        };
        if let Err(e) = std::fs::create_dir_all(&base) {
            tracing::warn!(error = %e.kind(), path = %base.display(), "m4a accounts root not writable; m4a off");
            return None;
        }
        let product = Self::resolve_product_url(enroll_dir);
        if product.is_none() {
            tracing::info!("m4a product url unset (HATCHERY_M4A_PRODUCT_URL or enrollment m4a_product_url); accounts stay local until set");
        }
        tracing::info!(bin = %bin.display(), accounts = %base.display(), product = product.as_deref().unwrap_or("-"), "m4a accounts ready");
        // Stash product url for SessionAccount::provision via env only when not already set.
        if let Some(url) = &product {
            if std::env::var_os(PRODUCT_URL_ENV).is_none() {
                std::env::set_var(PRODUCT_URL_ENV, url);
            }
        }
        Some(Self::new(bin, base))
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
        // mail4agent's own session env (`Provisioned::session_env`): the login session id is the
        // account ref and the account has its own store root.
        let session_id = v["account_ref"].as_str().filter(|r| !r.is_empty()).unwrap_or(session_key).to_owned();
        let store_root = v["store_root"].as_str().filter(|r| !r.is_empty()).map(str::to_owned);
        let product_url = std::env::var(PRODUCT_URL_ENV).ok().filter(|u| !u.is_empty());
        let account = SessionAccount { nick, session_id, store_root, product_url, cap };
        if !v["enrolled"].as_bool().unwrap_or(false) {
            if let Err(error) = self.node_enroll(&account) {
                tracing::warn!(nick = %account.nick, %error, "m4a account not registered on the messenger (stays local)");
            }
        }
        Ok(account)
    }
}

fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let p = dir.join(name);
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

fn enroll_dir_hint() -> Option<PathBuf> {
    std::env::var_os("HATCHERY_ENROLL_STATE")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            // Soft hint used when main has not set HATCHERY_ENROLL_STATE yet: none.
            None
        })
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
             umask 077; printf 'cap-%s-%s' \"$s\" \"$$\" > \"$o\"; printf '{\"nick\":\"codex-%s\",\"account_ref\":\"ref-%s\",\"store_root\":\"/s/%s\",\"enrolled\":true,\"cap_issued\":true}\\n' \"$s\" \"$s\" \"$s\"\n",
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p
    }

    #[test]
    #[test]
    fn defaults_find_agent_and_accounts_root_and_product_from_enrollment() {
        let dir = std::env::temp_dir().join(format!("m4a-defaults-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let agent = fake_agent(&dir);
        std::env::set_var(AGENT_BIN_ENV, &agent);
        std::env::remove_var(ACCOUNTS_ROOT_ENV);
        std::env::remove_var(PRODUCT_URL_ENV);
        let root = M4aSessionAccounts::default_accounts_root(Some(dir.as_path())).unwrap();
        assert_eq!(root, dir.join("m4a-accounts"));
        assert_eq!(M4aSessionAccounts::find_agent_bin().as_deref(), Some(agent.as_path()));
        std::fs::write(
            dir.join("enrollment.json"),
            r#"{"node_id":"n","response":{"node_id":"n","status":"approved","m4a_product_url":"https://example.invalid"}}"#,
        )
        .unwrap();
        assert_eq!(
            M4aSessionAccounts::resolve_product_url(Some(dir.as_path())).as_deref(),
            Some("https://example.invalid")
        );
        let got = M4aSessionAccounts::from_enroll_dir(Some(dir.as_path())).unwrap();
        assert_eq!(got.base(), root.as_path());
        std::env::remove_var(AGENT_BIN_ENV);
        std::env::remove_var(PRODUCT_URL_ENV);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn each_spawn_gets_its_own_account_and_its_own_session_env() {
        let dir = std::env::temp_dir().join(format!("hatch-m4a-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let acc = M4aSessionAccounts::new(fake_agent(&dir), dir.join("accounts"));
        let a = acc.provision("codex", "inc-1").unwrap();
        let b = acc.provision("codex", "inc-2").unwrap();
        assert_ne!(a.nick, b.nick);
        let env = a.env_mutations();
        let keys: Vec<_> = env.iter().map(|m| m.key.to_string_lossy().into_owned()).collect();
        assert_eq!(&keys[..5], ["M4A_NICK", "M4A_SESSION_CAP", "M4A_SESSION_ID", "M4A_BOT_NAME", "M4A_STORE_ROOT"]);
        assert_eq!(env[2].value.as_deref(), Some(std::ffi::OsStr::new("ref-inc-1")));
        assert_eq!(env[4].value.as_deref(), Some(std::ffi::OsStr::new("/s/inc-1")));
        assert_ne!(env[1].value, b.env_mutations()[1].value);
        // The one-shot cap file is gone and the cap is not in Debug output.
        let left: Vec<_> = std::fs::read_dir(acc.base()).unwrap().filter_map(Result::ok).filter(|e| e.file_name().to_string_lossy().ends_with(".cap")).collect();
        assert!(left.is_empty());
        assert!(!format!("{a:?}").contains("cap-"));
    }

    #[test]
    fn every_builtin_g4a_provider_maps_to_an_m4a_provider() {
        for spec in gate4agent_catalog::builtin_specs() {
            let id = spec.id.as_str().to_string();
            assert!(m4a_provider(&id).is_some(), "g4a provider {id} has no m4a mapping");
        }
        assert_eq!(m4a_provider("custom"), None);
    }

    #[cfg(unix)]
    #[test]
    fn wake_token_is_private_stable_and_long() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("node-wake-token-{}", std::process::id()));
        let a = M4aSessionAccounts::new("/bin/false", &dir);
        let t1 = a.wake_token().unwrap();
        assert_eq!(t1.len(), 48);
        assert_eq!(a.wake_token().unwrap(), t1);
        let mode = std::fs::metadata(dir.join(".node-wake-token")).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_failing_agent_injects_nothing() {
        let acc = M4aSessionAccounts::new("/nonexistent/m4a-agent", std::env::temp_dir().join("hatch-m4a-none"));
        assert!(acc.provision("codex", "x").is_err());
    }
}

fn codex_safe_value(v: &str) -> bool {
    !v.is_empty() && !v.chars().any(|c| c.is_whitespace() || matches!(c, '\'' | '"' | '%' | '!' | '^' | '&' | '|' | '<' | '>' | '(' | ')'))
}

/// The `command` value for codex: the full path when it is safe, else the bare file name when
/// the PATH lookup resolves to this same binary.
fn codex_safe_command(bin: &Path) -> Option<String> {
    let full = bin.to_string_lossy().into_owned();
    if codex_safe_value(&full) {
        return Some(full);
    }
    let name = bin.file_name()?.to_string_lossy().into_owned();
    if !codex_safe_value(&name) {
        return None;
    }
    let path = std::env::var_os("PATH")?;
    let dir = bin.parent()?;
    std::env::split_paths(&path)
        .any(|p| p == dir)
        .then(|| name.strip_suffix(".exe").map(str::to_owned).unwrap_or(name))
}

trait ZeroizeBestEffort {
    fn zeroize_best_effort(&mut self);
}

impl ZeroizeBestEffort for String {
    fn zeroize_best_effort(&mut self) {
        // SAFETY: zero bytes keep the buffer valid UTF-8.
        unsafe { self.as_bytes_mut() }.iter_mut().for_each(|b| *b = 0);
    }
}

#[cfg(test)]
mod session_file_tests {
    use super::*;

    #[test]
    fn mcp_args_are_session_scoped_and_carry_no_capability() {
        let root = std::env::temp_dir().join(format!("m4a-mcp-args-{}", std::process::id()));
        let a = M4aSessionAccounts::new("m4a-agent", &root);
        let c = a.mcp_launch_args("claude", "claude-1").unwrap();
        assert_eq!(c[0], OsString::from("--mcp-config"));
        let cfg = std::fs::read_to_string(&c[1]).unwrap();
        assert!(cfg.contains("--session-file") && cfg.contains("claude-1.session"));
        let x = a.mcp_launch_args("codex", "codex-1").unwrap();
        assert_eq!(x.len(), 4);
        assert!(x.iter().all(|v| !v.to_string_lossy().contains('"')));
        assert!(a.mcp_launch_args("grok", "grok-1").unwrap().is_empty());
        a.remove_session_file("claude-1");
        assert!(!std::path::Path::new(&c[1]).exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn codex_values_never_carry_whitespace_or_quotes() {
        assert!(codex_safe_value("/opt/m4a/bin/m4a-agent"));
        assert!(!codex_safe_value("/home/a b/m4a-agent"));
        assert!(!codex_safe_value("x'y"));
        assert_eq!(codex_safe_command(Path::new("/opt/m4a/m4a-agent")).as_deref(), Some("/opt/m4a/m4a-agent"));
        assert_eq!(codex_safe_command(Path::new("/no such dir/m4a-agent")), None);
    }

    #[test]
    fn session_file_path_refuses_path_like_nicks() {
        let a = M4aSessionAccounts::new("bin", std::env::temp_dir().join("m4a-sf-test"));
        assert!(a.session_file_path("grok-1").unwrap().ends_with("sessions/grok-1.session"));
        assert!(a.session_file_path("../x").is_none());
        assert!(a.session_file_path("").is_none());
    }
}

#[cfg(test)]
mod node_account_message_tests {
    #[test]
    fn signed_bytes_follow_the_product_contract() {
        let m = super::node_account_message("aud", "node-1", Some("nick-a"), 7, "n0");
        assert_eq!(m, b"m4a-node-account-v1\naud\nnode-1\nnick-a\n7\nn0\n".to_vec());
        assert_eq!(super::node_account_message("aud", "node-1", None, 7, "n0"), b"m4a-node-account-v1\naud\nnode-1\n\n7\nn0\n".to_vec());
    }
}
