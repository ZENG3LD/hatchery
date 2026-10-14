//! Server configuration: an optional TOML file (`HATCHERY_SERVER_CONFIG`) with
//! environment variables taking precedence. Addresses, binary paths and key
//! variable names are never compiled in beyond loopback defaults; secrets are
//! never stored here, only the *names* of the variables that hold them.

use crate::error::ServerError;
use serde::Deserialize;
use std::net::SocketAddr;
use std::path::PathBuf;

pub const CONFIG_ENV: &str = "HATCHERY_SERVER_CONFIG";
pub const CONTROL_LISTEN_ENV: &str = "HATCHERY_SERVER_CONTROL_LISTEN";
pub const CONTROL_TOKEN_ENV: &str = "HATCHERY_SERVER_CONTROL_TOKEN";
pub const DATA_DIR_ENV: &str = "HATCHERY_SERVER_DATA_DIR";
pub const MEMORY_BIN_ENV: &str = "HATCHERY_MEMORY_BIN";
pub const M4A_MASTER_KEY_VAR_ENV: &str = "HATCHERY_M4A_MASTER_KEY_VAR";
/// C2 operator API the provisioner polls (`host:port`, reachable on loopback or a tunnel).
pub const PROVISION_C2_ADDR_ENV: &str = "HATCHERY_PROVISION_C2_ADDR";
/// Name of the variable holding the C2 operator bearer (the value is never in config).
pub const PROVISION_C2_TOKEN_VAR_ENV: &str = "HATCHERY_PROVISION_C2_TOKEN_VAR";
pub const PROVISION_INTERVAL_ENV: &str = "HATCHERY_PROVISION_INTERVAL_SECS";
pub const M4A_SERVER_REF_ENV: &str = "HATCHERY_M4A_SERVER_REF";
pub const MAIL_DOMAIN_ENV: &str = "HATCHERY_MAIL_DOMAIN";
/// Status -> capability policy (JSON), the same file C2 reads.
pub const POLICY_FILE_ENV: &str = "HATCHERY_POLICY_FILE";

/// Memory4agent listens only inside this range, on loopback.
pub const MEMORY_PORT_MIN: u16 = 18200;
pub const MEMORY_PORT_MAX: u16 = 18299;
const DEFAULT_CONTROL_LISTEN: &str = "127.0.0.1:18340";

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct FileConfig {
    pub control_listen: Option<String>,
    pub data_dir: Option<PathBuf>,
    pub memory: MemoryFile,
    pub m4a: M4aFile,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct MemoryFile {
    pub bin: Option<PathBuf>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct M4aFile {
    pub enabled: bool,
    /// Name of the environment variable that holds the vault master key.
    pub master_key_var: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServerConfig {
    pub control_listen: SocketAddr,
    pub control_token: Option<String>,
    pub data_dir: PathBuf,
    pub memory_bin: PathBuf,
    pub m4a_enabled: bool,
    pub m4a_master_key_var: Option<String>,
    /// Provisioning is on only when m4a is enabled and a C2 address is configured.
    pub provision_c2_addr: Option<String>,
    pub provision_c2_token_var: String,
    pub provision_interval_secs: u64,
    pub m4a_server_ref: String,
    pub mail_domain: String,
    pub policy_path: Option<PathBuf>,
}

impl ServerConfig {
    /// Build from the process environment (and the file it points at).
    pub fn from_env() -> Result<Self, ServerError> {
        let file = match std::env::var_os(CONFIG_ENV) {
            Some(path) => {
                let text = std::fs::read_to_string(&path)
                    .map_err(|e| ServerError::Config(format!("{CONFIG_ENV}: {e}")))?;
                toml::from_str::<FileConfig>(&text)
                    .map_err(|e| ServerError::Config(format!("{CONFIG_ENV}: {e}")))?
            }
            None => FileConfig::default(),
        };
        Self::resolve(file, |name| std::env::var(name).ok())
    }

    /// Pure resolution (tests inject the environment).
    pub fn resolve(
        file: FileConfig,
        env: impl Fn(&str) -> Option<String>,
    ) -> Result<Self, ServerError> {
        let listen = env(CONTROL_LISTEN_ENV)
            .or(file.control_listen)
            .unwrap_or_else(|| DEFAULT_CONTROL_LISTEN.to_owned());
        let control_listen: SocketAddr = listen
            .parse()
            .map_err(|_| ServerError::Config(format!("{CONTROL_LISTEN_ENV}: not a socket address")))?;
        if !control_listen.ip().is_loopback() {
            return Err(ServerError::Config(
                "control listener must be loopback; publish it through a tunnel, not a bind".into(),
            ));
        }
        let data_dir = env(DATA_DIR_ENV)
            .map(PathBuf::from)
            .or(file.data_dir)
            .ok_or_else(|| ServerError::Config(format!("{DATA_DIR_ENV} is required")))?;
        if !data_dir.is_absolute() {
            return Err(ServerError::Config(format!("{DATA_DIR_ENV} must be absolute")));
        }
        Ok(Self {
            control_listen,
            control_token: env(CONTROL_TOKEN_ENV).filter(|v| !v.is_empty()),
            data_dir,
            memory_bin: env(MEMORY_BIN_ENV)
                .map(PathBuf::from)
                .or(file.memory.bin)
                .unwrap_or_else(|| PathBuf::from("memory4agent-server")),
            m4a_enabled: file.m4a.enabled || env(M4A_MASTER_KEY_VAR_ENV).is_some(),
            m4a_master_key_var: env(M4A_MASTER_KEY_VAR_ENV).or(file.m4a.master_key_var),
            provision_c2_addr: env(PROVISION_C2_ADDR_ENV).filter(|v| !v.is_empty()),
            provision_c2_token_var: env(PROVISION_C2_TOKEN_VAR_ENV)
                .unwrap_or_else(|| "HATCHERY_PROVISION_C2_TOKEN".to_owned()),
            provision_interval_secs: env(PROVISION_INTERVAL_ENV).and_then(|v| v.parse().ok()).unwrap_or(5).max(1),
            m4a_server_ref: env(M4A_SERVER_REF_ENV).unwrap_or_else(|| "https://messenger.invalid".to_owned()),
            mail_domain: env(MAIL_DOMAIN_ENV).unwrap_or_else(|| "nodes.invalid".to_owned()),
            policy_path: env(POLICY_FILE_ENV).filter(|v| !v.is_empty()).map(PathBuf::from),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env_of(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> =
            pairs.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect();
        move |name| map.get(name).cloned()
    }

    #[test]
    fn data_dir_is_required_and_absolute() {
        assert!(ServerConfig::resolve(FileConfig::default(), env_of(&[])).is_err());
        assert!(ServerConfig::resolve(
            FileConfig::default(),
            env_of(&[(DATA_DIR_ENV, "relative")])
        )
        .is_err());
        let cfg = ServerConfig::resolve(
            FileConfig::default(),
            env_of(&[(DATA_DIR_ENV, "/var/lib/hatchery")]),
        )
        .unwrap();
        assert!(cfg.control_listen.ip().is_loopback());
        assert!(!cfg.m4a_enabled);
    }

    #[test]
    fn control_listener_must_be_loopback() {
        let err = ServerConfig::resolve(
            FileConfig::default(),
            env_of(&[(DATA_DIR_ENV, "/d"), (CONTROL_LISTEN_ENV, "0.0.0.0:18340")]),
        )
        .unwrap_err();
        assert!(err.to_string().contains("loopback"));
    }

    #[test]
    fn env_overrides_file_and_enables_subservices() {
        let file: FileConfig = toml::from_str(
            "data_dir = \"/from/file\"\n[m4a]\nmaster_key_var = \"FILE_VAR\"\n",
        )
        .unwrap();
        let cfg = ServerConfig::resolve(
            file,
            env_of(&[(DATA_DIR_ENV, "/from/env")]),
        )
        .unwrap();
        assert_eq!(cfg.data_dir, PathBuf::from("/from/env"));
        assert_eq!(cfg.m4a_master_key_var.as_deref(), Some("FILE_VAR"));
    }

    #[test]
    fn unknown_file_keys_are_rejected() {
        assert!(toml::from_str::<FileConfig>("bogus = 1").is_err());
    }
}
