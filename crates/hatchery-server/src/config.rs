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
pub const HARNESS_BIN_ENV: &str = "HATCHERY_HARNESS_BIN";
pub const HARNESS_ARGS_ENV: &str = "HATCHERY_HARNESS_ARGS";
pub const MEMORY_BIN_ENV: &str = "HATCHERY_MEMORY_BIN";
pub const M4A_MASTER_KEY_VAR_ENV: &str = "HATCHERY_M4A_MASTER_KEY_VAR";

/// Memory4agent listens only inside this range, on loopback.
pub const MEMORY_PORT_MIN: u16 = 18200;
pub const MEMORY_PORT_MAX: u16 = 18299;
const DEFAULT_CONTROL_LISTEN: &str = "127.0.0.1:18340";

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct FileConfig {
    pub control_listen: Option<String>,
    pub data_dir: Option<PathBuf>,
    pub harness: HarnessFile,
    pub memory: MemoryFile,
    pub m4a: M4aFile,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct HarnessFile {
    pub enabled: bool,
    pub bin: Option<PathBuf>,
    pub args: Vec<String>,
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
    pub harness_enabled: bool,
    pub harness_bin: Option<PathBuf>,
    pub harness_args: Vec<String>,
    pub memory_bin: PathBuf,
    pub m4a_enabled: bool,
    pub m4a_master_key_var: Option<String>,
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
        let harness_bin = env(HARNESS_BIN_ENV).map(PathBuf::from).or(file.harness.bin);
        let harness_args = match env(HARNESS_ARGS_ENV) {
            Some(raw) => raw.split_whitespace().map(str::to_owned).collect(),
            None => file.harness.args,
        };
        Ok(Self {
            control_listen,
            control_token: env(CONTROL_TOKEN_ENV).filter(|v| !v.is_empty()),
            data_dir,
            harness_enabled: file.harness.enabled || harness_bin.is_some(),
            harness_bin,
            harness_args,
            memory_bin: env(MEMORY_BIN_ENV)
                .map(PathBuf::from)
                .or(file.memory.bin)
                .unwrap_or_else(|| PathBuf::from("memory4agent-server")),
            m4a_enabled: file.m4a.enabled || env(M4A_MASTER_KEY_VAR_ENV).is_some(),
            m4a_master_key_var: env(M4A_MASTER_KEY_VAR_ENV).or(file.m4a.master_key_var),
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
        assert!(!cfg.harness_enabled && !cfg.m4a_enabled);
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
            "data_dir = \"/from/file\"\n[harness]\nbin = \"/opt/h\"\nargs = [\"--a\"]\n[m4a]\nmaster_key_var = \"FILE_VAR\"\n",
        )
        .unwrap();
        let cfg = ServerConfig::resolve(
            file,
            env_of(&[(DATA_DIR_ENV, "/from/env"), (HARNESS_ARGS_ENV, "--x --y")]),
        )
        .unwrap();
        assert_eq!(cfg.data_dir, PathBuf::from("/from/env"));
        assert_eq!(cfg.harness_args, ["--x", "--y"]);
        assert!(cfg.harness_enabled);
        assert_eq!(cfg.m4a_master_key_var.as_deref(), Some("FILE_VAR"));
    }

    #[test]
    fn unknown_file_keys_are_rejected() {
        assert!(toml::from_str::<FileConfig>("bogus = 1").is_err());
    }
}
