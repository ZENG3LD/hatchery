//! memory4agent subservice (crates.io `memory4agent` 0.3, binary
//! `memory4agent-server`). On demand only: the server never starts it by
//! itself. It listens on loopback in 18200-18299, has no authentication, and is
//! never published to WireGuard or outside. The child runs with a scrubbed
//! environment so it never sees vault keys or tokens.

use crate::config::{MEMORY_PORT_MAX, MEMORY_PORT_MIN};
use crate::error::ServerError;
use crate::subservice::{ManagedProcess, State, Status, Subservice};
use std::net::{Ipv4Addr, SocketAddr, TcpListener};
use std::path::{Path, PathBuf};
use tokio::process::Command;

/// Variables the child may inherit. Everything else is dropped.
const PASS_THROUGH_ENV: &[&str] = &["PATH", "HOME", "LANG", "RUST_LOG", "TMPDIR"];

pub struct MemorySubservice {
    bin: PathBuf,
    data_dir: PathBuf,
    process: ManagedProcess,
    port: Option<u16>,
    failed: Option<String>,
}

impl MemorySubservice {
    pub fn new(bin: PathBuf, data_dir: &Path) -> Self {
        Self {
            bin,
            data_dir: data_dir.join("memory"),
            process: ManagedProcess::new(),
            port: None,
            failed: None,
        }
    }

    /// First port of the range that can be bound on loopback right now.
    pub fn pick_port() -> Result<u16, ServerError> {
        (MEMORY_PORT_MIN..=MEMORY_PORT_MAX)
            .find(|port| TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, *port))).is_ok())
            .ok_or_else(|| ServerError::sub("memory", "no free loopback port in 18200-18299"))
    }
}

impl Subservice for MemorySubservice {
    fn name(&self) -> &'static str {
        "memory"
    }

    fn on_demand(&self) -> bool {
        true
    }

    fn start(&mut self) -> Result<(), ServerError> {
        if self.process.alive() {
            return Ok(());
        }
        let result = (|| {
            std::fs::create_dir_all(&self.data_dir)?;
            let port = Self::pick_port()?;
            debug_assert!((MEMORY_PORT_MIN..=MEMORY_PORT_MAX).contains(&port));
            let mut command = Command::new(&self.bin);
            command.env_clear();
            for name in PASS_THROUGH_ENV {
                if let Some(value) = std::env::var_os(name) {
                    command.env(name, value);
                }
            }
            command
                .env("MEMORY4AGENT_PORT", port.to_string())
                .env("MEMORY4AGENT_DATA_DIR", &self.data_dir);
            self.process.spawn(command)?;
            Ok::<u16, ServerError>(port)
        })();
        match result {
            Ok(port) => {
                self.port = Some(port);
                self.failed = None;
                Ok(())
            }
            Err(error) => {
                self.port = None;
                self.failed = Some(error.to_string());
                Err(ServerError::sub("memory", error.to_string()))
            }
        }
    }

    fn stop(&mut self) {
        self.process.stop();
        self.port = None;
    }

    fn status(&mut self) -> Status {
        let state = if self.process.alive() {
            State::Running
        } else if self.failed.is_some() {
            State::Failed
        } else {
            State::Stopped
        };
        let port = if state == State::Running { self.port } else { None };
        Status { name: "memory", state, port, detail: self.failed.clone() }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn fake_server(dir: &Path) -> PathBuf {
        let path = dir.join("fake-memory-server");
        std::fs::write(&path, "#!/bin/sh\nenv > \"$MEMORY4AGENT_DATA_DIR/env.txt\"\nexec sleep 30\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[tokio::test]
    async fn is_on_demand_and_runs_on_loopback_range_with_scrubbed_env() {
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("HATCHERY_TEST_SECRET_NOT_FOR_CHILD", "value");
        let mut service = MemorySubservice::new(fake_server(dir.path()), dir.path());
        assert!(service.on_demand());
        assert_eq!(service.status().state, State::Stopped);
        service.start().unwrap();
        let status = service.status();
        assert_eq!(status.state, State::Running);
        let port = status.port.unwrap();
        assert!((MEMORY_PORT_MIN..=MEMORY_PORT_MAX).contains(&port));
        for _ in 0..50 {
            if dir.path().join("memory/env.txt").exists() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        let env = std::fs::read_to_string(dir.path().join("memory/env.txt")).unwrap();
        assert!(env.contains(&format!("MEMORY4AGENT_PORT={port}")));
        assert!(!env.contains("HATCHERY_TEST_SECRET_NOT_FOR_CHILD"), "{env}");
        service.stop();
        assert_eq!(service.status().state, State::Stopped);
        std::env::remove_var("HATCHERY_TEST_SECRET_NOT_FOR_CHILD");
    }

    #[test]
    fn skips_busy_ports_and_stays_in_range() {
        let first = MemorySubservice::pick_port().unwrap();
        let _held = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, first))).unwrap();
        let second = MemorySubservice::pick_port().unwrap();
        assert_ne!(first, second);
        assert!((MEMORY_PORT_MIN..=MEMORY_PORT_MAX).contains(&second));
    }

    #[test]
    fn missing_binary_fails_without_a_port() {
        let dir = tempfile::tempdir().unwrap();
        let mut service = MemorySubservice::new(dir.path().join("absent"), dir.path());
        assert!(service.start().is_err());
        let status = service.status();
        assert_eq!(status.state, State::Failed);
        assert!(status.port.is_none());
    }
}
