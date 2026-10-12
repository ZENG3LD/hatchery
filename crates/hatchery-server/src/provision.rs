//! P2: provisioning of enrolled nodes on the server side.
//!
//! The server reads the node list from C2 (operator API, bearer from an environment
//! variable) and, whenever a node enrolls or its status changes, brings the server side in
//! line:
//!
//! * an m4a identity per node (ed25519 key pair generated in the vault, master key from the
//!   environment; the private seed never leaves the vault and no agent sees it);
//! * a mailbox address (stub: recorded in the ledger, the mail server consumes the ledger);
//! * rooms and capabilities by status (stub grants, see [`policy`]).
//!
//! Reconciliation is idempotent: the same input changes nothing. A revoked node loses its
//! identity (keys deleted from the vault) and every grant; revocation is final on C2 as well.
//! The ledger holds public data only (key id, public key, grants) and is written 0600.

use crate::error::ServerError;
use crate::subservice::{State, Status, Subservice};
use m4a_agent::vault::EnvMasterKey;
use m4a_agent::{BackendKind, FileVault, IdentityStore, KeyVault};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub use hatchery_enroll::{Grants, NodeStatus, Policy};

/// What C2 tells us about a node.
#[derive(Clone, Debug, Deserialize)]
pub struct NodeInfo {
    pub node_id: String,
    pub status: NodeStatus,
    #[serde(default)]
    pub kind: Option<String>,
}

/// One node as the server provisioned it. Public data only.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LedgerEntry {
    pub node_id: String,
    pub status: NodeStatus,
    pub kind: Option<String>,
    /// m4a identity (absent after revocation).
    pub m4a_key_id: Option<String>,
    pub m4a_public_key: Option<String>,
    pub mailbox: Option<String>,
    pub grants: Grants,
    pub updated_at_ms: u64,
}

/// Read-only shape of a ledger entry for consumers (the mail server, tests): status as text.
#[derive(Clone, Debug, Deserialize)]
pub struct LedgerEntryView {
    pub node_id: String,
    pub status: String,
    pub m4a_public_key: Option<String>,
    pub mailbox: Option<String>,
    pub grants: Grants,
}

/// What a reconcile pass did to one node.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Change {
    Created(String),
    Updated(String, NodeStatus),
    Revoked(String),
}

#[derive(Default, Serialize, Deserialize)]
struct LedgerFile {
    nodes: BTreeMap<String, LedgerEntry>,
}

/// Read the ledger a [`Provisioner`] wrote (consumers such as the messenger and task manager).
pub fn read_ledger(data_dir: &Path) -> Result<Vec<LedgerEntry>, ServerError> {
    let path = data_dir.join("provision").join("ledger.json");
    match std::fs::read(&path) {
        Ok(bytes) => {
            let file: LedgerFile = serde_json::from_slice(&bytes).map_err(|e| ServerError::sub("provision", format!("ledger: {e}")))?;
            Ok(file.nodes.into_values().collect())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(error.into()),
    }
}

pub struct Provisioner {
    ids: IdentityStore,
    ledger_path: PathBuf,
    ledger: LedgerFile,
    policy: Policy,
    server_ref: String,
    mail_domain: String,
}

fn unix_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}

impl Provisioner {
    pub fn new(vault: Arc<dyn KeyVault>, data_dir: &Path, server_ref: &str, mail_domain: &str) -> Result<Self, ServerError> {
        let dir = data_dir.join("provision");
        std::fs::create_dir_all(&dir)?;
        let ledger_path = dir.join("ledger.json");
        let ledger = match std::fs::read(&ledger_path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| ServerError::sub("provision", format!("ledger: {e}")))?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => LedgerFile::default(),
            Err(error) => return Err(error.into()),
        };
        Ok(Self {
            ids: IdentityStore::new(vault),
            ledger_path,
            ledger,
            policy: Policy::default(),
            server_ref: server_ref.to_owned(),
            mail_domain: mail_domain.to_owned(),
        })
    }

    /// Replace the status -> grants policy; the next `reconcile` applies the difference.
    pub fn set_policy(&mut self, policy: Policy) {
        self.policy = policy;
    }

    pub fn entries(&self) -> Vec<LedgerEntry> {
        self.ledger.nodes.values().cloned().collect()
    }

    pub fn entry(&self, node_id: &str) -> Option<&LedgerEntry> {
        self.ledger.nodes.get(node_id)
    }

    /// Bring the server side in line with `nodes`. Idempotent; a node already revoked stays so.
    pub fn reconcile(&mut self, nodes: &[NodeInfo]) -> Result<Vec<Change>, ServerError> {
        let mut changes = Vec::new();
        for node in nodes {
            let existing = self.ledger.nodes.get(&node.node_id).cloned();
            if existing.as_ref().is_some_and(|e| e.status == NodeStatus::Revoked) {
                continue; // final
            }
            let grants = self.policy.grants(node.status);
            if existing.as_ref().is_some_and(|e| e.status == node.status && e.kind == node.kind && e.grants == grants) {
                continue;
            }
            let entry = if node.status == NodeStatus::Revoked {
                self.ids.forget(&node.node_id).map_err(|e| ServerError::sub("provision", e.to_string()))?;
                LedgerEntry {
                    node_id: node.node_id.clone(),
                    status: node.status,
                    kind: node.kind.clone(),
                    m4a_key_id: None,
                    m4a_public_key: None,
                    mailbox: None,
                    grants,
                    updated_at_ms: unix_ms(),
                }
            } else {
                let mut identity = self
                    .ids
                    .resolve(&node.node_id, BackendKind::Server, &self.server_ref)
                    .map_err(|e| ServerError::sub("provision", e.to_string()))?;
                // The nick is the node id; frozen after the first call.
                self.ids.adopt_nick(&mut identity, &node.node_id).map_err(|e| ServerError::sub("provision", e.to_string()))?;
                LedgerEntry {
                    node_id: node.node_id.clone(),
                    status: node.status,
                    kind: node.kind.clone(),
                    m4a_key_id: Some(identity.key_id.clone()),
                    m4a_public_key: Some(identity.public_key.clone()),
                    mailbox: grants.mailbox.then(|| format!("{}@{}", node.node_id, self.mail_domain)),
                    grants,
                    updated_at_ms: unix_ms(),
                }
            };
            changes.push(match (&existing, node.status) {
                (_, NodeStatus::Revoked) => Change::Revoked(node.node_id.clone()),
                (None, _) => Change::Created(node.node_id.clone()),
                _ => Change::Updated(node.node_id.clone(), node.status),
            });
            self.ledger.nodes.insert(node.node_id.clone(), entry);
        }
        if !changes.is_empty() {
            self.save()?;
        }
        Ok(changes)
    }

    fn save(&self) -> Result<(), ServerError> {
        let bytes = serde_json::to_vec_pretty(&self.ledger).map_err(|e| ServerError::sub("provision", e.to_string()))?;
        let tmp = self.ledger_path.with_extension("tmp");
        {
            use std::io::Write;
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create(true).truncate(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            options.open(&tmp)?.write_all(&bytes)?;
        }
        std::fs::rename(tmp, &self.ledger_path)?;
        Ok(())
    }
}

/// Where the node list comes from: the C2 operator API (`GET /v1/enroll/nodes`, bearer).
#[derive(Clone)]
pub struct C2Source {
    pub addr: String,
    pub bearer: String,
}

impl C2Source {
    pub async fn fetch(&self) -> Result<Vec<NodeInfo>, ServerError> {
        #[derive(Deserialize)]
        struct Reply {
            nodes: Vec<NodeInfo>,
        }
        let io = async {
            let mut stream = tokio::net::TcpStream::connect(&self.addr).await?;
            let request = format!(
                "GET /v1/enroll/nodes HTTP/1.1\r\nHost: c2\r\nAuthorization: Bearer {}\r\nConnection: close\r\n\r\n",
                self.bearer
            );
            stream.write_all(request.as_bytes()).await?;
            let mut raw = Vec::new();
            stream.read_to_end(&mut raw).await?;
            Ok::<_, std::io::Error>(raw)
        };
        let raw = tokio::time::timeout(Duration::from_secs(10), io)
            .await
            .map_err(|_| ServerError::sub("provision", "C2 request timed out"))??;
        let split = raw
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .ok_or_else(|| ServerError::sub("provision", "C2 reply has no header end"))?;
        let head = String::from_utf8_lossy(&raw[..split]);
        let status: u16 = head.split_whitespace().nth(1).and_then(|s| s.parse().ok()).unwrap_or(0);
        if status != 200 {
            return Err(ServerError::sub("provision", format!("C2 answered {status}")));
        }
        let reply: Reply = serde_json::from_slice(&raw[split + 4..]).map_err(|e| ServerError::sub("provision", format!("C2 reply: {e}")))?;
        Ok(reply.nodes)
    }
}

#[derive(Clone, Debug, Default)]
struct Summary {
    nodes: usize,
    last_error: Option<String>,
    last_pass_ms: u64,
}

pub struct ProvisionConfig {
    pub data_dir: PathBuf,
    pub master_key_var: String,
    pub c2_addr: String,
    pub c2_token_var: String,
    pub interval: Duration,
    pub server_ref: String,
    pub mail_domain: String,
    /// Same policy file C2 enforces (`HATCHERY_POLICY_FILE`); `None` uses the built-in table.
    pub policy_path: Option<PathBuf>,
}

/// Runs the poll loop as a supervised subservice.
pub struct ProvisionSubservice {
    config: ProvisionConfig,
    task: Option<tokio::task::JoinHandle<()>>,
    summary: Arc<Mutex<Summary>>,
    failed: Option<String>,
}

impl ProvisionSubservice {
    pub fn new(config: ProvisionConfig) -> Self {
        Self { config, task: None, summary: Arc::new(Mutex::new(Summary::default())), failed: None }
    }

    fn build(&self) -> Result<(Provisioner, C2Source), ServerError> {
        let c = &self.config;
        let bearer = std::env::var(&c.c2_token_var)
            .ok()
            .filter(|v| !v.is_empty())
            .ok_or_else(|| ServerError::sub("provision", "the C2 operator token variable is not set"))?;
        let dir = c.data_dir.join("m4a").join(".m4a-agent");
        std::fs::create_dir_all(&dir)?;
        let vault = FileVault::open(dir.join("vault.enc"), Box::new(EnvMasterKey { var: c.master_key_var.clone() }))
            .map_err(|e| ServerError::sub("provision", format!("key vault: {e}")))?;
        let provisioner = Provisioner::new(Arc::new(vault), &c.data_dir, &c.server_ref, &c.mail_domain)?;
        Ok((provisioner, C2Source { addr: c.c2_addr.clone(), bearer }))
    }
}

impl Subservice for ProvisionSubservice {
    fn name(&self) -> &'static str {
        "provision"
    }

    fn start(&mut self) -> Result<(), ServerError> {
        if self.task.is_some() {
            return Ok(());
        }
        let (mut provisioner, source) = match self.build() {
            Ok(parts) => parts,
            Err(error) => {
                self.failed = Some(error.to_string());
                return Err(error);
            }
        };
        self.failed = None;
        let summary = Arc::clone(&self.summary);
        let interval = self.config.interval;
        let policy_path = self.config.policy_path.clone();
        self.task = Some(tokio::spawn(async move {
            let mut policy_stamp: Option<std::time::SystemTime> = None;
            loop {
                // Policy reload: a changed (or newly valid) file is applied on the next pass; an
                // invalid file keeps the previous policy and is reported.
                let mut policy_error = None;
                if let Some(path) = &policy_path {
                    let stamp = std::fs::metadata(path).and_then(|m| m.modified()).ok();
                    if stamp != policy_stamp {
                        match Policy::load(path) {
                            Ok(policy) => {
                                provisioner.set_policy(policy);
                                policy_stamp = stamp;
                                tracing::info!("provisioning policy loaded");
                            }
                            Err(error) => policy_error = Some(format!("policy file: {error}")),
                        }
                    }
                }
                let outcome = match source.fetch().await {
                    Ok(nodes) => provisioner.reconcile(&nodes).map(|changes| (nodes.len(), changes)),
                    Err(error) => Err(error),
                };
                {
                    let mut s = summary.lock().unwrap_or_else(|p| p.into_inner());
                    s.last_pass_ms = unix_ms();
                    match outcome {
                        Ok((count, changes)) => {
                            s.nodes = count;
                            s.last_error = policy_error;
                            for change in changes {
                                tracing::info!(?change, "node provisioned");
                            }
                        }
                        Err(error) => {
                            tracing::warn!(%error, "provisioning pass failed");
                            s.last_error = Some(error.to_string());
                        }
                    }
                }
                tokio::time::sleep(interval).await;
            }
        }));
        Ok(())
    }

    fn stop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }

    fn status(&mut self) -> Status {
        let summary = self.summary.lock().unwrap_or_else(|p| p.into_inner()).clone();
        let running = self.task.as_ref().is_some_and(|t| !t.is_finished());
        let state = if running && summary.last_error.is_none() {
            State::Running
        } else if self.failed.is_some() || summary.last_error.is_some() {
            State::Failed
        } else {
            State::Stopped
        };
        let detail = self.failed.clone().or(summary.last_error).or_else(|| running.then(|| format!("{} nodes", summary.nodes)));
        Status { name: "provision", state, port: None, detail }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use m4a_agent::MemoryVault;

    fn policy(status: NodeStatus) -> Grants {
        Policy::default().grants(status)
    }

    fn node(id: &str, status: NodeStatus) -> NodeInfo {
        NodeInfo { node_id: id.to_owned(), status, kind: Some("qemu".to_owned()) }
    }

    fn provisioner(dir: &Path, vault: Arc<dyn KeyVault>) -> Provisioner {
        Provisioner::new(vault, dir, "https://messenger.test", "nodes.test").unwrap()
    }

    #[test]
    fn a_policy_change_re_grants_without_changing_status_or_key() {
        let dir = tempfile::tempdir().unwrap();
        let vault: Arc<dyn KeyVault> = Arc::new(MemoryVault::new());
        let mut p = provisioner(dir.path(), Arc::clone(&vault));
        p.reconcile(&[node("n", NodeStatus::Member)]).unwrap();
        let key = p.entry("n").unwrap().m4a_public_key.clone();
        p.set_policy(Policy::from_json(r#"{"member":{"mailbox":false,"rooms":["general"],"capabilities":["heartbeat"]}}"#).unwrap());
        let c = p.reconcile(&[node("n", NodeStatus::Member)]).unwrap();
        assert_eq!(c, [Change::Updated("n".into(), NodeStatus::Member)]);
        let e = p.entry("n").unwrap();
        assert!(e.mailbox.is_none() && e.grants.rooms == ["general"] && e.m4a_public_key == key);
        assert!(p.reconcile(&[node("n", NodeStatus::Member)]).unwrap().is_empty());
    }

    #[test]
    fn policy_grows_with_status_and_is_empty_when_out() {
        let n = policy(NodeStatus::Neutral);
        let m = policy(NodeStatus::Member);
        let t = policy(NodeStatus::Trusted);
        assert!(!n.mailbox && n.rooms == ["lobby"] && n.capabilities == ["heartbeat"]);
        assert!(m.mailbox && m.rooms.len() > n.rooms.len() && m.capabilities.contains(&"mail".to_owned()));
        assert!(t.rooms.len() > m.rooms.len() && t.capabilities.contains(&"tasks".to_owned()));
        assert_eq!(policy(NodeStatus::Suspended), Grants::default());
        assert_eq!(policy(NodeStatus::Revoked), Grants::default());
    }

    #[test]
    fn enroll_creates_identity_neutral_gets_almost_nothing_then_flips() {
        let dir = tempfile::tempdir().unwrap();
        let vault: Arc<dyn KeyVault> = Arc::new(MemoryVault::new());
        let mut p = provisioner(dir.path(), Arc::clone(&vault));
        let c = p.reconcile(&[node("lab1", NodeStatus::Neutral)]).unwrap();
        assert_eq!(c, [Change::Created("lab1".into())]);
        let e = p.entry("lab1").unwrap().clone();
        assert!(e.m4a_public_key.is_some() && e.mailbox.is_none());
        assert_eq!(e.grants, policy(NodeStatus::Neutral));
        // the key pair really is in the vault and the ledger holds only the public half
        assert!(vault.get("identity-key/lab1").unwrap().is_some());
        let text = std::fs::read_to_string(dir.path().join("provision/ledger.json")).unwrap();
        assert!(!text.contains("identity-key"));
        // idempotent
        assert!(p.reconcile(&[node("lab1", NodeStatus::Neutral)]).unwrap().is_empty());
        // approval: same key, more grants, mailbox appears
        let c = p.reconcile(&[node("lab1", NodeStatus::Member)]).unwrap();
        assert_eq!(c, [Change::Updated("lab1".into(), NodeStatus::Member)]);
        let m = p.entry("lab1").unwrap();
        assert_eq!(m.m4a_public_key, e.m4a_public_key, "the identity is stable across status changes");
        assert_eq!(m.mailbox.as_deref(), Some("lab1@nodes.test"));
        p.reconcile(&[node("lab1", NodeStatus::Trusted)]).unwrap();
        assert!(p.entry("lab1").unwrap().grants.rooms.contains(&"ops".to_owned()));
    }

    #[test]
    fn suspend_keeps_the_key_but_removes_every_grant() {
        let dir = tempfile::tempdir().unwrap();
        let vault: Arc<dyn KeyVault> = Arc::new(MemoryVault::new());
        let mut p = provisioner(dir.path(), Arc::clone(&vault));
        p.reconcile(&[node("n", NodeStatus::Trusted)]).unwrap();
        p.reconcile(&[node("n", NodeStatus::Suspended)]).unwrap();
        let e = p.entry("n").unwrap();
        assert!(e.m4a_public_key.is_some() && e.mailbox.is_none() && e.grants == Grants::default());
        assert!(vault.get("identity-key/n").unwrap().is_some());
        p.reconcile(&[node("n", NodeStatus::Member)]).unwrap();
        assert!(p.entry("n").unwrap().mailbox.is_some(), "a suspended node can be restored");
    }

    #[test]
    fn revoke_deletes_the_identity_and_is_final() {
        let dir = tempfile::tempdir().unwrap();
        let vault: Arc<dyn KeyVault> = Arc::new(MemoryVault::new());
        let mut p = provisioner(dir.path(), Arc::clone(&vault));
        p.reconcile(&[node("n", NodeStatus::Member)]).unwrap();
        let c = p.reconcile(&[node("n", NodeStatus::Revoked)]).unwrap();
        assert_eq!(c, [Change::Revoked("n".into())]);
        assert!(vault.get("identity-key/n").unwrap().is_none(), "keys are gone from the vault");
        let e = p.entry("n").unwrap();
        assert!(e.m4a_public_key.is_none() && e.grants == Grants::default());
        // nothing brings it back, even if the source says member again
        assert!(p.reconcile(&[node("n", NodeStatus::Member)]).unwrap().is_empty());
        assert_eq!(p.entry("n").unwrap().status, NodeStatus::Revoked);
        assert!(vault.get("identity-key/n").unwrap().is_none());
    }

    #[test]
    fn ledger_survives_a_restart_and_is_private() {
        let dir = tempfile::tempdir().unwrap();
        let vault: Arc<dyn KeyVault> = Arc::new(MemoryVault::new());
        {
            let mut p = provisioner(dir.path(), Arc::clone(&vault));
            p.reconcile(&[node("a", NodeStatus::Member)]).unwrap();
        }
        let mut p = provisioner(dir.path(), Arc::clone(&vault));
        assert_eq!(p.entry("a").unwrap().status, NodeStatus::Member);
        assert!(p.reconcile(&[node("a", NodeStatus::Member)]).unwrap().is_empty());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.path().join("provision/ledger.json")).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn real_file_vault_needs_the_env_master_key() {
        let dir = tempfile::tempdir().unwrap();
        let var = "HATCHERY_TEST_PROVISION_MASTER";
        std::env::set_var(var, "test-master-secret-0123456789");
        let config = |v: &str| ProvisionConfig {
            data_dir: dir.path().to_owned(),
            master_key_var: v.to_owned(),
            c2_addr: "127.0.0.1:1".into(),
            c2_token_var: var.to_owned(),
            interval: Duration::from_secs(1),
            server_ref: "https://messenger.test".into(),
            mail_domain: "nodes.test".into(),
            policy_path: None,
        };
        let (mut p, _) = ProvisionSubservice::new(config(var)).build().unwrap();
        p.reconcile(&[node("k", NodeStatus::Member)]).unwrap();
        assert!(dir.path().join("m4a/.m4a-agent/vault.enc").exists());
        // the seed is not readable from the ledger or the directory listing of provision/
        for f in std::fs::read_dir(dir.path().join("provision")).unwrap() {
            let text = std::fs::read(f.unwrap().path()).unwrap();
            assert!(!String::from_utf8_lossy(&text).contains("test-master-secret"));
        }
        std::env::remove_var(var);
    }
}
