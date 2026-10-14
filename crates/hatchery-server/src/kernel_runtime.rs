//! hq-core kernel runtime: the full former-harness runtime (run dispatch, managed
//! worktrees, scheduling and continuations, delivery bundles, observation feed, operator
//! and read host) hosted in-process.
//!
//! The runtime's node adapter dials core's own operator control socket, so every run
//! dispatch, spawn and observation read goes core -> drain -> C2 -> node. Nothing dials
//! a separate C2 control endpoint.
//!
//! The runtime owns the kernel databases (single writer); the hub forwards operator
//! requests to it via [`RemoteKernel`]. Its operator/read host binds loopback only.
//!
//! Catalogs: launch-plan overrides and delivery bundles are read as `*.json` files from
//! `<data dir>/kernel/launch-plans/` and `<data dir>/kernel/delivery-bundles/`. With none,
//! the runtime derives default launch plans from the live node inventory.

use crate::kernel::{KernelError, KERNEL_DB, OBSERVATION_DB};
use hatchery_harness_api::{HarnessOperatorCredential, HarnessOperatorEnvelopeV1, HarnessOperatorRequestV1};
use hatchery_harness_service::c2::HarnessC2Adapter;
use hatchery_harness_service::dispatch::{compile_delivery_catalog_from_json_arguments, HarnessLaunchCatalog};
use hatchery_harness_service::runtime::{start_harness_host_with_operator_and_catalogs, HarnessRuntimeCatalogs};
use hatchery_harness_service::HarnessService;
use hatchery_observation_service::ObservationService;
use serde_json::Value;
use std::io::{BufRead, BufReader, Write};
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::sync::watch;

/// Set to `1` to run the kernel runtime (needs the core mesh).
pub const KERNEL_RUNTIME_ENV: &str = "HATCHERY_KERNEL_RUNTIME";
/// Loopback address of the runtime's operator/read host.
pub const KERNEL_BIND_ENV: &str = "HATCHERY_KERNEL_BIND";
pub const DEFAULT_KERNEL_BIND: &str = "127.0.0.1:18961";
/// Operator credential the hub presents to the runtime host.
pub const KERNEL_OPERATOR_TOKEN_ENV: &str = "GATE4AGENT_HARNESS_OPERATOR_TOKEN";

const CALL_DEADLINE: Duration = Duration::from_secs(45);

/// Settings resolved from the environment (token var removed from the process env).
pub struct KernelRuntimeConfig {
    pub bind: SocketAddr,
    pub credential: HarnessOperatorCredential,
}

impl KernelRuntimeConfig {
    pub fn from_env() -> Result<Option<Self>, String> {
        if std::env::var(KERNEL_RUNTIME_ENV).ok().as_deref() != Some("1") {
            return Ok(None);
        }
        let bind: SocketAddr = std::env::var(KERNEL_BIND_ENV)
            .unwrap_or_else(|_| DEFAULT_KERNEL_BIND.into())
            .parse()
            .map_err(|_| format!("{KERNEL_BIND_ENV} must be a socket address"))?;
        if !bind.ip().is_loopback() {
            return Err(format!("{KERNEL_BIND_ENV} must be loopback"));
        }
        let token = std::env::var(KERNEL_OPERATOR_TOKEN_ENV)
            .map_err(|_| format!("{KERNEL_OPERATOR_TOKEN_ENV} is required with {KERNEL_RUNTIME_ENV}=1"))?;
        std::env::remove_var(KERNEL_OPERATOR_TOKEN_ENV);
        let credential = HarnessOperatorCredential::parse(token)
            .map_err(|e| format!("{KERNEL_OPERATOR_TOKEN_ENV} is malformed: {e}"))?;
        Ok(Some(Self { bind, credential }))
    }
}

/// Hub-side handle: forwards typed operator requests to the runtime host.
#[derive(Clone)]
pub struct RemoteKernel {
    endpoint: SocketAddr,
    credential: HarnessOperatorCredential,
}

impl RemoteKernel {
    pub fn new(endpoint: SocketAddr, credential: HarnessOperatorCredential) -> Self {
        Self { endpoint, credential }
    }

    pub fn endpoint(&self) -> SocketAddr {
        self.endpoint
    }

    /// One operator request (`HarnessOperatorRequestV1` JSON) -> the host's reply JSON.
    pub fn call(&self, request: &Value) -> Result<Value, KernelError> {
        let request: HarnessOperatorRequestV1 = serde_json::from_value(request.clone())
            .map_err(|e| KernelError { status: 400, message: format!("bad operator request: {e}") })?;
        let envelope = HarnessOperatorEnvelopeV1::new(self.credential.clone(), request);
        let mut line = serde_json::to_vec(&envelope)
            .map_err(|e| KernelError { status: 500, message: e.to_string() })?;
        line.push(b'\n');
        let io = |e: std::io::Error| KernelError { status: 503, message: format!("kernel runtime unreachable: {e}") };
        let mut stream = TcpStream::connect_timeout(&self.endpoint, Duration::from_secs(3)).map_err(io)?;
        stream.set_write_timeout(Some(CALL_DEADLINE)).map_err(io)?;
        stream.set_read_timeout(Some(CALL_DEADLINE)).map_err(io)?;
        stream.write_all(&line).map_err(io)?;
        stream.shutdown(Shutdown::Write).map_err(io)?;
        let mut reply = String::new();
        BufReader::new(stream).read_line(&mut reply).map_err(io)?;
        serde_json::from_str(reply.trim_end())
            .map_err(|e| KernelError { status: 502, message: format!("bad kernel reply: {e}") })
    }
}

fn read_json_dir(dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut files: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    files.sort();
    files.iter().filter_map(|p| std::fs::read_to_string(p).ok()).collect()
}

/// Launch-plan overrides and delivery bundles from `<data dir>/kernel/`.
pub fn load_catalogs(data_dir: &Path) -> Result<HarnessRuntimeCatalogs, String> {
    let base = data_dir.join("kernel");
    let launch = HarnessLaunchCatalog::from_json_arguments(read_json_dir(&base.join("launch-plans")))
        .map_err(|e| format!("launch plans: {e}"))?;
    let delivery = compile_delivery_catalog_from_json_arguments(read_json_dir(&base.join("delivery-bundles")))
        .map_err(|e| format!("delivery bundles: {e}"))?;
    HarnessRuntimeCatalogs::new(launch, delivery).map_err(|e| format!("catalogs disagree: {e}"))
}

async fn run_once(
    data_dir: &Path,
    operator_endpoint: &str,
    operator_token: &str,
    config_bind: SocketAddr,
    credential: &HarnessOperatorCredential,
    shutdown: &mut watch::Receiver<bool>,
) -> Result<(), String> {
    let catalogs = load_catalogs(data_dir)?;
    let harness = HarnessService::open(data_dir.join(KERNEL_DB)).map_err(|e| format!("kernel db: {e}"))?;
    let observation =
        ObservationService::open(data_dir.join(OBSERVATION_DB)).map_err(|e| format!("observation db: {e}"))?;
    let (adapter, events) = HarnessC2Adapter::connect(operator_endpoint.to_owned(), operator_token.to_owned())
        .await
        .map_err(|e| format!("operator link: {e}"))?;
    let (host, mut task) = start_harness_host_with_operator_and_catalogs(
        harness,
        observation,
        adapter,
        events,
        config_bind,
        Some(credential.clone()),
        catalogs,
    )
    .await
    .map_err(|e| format!("kernel host: {e}"))?;
    tracing::info!(endpoint = %host.endpoint().socket_addr(), "kernel runtime up (dispatch over core drain)");
    tokio::select! {
        r = &mut task => match r {
            Ok(Ok(())) => Err("kernel runtime stopped".into()),
            Ok(Err(e)) => Err(format!("kernel runtime: {e}")),
            Err(e) => Err(format!("kernel runtime panicked: {e}")),
        },
        _ = shutdown.changed() => {
            let _ = host.shutdown().await;
            let _ = task.await;
            Ok(())
        }
    }
}

/// Run the kernel runtime until shutdown, restarting it with backoff if it stops.
pub async fn run(
    data_dir: PathBuf,
    operator_endpoint: String,
    operator_token: String,
    config: KernelRuntimeConfig,
    mut shutdown: watch::Receiver<bool>,
) {
    let mut backoff = Duration::from_millis(500);
    while !*shutdown.borrow() {
        match run_once(&data_dir, &operator_endpoint, &operator_token, config.bind, &config.credential, &mut shutdown).await {
            Ok(()) => return,
            Err(err) => tracing::warn!(error = %err, "kernel runtime down; restarting"),
        }
        tokio::select! {
            _ = tokio::time::sleep(backoff) => {}
            _ = shutdown.changed() => return,
        }
        backoff = (backoff * 2).min(Duration::from_secs(30));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_catalog_dirs_load() {
        let dir = tempfile::tempdir().unwrap();
        load_catalogs(dir.path()).unwrap();
        std::fs::create_dir_all(dir.path().join("kernel/delivery-bundles")).unwrap();
        std::fs::write(dir.path().join("kernel/delivery-bundles/bad.json"), "{nope").unwrap();
        assert!(load_catalogs(dir.path()).is_err());
    }

    #[test]
    fn remote_kernel_rejects_unknown_request_before_dialing() {
        let cred = HarnessOperatorCredential::parse(format!("{}{}", "g4aho_", "a".repeat(64))).unwrap();
        let r = RemoteKernel::new("127.0.0.1:9".parse().unwrap(), cred);
        assert_eq!(r.call(&serde_json::json!({"nope": 1})).unwrap_err().status, 400);
    }
}
