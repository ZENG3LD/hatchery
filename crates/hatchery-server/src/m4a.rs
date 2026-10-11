//! m4a subservice: the node-side messenger key vault (`m4a-agent` 0.4).
//!
//! Keys live in the vault under `<data_dir>/m4a/.m4a-agent/`. The vault master
//! key is never on disk and never created here: it comes from the environment
//! variable *named* by `HATCHERY_M4A_MASTER_KEY_VAR` (a secret the host
//! injects). Identity is by key signature; no password exists. Agents never see
//! this service, its vault or the master key.

use crate::error::ServerError;
use crate::subservice::{State, Status, Subservice};
use m4a_agent::vault::EnvMasterKey;
use m4a_agent::{FileVault, KeyVault};
use std::path::{Path, PathBuf};

pub struct M4aSubservice {
    store_root: PathBuf,
    master_key_var: Option<String>,
    vault_kind: Option<&'static str>,
    failed: Option<String>,
}

impl M4aSubservice {
    pub fn new(data_dir: &Path, master_key_var: Option<String>) -> Self {
        Self {
            store_root: data_dir.join("m4a"),
            master_key_var,
            vault_kind: None,
            failed: None,
        }
    }

    pub fn vault_dir(&self) -> PathBuf {
        self.store_root.join(".m4a-agent")
    }

    fn open(&self) -> Result<&'static str, ServerError> {
        let var = self
            .master_key_var
            .as_deref()
            .filter(|v| !v.is_empty())
            .ok_or_else(|| {
                ServerError::sub("m4a", "refusing to start: no master key variable configured")
            })?;
        let dir = self.vault_dir();
        std::fs::create_dir_all(&dir)?;
        let marker = dir.join("vault.home");
        if !marker.exists() {
            write_private(&marker, b"envkey")?;
        }
        let vault = FileVault::open(dir.join("vault.enc"), Box::new(EnvMasterKey { var: var.to_owned() }))
            .map_err(|e| ServerError::sub("m4a", format!("key vault: {e}")))?;
        Ok(vault.kind())
    }
}

fn write_private(path: &Path, data: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    #[cfg(unix)]
    use std::os::unix::fs::OpenOptionsExt;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    options.open(path)?.write_all(data)
}

impl Subservice for M4aSubservice {
    fn name(&self) -> &'static str {
        "m4a"
    }

    fn start(&mut self) -> Result<(), ServerError> {
        match self.open() {
            Ok(kind) => {
                self.vault_kind = Some(kind);
                self.failed = None;
                Ok(())
            }
            Err(error) => {
                self.vault_kind = None;
                self.failed = Some(error.to_string());
                Err(error)
            }
        }
    }

    fn stop(&mut self) {
        self.vault_kind = None;
    }

    fn status(&mut self) -> Status {
        let state = if self.vault_kind.is_some() {
            State::Running
        } else if self.failed.is_some() {
            State::Failed
        } else {
            State::Stopped
        };
        // Never report key material: only the vault kind or the failure text.
        let detail = self.vault_kind.map(str::to_owned).or_else(|| self.failed.clone());
        Status { name: "m4a", state, port: None, detail }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_without_master_key_variable() {
        let dir = tempfile::tempdir().unwrap();
        let mut service = M4aSubservice::new(dir.path(), None);
        assert!(service.start().is_err());
        assert_eq!(service.status().state, State::Failed);
    }

    #[test]
    fn refuses_when_variable_is_unset_or_short() {
        let dir = tempfile::tempdir().unwrap();
        let mut unset = M4aSubservice::new(dir.path(), Some("HATCHERY_TEST_M4A_UNSET_KEY".into()));
        assert!(unset.start().is_err());
        assert!(!dir.path().join("m4a/.m4a-agent/vault.enc").exists(), "no vault under a missing key");
    }

    #[test]
    fn opens_vault_with_env_master_key_and_never_reports_it() {
        let dir = tempfile::tempdir().unwrap();
        let var = "HATCHERY_TEST_M4A_MASTER_OK";
        // Test-local secret value, not a real key.
        std::env::set_var(var, "test-master-secret-0123456789");
        let mut service = M4aSubservice::new(dir.path(), Some(var.into()));
        service.start().unwrap();
        let status = service.status();
        assert_eq!(status.state, State::Running);
        let json = serde_json::to_string(&status).unwrap();
        assert!(!json.contains("test-master-secret"), "{json}");
        assert!(dir.path().join("m4a/.m4a-agent/vault.home").exists());
        // Put a record so the vault file exists, then restart with the same key.
        let vault = FileVault::open(
            service.vault_dir().join("vault.enc"),
            Box::new(EnvMasterKey { var: var.to_owned() }),
        )
        .unwrap();
        vault.put("test-label", b"value").unwrap();
        service.stop();
        service.start().unwrap();
        // A different master key must not open the vault.
        let other = "HATCHERY_TEST_M4A_MASTER_OTHER";
        std::env::set_var(other, "another-master-secret-9876543210");
        let mut wrong = M4aSubservice::new(dir.path(), Some(other.into()));
        assert!(wrong.start().is_err(), "a wrong master key must not open the vault");
        std::env::remove_var(var);
        std::env::remove_var(other);
    }
}
