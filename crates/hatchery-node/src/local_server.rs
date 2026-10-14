//! Session-local HTTP server pool (tesserax-http static + supervised http children).
//!
//! Ports bind loopback only. Instances are keyed by session id (same string the
//! session browser uses). Lifecycle: start/stop via MCP door or restream control;
//! session end stops every instance for that session.

use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use tokio::process::{Child, Command};

/// Per-session port window size (ports are still ephemeral binds on 127.0.0.1:0).
const MAX_PER_SESSION: usize = 8;
const STOP_WAIT: Duration = Duration::from_secs(3);

#[derive(Clone, Debug)]
pub struct InstanceInfo {
    pub id: String,
    pub session: String,
    pub kind: String,
    pub url: String,
    pub port: u16,
    pub root: Option<String>,
}

enum Runtime {
    Static {
        // Keep the running server alive until stop.
        server: tesserax::RunningServer,
    },
    Http {
        child: Child,
    },
}

struct Instance {
    info: InstanceInfo,
    runtime: Runtime,
    log: Arc<Mutex<Vec<String>>>,
}

struct PoolState {
    by_session: BTreeMap<String, BTreeMap<String, Instance>>,
}

pub struct LocalServerPool {
    inner: Mutex<PoolState>,
}

impl LocalServerPool {
    fn new() -> Self {
        Self {
            inner: Mutex::new(PoolState {
                by_session: BTreeMap::new(),
            }),
        }
    }

    pub fn global() -> Arc<LocalServerPool> {
        static POOL: OnceLock<Arc<LocalServerPool>> = OnceLock::new();
        POOL.get_or_init(|| Arc::new(LocalServerPool::new())).clone()
    }

    pub fn kinds() -> Value {
        json!([
            {"kind": "static", "description": "Serve a directory with tesserax-http StaticDir (SPA fallback when index.html exists)."},
            {"kind": "http", "description": "Run a user binary that listens on PORT (loopback). cwd and env are optional."}
        ])
    }

    pub async fn start(
        &self,
        session: &str,
        kind: &str,
        root: Option<&str>,
        cwd: Option<&str>,
        bin: Option<&str>,
        env: Option<&Value>,
        spa: bool,
    ) -> Result<InstanceInfo, String> {
        if session.is_empty() || session.len() > 128 {
            return Err("invalid session".into());
        }
        {
            let g = self.inner.lock().map_err(|_| "lock")?;
            let n = g.by_session.get(session).map(|m| m.len()).unwrap_or(0);
            if n >= MAX_PER_SESSION {
                return Err(format!("session already has {MAX_PER_SESSION} local servers"));
            }
        }
        let id = format!("ls-{}", &uuid::Uuid::new_v4().to_string()[..8]);
        let (info, runtime) = match kind {
            "static" => {
                let root = root.ok_or("static kind requires root")?;
                let root_path = PathBuf::from(root);
                if !root_path.is_absolute() {
                    return Err("root must be an absolute path".into());
                }
                if !root_path.is_dir() {
                    return Err("root is not a directory".into());
                }
                start_static(&id, session, &root_path, spa).await?
            }
            "http" => {
                let bin = bin.or(cwd).ok_or("http kind requires bin or cwd")?;
                let cwd_path = cwd.map(PathBuf::from);
                start_http(&id, session, bin, cwd_path.as_deref(), env).await?
            }
            _ => return Err(format!("unknown kind: {kind}")),
        };
        let log = Arc::new(Mutex::new(Vec::new()));
        {
            let mut g = self.inner.lock().map_err(|_| "lock")?;
            g.by_session
                .entry(session.to_owned())
                .or_default()
                .insert(id.clone(), Instance { info: info.clone(), runtime, log });
        }
        Ok(info)
    }

    pub async fn stop(&self, session: &str, id: &str) -> Result<(), String> {
        let inst = {
            let mut g = self.inner.lock().map_err(|_| "lock")?;
            g.by_session
                .get_mut(session)
                .and_then(|m| m.remove(id))
                .ok_or_else(|| "not found".to_owned())?
        };
        stop_runtime(inst.runtime).await;
        let mut g = self.inner.lock().map_err(|_| "lock")?;
        if let Some(m) = g.by_session.get(session) {
            if m.is_empty() {
                g.by_session.remove(session);
            }
        }
        Ok(())
    }

    pub fn list(&self, session: &str) -> Value {
        let g = match self.inner.lock() {
            Ok(g) => g,
            Err(_) => return json!({"kinds": Self::kinds(), "instances": []}),
        };
        let instances: Vec<Value> = g
            .by_session
            .get(session)
            .map(|m| {
                m.values()
                    .map(|i| {
                        json!({
                            "id": i.info.id,
                            "kind": i.info.kind,
                            "url": i.info.url,
                            "port": i.info.port,
                            "root": i.info.root,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        json!({ "kinds": Self::kinds(), "instances": instances, "session": session })
    }

    pub fn logs(&self, session: &str, id: &str, limit: usize) -> Result<Value, String> {
        let g = self.inner.lock().map_err(|_| "lock")?;
        let inst = g
            .by_session
            .get(session)
            .and_then(|m| m.get(id))
            .ok_or_else(|| "not found".to_owned())?;
        let lines = inst.log.lock().map_err(|_| "lock")?;
        let start = lines.len().saturating_sub(limit.max(1).min(200));
        Ok(json!({ "id": id, "lines": lines[start..].to_vec() }))
    }

    pub async fn stop_session(&self, session: &str) {
        let ids: Vec<String> = {
            let g = match self.inner.lock() {
                Ok(g) => g,
                Err(_) => return,
            };
            g.by_session
                .get(session)
                .map(|m| m.keys().cloned().collect())
                .unwrap_or_default()
        };
        for id in ids {
            let _ = self.stop(session, &id).await;
        }
    }
}

async fn start_static(
    id: &str,
    session: &str,
    root: &Path,
    spa: bool,
) -> Result<(InstanceInfo, Runtime), String> {
    use tesserax::{ServerBuilder, Transport};
    use tesserax_http::assets::StaticDir;
    use tesserax_http::HttpExt;

    let mut dir = StaticDir::new(root);
    if spa {
        let index = root.join("index.html");
        if index.is_file() {
            dir = dir.with_spa_fallback(index);
        }
    }
    let built = ServerBuilder::new(format!("hatchery-local-{id}"))
        .transport(Transport::local(0))
        .with_static_dir(dir)
        .build()
        .await
        .map_err(|e| format!("build static server: {e}"))?;
    let running = built
        .start()
        .await
        .map_err(|e| format!("start static server: {e}"))?;
    let addr = running.local_addr();
    if !addr.ip().is_loopback() {
        running.shutdown();
        let _ = running.wait().await;
        return Err("refused non-loopback bind".into());
    }
    let port = addr.port();
    let url = format!("http://127.0.0.1:{port}/");
    Ok((
        InstanceInfo {
            id: id.to_owned(),
            session: session.to_owned(),
            kind: "static".into(),
            url,
            port,
            root: Some(root.display().to_string()),
        },
        Runtime::Static { server: running },
    ))
}

async fn start_http(
    id: &str,
    session: &str,
    bin: &str,
    cwd: Option<&Path>,
    env: Option<&Value>,
) -> Result<(InstanceInfo, Runtime), String> {
    // Bind an ephemeral port ourselves and pass it as PORT so the child can listen.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| format!("port alloc: {e}"))?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    drop(listener);

    let mut cmd = Command::new(bin);
    if let Some(cwd) = cwd {
        cmd.current_dir(cwd);
    }
    cmd.env("PORT", port.to_string())
        .env("HOST", "127.0.0.1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    if let Some(Value::Object(map)) = env {
        for (k, v) in map {
            if let Some(s) = v.as_str() {
                cmd.env(k, s);
            }
        }
    }
    let child = cmd.spawn().map_err(|e| format!("spawn http server: {e}"))?;
    // Brief readiness: accept a TCP connect.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        if tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .is_ok()
        {
            break;
        }
        if tokio::time::Instant::now() > deadline {
            return Err("http server did not accept on PORT in time".into());
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let url = format!("http://127.0.0.1:{port}/");
    Ok((
        InstanceInfo {
            id: id.to_owned(),
            session: session.to_owned(),
            kind: "http".into(),
            url,
            port,
            root: cwd.map(|p| p.display().to_string()),
        },
        Runtime::Http { child },
    ))
}

async fn stop_runtime(runtime: Runtime) {
    match runtime {
        Runtime::Static { server } => {
            server.shutdown();
            let _ = tokio::time::timeout(STOP_WAIT, server.wait()).await;
        }
        Runtime::Http { mut child } => {
            let _ = child.kill().await;
            let _ = tokio::time::timeout(STOP_WAIT, child.wait()).await;
        }
    }
}

/// Door / control dispatch. `op` is `local_server.start|stop|list|logs`.
pub async fn handle_op(op: &str, payload: &Value) -> Result<Value, String> {
    let pool = LocalServerPool::global();
    let session = payload
        .get("session")
        .and_then(Value::as_str)
        .ok_or("session required")?;
    match op {
        "local_server.list" => Ok(pool.list(session)),
        "local_server.start" => {
            let kind = payload
                .get("kind")
                .and_then(Value::as_str)
                .ok_or("kind required")?;
            let root = payload.get("root").and_then(Value::as_str);
            let cwd = payload.get("cwd").and_then(Value::as_str);
            let bin = payload
                .get("bin")
                .and_then(Value::as_str)
                .or_else(|| payload.get("command").and_then(Value::as_str));
            let spa = payload
                .get("spa")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            let env = payload.get("env");
            let info = pool
                .start(session, kind, root, cwd, bin, env, spa)
                .await?;
            let open = payload
                .get("open")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if open {
                #[cfg(feature = "d2b")]
                {
                    let browsers = crate::browser_capture::shared_map();
                    let url = info.url.clone();
                    let sess = session.to_owned();
                    // Best-effort: navigate an existing session browser; ignore if none.
                    if let Ok(bytes) = hatchery_core::encode_browser_navigate(&url) {
                        let _ = browsers.input(&sess, &bytes).await;
                    }
                }
            }
            Ok(json!({
                "id": info.id,
                "url": info.url,
                "port": info.port,
                "kind": info.kind,
            }))
        }
        "local_server.stop" => {
            let id = payload
                .get("id")
                .and_then(Value::as_str)
                .ok_or("id required")?;
            pool.stop(session, id).await?;
            Ok(json!({"ok": true, "id": id}))
        }
        "local_server.logs" => {
            let id = payload
                .get("id")
                .and_then(Value::as_str)
                .ok_or("id required")?;
            let limit = payload
                .get("limit")
                .and_then(Value::as_u64)
                .unwrap_or(50) as usize;
            pool.logs(session, id, limit)
        }
        _ => Err("unknown local_server op".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[tokio::test]
    async fn static_start_list_stop_on_loopback() {
        let dir = tempfile::tempdir().unwrap();
        let mut f = std::fs::File::create(dir.path().join("index.html")).unwrap();
        write!(f, "<h1>hi</h1>").unwrap();
        let pool = LocalServerPool::new();
        let info = pool
            .start("pty-test-1", "static", Some(dir.path().to_str().unwrap()), None, None, None, true)
            .await
            .unwrap();
        assert!(info.url.starts_with("http://127.0.0.1:"));
        let list = pool.list("pty-test-1");
        assert_eq!(list["instances"].as_array().unwrap().len(), 1);
        let body = reqwest::get(&info.url).await.unwrap().text().await.unwrap();
        assert!(body.contains("hi"));
        pool.stop("pty-test-1", &info.id).await.unwrap();
        assert!(pool.list("pty-test-1")["instances"]
            .as_array()
            .unwrap()
            .is_empty());
    }
}
