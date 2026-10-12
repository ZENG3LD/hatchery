//! The build host: admission, recipes, build, sign. The signing key is read once at start,
//! kept in memory, never passed to a build, and never returned.

use crate::http;
use crate::wire::{hex, valid_name, valid_source_ref, BuildReceipt, BuildResponse, HostRequest};
use async_trait::async_trait;
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use ed25519_dalek::SigningKey;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::process::Command;
use tokio::sync::{watch, Semaphore};

#[derive(Debug, thiserror::Error)]
pub enum BuildError {
    #[error("node is not admitted for builds")]
    Forbidden,
    #[error("unknown recipe")]
    UnknownRecipe,
    #[error("bad request")]
    BadRequest,
    #[error("admission check unavailable: {0}")]
    AdmissionUnavailable(String),
    #[error("build failed: {0}")]
    Failed(String),
    #[error("build timed out")]
    Timeout,
    #[error("artifact missing or larger than the limit")]
    Artifact,
}

/// Decides whether a node may build right now (node id + status + capability).
#[async_trait]
pub trait Admission: Send + Sync {
    async fn admitted(&self, node_id: &str) -> Result<bool, String>;
}

/// Asks the C2 operator API: `POST /v1/enroll/nodes/<id>/allows {"capability":"build"}`.
pub struct C2Admission {
    pub api: SocketAddr,
    pub token: String,
}

#[async_trait]
impl Admission for C2Admission {
    async fn admitted(&self, node_id: &str) -> Result<bool, String> {
        if !valid_name(node_id, 64) {
            return Ok(false);
        }
        let path = format!("/v1/enroll/nodes/{node_id}/allows");
        let (status, body) = http::post(self.api, &path, Some(&self.token), br#"{"capability":"build"}"#, Duration::from_secs(5))
            .await
            .map_err(|e| e.to_string())?;
        if status == 404 {
            return Ok(false);
        }
        if status != 200 {
            return Err(format!("C2 answered {status}"));
        }
        let value: serde_json::Value = serde_json::from_slice(&body).map_err(|e| e.to_string())?;
        Ok(value["allowed"].as_bool().unwrap_or(false))
    }
}

pub struct Signer {
    key: SigningKey,
}

impl Signer {
    pub fn generate_to_file(path: &Path) -> Result<Self, String> {
        let mut seed = [0_u8; 32];
        getrandom::getrandom(&mut seed).map_err(|e| e.to_string())?;
        hatchery_enroll::write_private(path, B64.encode(seed).as_bytes()).map_err(|e| e.to_string())?;
        Ok(Self { key: SigningKey::from_bytes(&seed) })
    }

    /// The key file must be a regular file owned by the current user with mode 0600.
    pub fn from_file(path: &Path) -> Result<Self, String> {
        let meta = std::fs::symlink_metadata(path).map_err(|e| format!("signing key: {e}"))?;
        // SAFETY: geteuid has no preconditions.
        if !meta.is_file() || meta.uid() != unsafe { libc::geteuid() } || meta.permissions().mode() & 0o777 != 0o600 {
            return Err("signing key file must be a regular file owned by this user with mode 0600".to_owned());
        }
        let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let seed = hatchery_enroll::decode_key32(text.trim()).map_err(|e| e.to_string())?;
        Ok(Self { key: SigningKey::from_bytes(&seed) })
    }

    pub fn from_seed(seed: [u8; 32]) -> Self {
        Self { key: SigningKey::from_bytes(&seed) }
    }

    pub fn public_key(&self) -> String {
        B64.encode(self.key.verifying_key().to_bytes())
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct Recipe {
    pub argv: Vec<String>,
    pub workdir: PathBuf,
    /// Relative to `workdir`.
    pub artifact: PathBuf,
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    /// Run the build as this unprivileged user (needs root); the signing key's owner is
    /// a different user, so the build cannot read the key file.
    #[serde(default)]
    pub run_as: Option<String>,
}

fn default_timeout() -> u64 {
    600
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Recipes {
    pub recipes: BTreeMap<String, Recipe>,
}

impl Recipes {
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let recipes: Self = serde_json::from_str(&text).map_err(|e| e.to_string())?;
        for (name, r) in &recipes.recipes {
            if !valid_name(name, 64) || r.argv.is_empty() || r.artifact.is_absolute() || r.artifact.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
                return Err(format!("recipe '{name}' is invalid"));
            }
        }
        Ok(recipes)
    }
}

pub struct BuildHost {
    recipes: Recipes,
    admission: Arc<dyn Admission>,
    signer: Signer,
    one_at_a_time: Semaphore,
    max_artifact: u64,
}

fn unix_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}

impl BuildHost {
    pub fn new(recipes: Recipes, admission: Arc<dyn Admission>, signer: Signer) -> Self {
        Self { recipes, admission, signer, one_at_a_time: Semaphore::new(1), max_artifact: 16 * 1024 * 1024 }
    }

    pub fn signer_public_key(&self) -> String {
        self.signer.public_key()
    }

    pub async fn build(&self, request: &HostRequest) -> Result<BuildResponse, BuildError> {
        if !valid_name(&request.node_id, 64) || !valid_name(&request.recipe, 64) || request.source_ref.as_deref().is_some_and(|s| !valid_source_ref(s)) {
            return Err(BuildError::BadRequest);
        }
        match self.admission.admitted(&request.node_id).await {
            Ok(true) => {}
            Ok(false) => return Err(BuildError::Forbidden),
            Err(cause) => return Err(BuildError::AdmissionUnavailable(cause)),
        }
        let recipe = self.recipes.recipes.get(&request.recipe).ok_or(BuildError::UnknownRecipe)?;
        let _turn = self.one_at_a_time.acquire().await.map_err(|_| BuildError::Failed("shutting down".into()))?;
        // Admission may have changed while this request waited for its turn.
        if !matches!(self.admission.admitted(&request.node_id).await, Ok(true)) {
            return Err(BuildError::Forbidden);
        }
        let artifact_path = recipe.workdir.join(&recipe.artifact);
        let _ = std::fs::remove_file(&artifact_path);
        let mut command = Command::new(&recipe.argv[0]);
        command
            .args(&recipe.argv[1..])
            .current_dir(&recipe.workdir)
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_else(|| "/usr/local/bin:/usr/bin:/bin".into()))
            .env("HOME", &recipe.workdir)
            .env("HATCHERY_NODE_ID", &request.node_id)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        if let Some(source_ref) = &request.source_ref {
            command.env("HATCHERY_SOURCE_REF", source_ref);
        }
        if let Some(user) = &recipe.run_as {
            let (uid, gid) = lookup_user(user).map_err(BuildError::Failed)?;
            command.uid(uid).gid(gid);
        }
        let child = command.spawn().map_err(|e| BuildError::Failed(e.to_string()))?;
        let output = tokio::time::timeout(Duration::from_secs(recipe.timeout_secs), child.wait_with_output())
            .await
            .map_err(|_| BuildError::Timeout)?
            .map_err(|e| BuildError::Failed(e.to_string()))?;
        if !output.status.success() {
            let tail = String::from_utf8_lossy(&output.stderr);
            let tail: String = tail.chars().rev().take(300).collect::<Vec<_>>().into_iter().rev().collect();
            return Err(BuildError::Failed(format!("exit {:?}: {}", output.status.code(), tail.trim())));
        }
        let meta = std::fs::metadata(&artifact_path).map_err(|_| BuildError::Artifact)?;
        if !meta.is_file() || meta.len() > self.max_artifact {
            return Err(BuildError::Artifact);
        }
        let bytes = std::fs::read(&artifact_path).map_err(|_| BuildError::Artifact)?;
        let receipt = BuildReceipt {
            node_id: request.node_id.clone(),
            recipe: request.recipe.clone(),
            source_ref: request.source_ref.clone(),
            artifact_sha256: hex(&Sha256::digest(&bytes)),
            artifact_size: bytes.len() as u64,
            built_at_ms: unix_ms(),
            signer_public_key: String::new(),
            signature: String::new(),
        }
        .sign(&self.signer.key);
        Ok(BuildResponse { receipt, artifact_b64: B64.encode(&bytes) })
    }
}

fn lookup_user(user: &str) -> Result<(u32, u32), String> {
    let name = std::ffi::CString::new(user).map_err(|_| "bad user".to_owned())?;
    // SAFETY: getpwnam returns static storage; fields are copied immediately.
    let pw = unsafe { libc::getpwnam(name.as_ptr()) };
    if pw.is_null() {
        return Err(format!("user '{user}' does not exist"));
    }
    // SAFETY: non-null checked.
    let (uid, gid) = unsafe { ((*pw).pw_uid, (*pw).pw_gid) };
    if uid == 0 {
        return Err("recipes must not run as root".to_owned());
    }
    Ok((uid, gid))
}

fn status_of(error: &BuildError) -> u16 {
    match error {
        BuildError::Forbidden => 403,
        BuildError::UnknownRecipe => 404,
        BuildError::BadRequest => 400,
        BuildError::AdmissionUnavailable(_) => 502,
        BuildError::Timeout => 504,
        BuildError::Failed(_) | BuildError::Artifact => 422,
    }
}

/// Serve `POST /v1/build` (bearer = the secret shared with C2 only; nodes never hold it).
pub async fn serve(listener: TcpListener, host: Arc<BuildHost>, bearer: String, mut shutdown: watch::Receiver<bool>) -> std::io::Result<()> {
    let bearer: Arc<str> = Arc::from(bearer);
    loop {
        let (mut stream, _) = tokio::select! {
            accepted = listener.accept() => accepted?,
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() { return Ok(()); }
                continue;
            }
        };
        let (host, bearer) = (Arc::clone(&host), Arc::clone(&bearer));
        tokio::spawn(async move {
            let Ok(Ok((method, path, token, body))) = tokio::time::timeout(Duration::from_secs(5), http::read_request(&mut stream, 4096)).await else {
                let _ = http::respond(&mut stream, 400, br#"{"error":"bad request"}"#).await;
                return;
            };
            if !token.as_deref().is_some_and(|t| http::secret_eq(t, &bearer)) {
                let _ = http::respond(&mut stream, 401, br#"{"error":"unauthorized"}"#).await;
                return;
            }
            if method != "POST" || path != "/v1/build" {
                let _ = http::respond(&mut stream, 404, br#"{"error":"not found"}"#).await;
                return;
            }
            let Ok(request) = serde_json::from_slice::<HostRequest>(&body) else {
                let _ = http::respond(&mut stream, 400, br#"{"error":"malformed"}"#).await;
                return;
            };
            match host.build(&request).await {
                Ok(response) => {
                    tracing::info!(node_id = %request.node_id, recipe = %request.recipe, sha256 = %response.receipt.artifact_sha256, "build signed");
                    let _ = http::respond(&mut stream, 200, &serde_json::to_vec(&response).unwrap_or_default()).await;
                }
                Err(error) => {
                    tracing::warn!(node_id = %request.node_id, recipe = %request.recipe, cause = %error, "build refused or failed");
                    let body = serde_json::json!({ "error": error.to_string() });
                    let _ = http::respond(&mut stream, status_of(&error), body.to_string().as_bytes()).await;
                }
            }
        });
    }
}
