#![allow(dead_code)]
#![cfg(unix)]
//! C2 <-> node enrollment end to end on loopback (the WireGuard provisioner is recorded, the
//! kernel path is covered by the netns run in deploy/node/E2E.md).
//!
//! Covers: no peer without a valid code, neutral first, operator approval flips status,
//! single-use code (replay refused), signed call-home (the node holds the only key), token
//! nodes unchanged, revoke is final and disconnects, an impostor with the wrong key is
//! refused, and restart persistence (registry reload, peers re-applied, node reconnects).

use hatchery_c2::enroll_service::{EnrollConfig, PeerProvisioner, PublicInfo};
use hatchery_c2::enrollment::TunnelPool;
use hatchery_c2::protocol::NodeId;
use hatchery_c2::{C2Config, C2NodeConfig, C2Running};
use hatchery_enroll::{NodeIdentity, NodeKind};
use hatchery_node::enroll_client;
use hatchery_node::protocol::WorkspaceId;
use hatchery_node::{NodeServer, NodeServerConfig, WorkspaceConfig};
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::{sleep, timeout};

const API_TOKEN: &str = "operator-api-token-for-test";

#[derive(Default)]
struct Recording(Mutex<Vec<String>>);

impl PeerProvisioner for Recording {
    fn add_peer(&self, key: &str, ip: &str) -> std::io::Result<()> {
        self.0.lock().unwrap().push(format!("add {key} {ip}"));
        Ok(())
    }
    fn remove_peer(&self, key: &str) -> std::io::Result<()> {
        self.0.lock().unwrap().push(format!("remove {key}"));
        Ok(())
    }
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

struct Dir(PathBuf);

impl Dir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("hatchery-enroll-e2e-{}-{}", std::process::id(), free_port()));
        std::fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
        Self(path)
    }
    fn p(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

async fn http(addr: std::net::SocketAddr, method: &str, path: &str, bearer: Option<&str>, body: &str) -> (u16, String) {
    let mut stream = TcpStream::connect(addr).await.unwrap();
    let auth = bearer.map(|t| format!("Authorization: Bearer {t}\r\n")).unwrap_or_default();
    let request = format!("{method} {path} HTTP/1.1\r\nHost: x\r\n{auth}Content-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).await.unwrap();
    let text = String::from_utf8_lossy(&raw).into_owned();
    let status = text.split_whitespace().nth(1).unwrap().parse().unwrap();
    let body = text.split("\r\n\r\n").nth(1).unwrap_or_default().to_owned();
    (status, body)
}

struct Ports {
    api: std::net::SocketAddr,
    enroll: std::net::SocketAddr,
    call_home: std::net::SocketAddr,
}

fn start_config(dir: &Dir, ports: &Ports, provisioner: Arc<Recording>, with_token_node: bool) -> C2Config {
    let nodes = if with_token_node {
        vec![C2NodeConfig::new(NodeId::new("tok1").unwrap(), "accept", "token-node-secret-token").unwrap()]
    } else {
        Vec::new()
    };
    let mut enroll = EnrollConfig::new(ports.enroll);
    enroll.registry_path = Some(dir.p("registry.json"));
    enroll.pool = Some(TunnelPool { c2_ip: "10.79.0.1".parse().unwrap(), prefix: 24 });
    enroll.provisioner = provisioner;
    enroll.public = PublicInfo {
        c2_tunnel_ip: Some("10.79.0.1".into()),
        c2_wg_public_key: Some("c2-public-key-placeholder".into()),
        c2_wg_endpoint: Some("c2.example:51890".into()),
        c2_call_home: Some(ports.call_home.to_string()),
            upstream_c2_public_key: None,
            upstream_recipient_public_key: None,
        };
    C2Config::new(ports.api, API_TOKEN, nodes)
        .unwrap()
        .with_control_endpoint(dir.p("c2.sock").to_string_lossy().into_owned())
        .unwrap()
        .with_node_listen(ports.call_home)
        .unwrap()
        .with_enrollment(enroll)
}

fn node_server(dir: &Dir, id: &str, identity: Option<Arc<NodeIdentity>>, call_home: std::net::SocketAddr, token: &str) -> NodeServer {
    let mut config = NodeServerConfig::new(
        dir.p(&format!("{id}.sock")).to_string_lossy().into_owned(),
        token,
        NodeId::new(id).unwrap(),
        [WorkspaceConfig::new(WorkspaceId::new("primary").unwrap(), std::env::current_dir().unwrap()).unwrap()],
    )
    .unwrap()
    .with_call_home(call_home)
    .unwrap();
    if let Some(identity) = identity {
        config = config.with_call_home_identity(identity);
    }
    NodeServer::new_fixture(config).unwrap()
}

async fn is_online(api: std::net::SocketAddr, id: &str) -> bool {
    let (status, body) = http(api, "GET", "/status", Some(API_TOKEN), "").await;
    if status != 200 {
        return false;
    }
    let value: serde_json::Value = serde_json::from_str(&body).unwrap();
    value["nodes"][id]["transport"].as_str().is_some_and(|t| t.eq_ignore_ascii_case("online"))
}

async fn wait_online(api: std::net::SocketAddr, id: &str, want: bool, secs: u64) {
    timeout(Duration::from_secs(secs), async {
        while is_online(api, id).await != want {
            sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("node {id} did not become online={want}"));
}

async fn code(api: std::net::SocketAddr) -> String {
    let (status, body) = http(api, "POST", "/v1/enroll/codes", Some(API_TOKEN), "{}").await;
    assert_eq!(status, 200);
    serde_json::from_str::<serde_json::Value>(&body).unwrap()["code"].as_str().unwrap().to_owned()
}

async fn record_status(api: std::net::SocketAddr, id: &str) -> Option<String> {
    let (status, body) = http(api, "GET", "/v1/enroll/nodes", Some(API_TOKEN), "").await;
    assert_eq!(status, 200);
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    v["nodes"].as_array().unwrap().iter().find(|n| n["node_id"] == id).map(|n| n["status"].as_str().unwrap().to_owned())
}

async fn stop(shutdown: hatchery_node::NodeShutdownHandle, task: tokio::task::JoinHandle<Result<(), hatchery_node::NodeServerError>>) {
    shutdown.request_shutdown().await.unwrap();
    let _ = timeout(Duration::from_secs(5), task).await;
}


use hatchery_build::{serve, BuildHost, BuildReceipt, C2Admission, Recipes, Signer};
use hatchery_build::wire::{HostRequest, SignedBuildRequest};

struct Rig {
    dir: Dir,
    api: std::net::SocketAddr,
    enroll: std::net::SocketAddr,
    host_addr: std::net::SocketAddr,
    host_bearer: String,
    signer_pub: String,
    signer_seed: [u8; 32],
    _c2: C2Running,
    _shutdown: tokio::sync::watch::Sender<bool>,
}

async fn rig() -> Rig {
    let dir = Dir::new();
    let ports = Ports {
        api: format!("127.0.0.1:{}", free_port()).parse().unwrap(),
        enroll: format!("127.0.0.1:{}", free_port()).parse().unwrap(),
        call_home: format!("127.0.0.1:{}", free_port()).parse().unwrap(),
    };
    // Build host: recipe writes an artifact; signing seed lives only in this struct.
    let work = dir.p("work");
    std::fs::create_dir_all(&work).unwrap();
    std::fs::write(work.join("build.sh"), "echo \"artifact-for:$HATCHERY_NODE_ID\" > out.bin\n").unwrap();
    let recipes = Recipes {
        recipes: std::collections::BTreeMap::from([(
            "hello".to_owned(),
            serde_json::from_value(serde_json::json!({"argv":["sh","build.sh"],"workdir":work,"artifact":"out.bin","timeout_secs":20})).unwrap(),
        )]),
    };
    let signer_seed = [42_u8; 32];
    let host_addr: std::net::SocketAddr = format!("127.0.0.1:{}", free_port()).parse().unwrap();
    let host_bearer = "c2-to-host-secret-for-test".to_owned();
    let host = Arc::new(BuildHost::new(
        recipes,
        Arc::new(C2Admission { api: ports.api, token: API_TOKEN.to_owned() }),
        Signer::from_seed(signer_seed),
    ));
    let signer_pub = host.signer_public_key();
    let (tx, rx) = tokio::sync::watch::channel(false);
    let listener = tokio::net::TcpListener::bind(host_addr).await.unwrap();
    tokio::spawn(serve(listener, host, host_bearer.clone(), rx));

    let mut config = start_config(&dir, &ports, Arc::new(Recording::default()), false);
    if let Some(enroll) = config.enrollment.as_mut() {
        enroll.build_host = Some((host_addr, host_bearer.clone()));
    }
    let c2 = C2Running::start(config).await.unwrap();
    Rig { api: ports.api, enroll: ports.enroll, host_addr, host_bearer, signer_pub, signer_seed, dir, _c2: c2, _shutdown: tx }
}

async fn enroll_node(rig: &Rig, id: &str) -> NodeIdentity {
    let keys = rig.dir.p(&format!("keys-{id}"));
    let code = code(rig.api).await;
    enroll_client::enroll(rig.enroll, id, NodeKind::Qemu, Some(&code), &keys).await.unwrap();
    NodeIdentity::load_signing_only(&keys).unwrap()
}

async fn status(rig: &Rig, id: &str, s: &str) {
    let (code, body) = http(rig.api, "POST", &format!("/v1/enroll/nodes/{id}/status"), Some(API_TOKEN), &format!(r#"{{"status":"{s}"}}"#)).await;
    assert_eq!(code, 200, "{body}");
}

async fn build(rig: &Rig, who: &NodeIdentity, id: &str) -> Result<(BuildReceipt, Vec<u8>), String> {
    enroll_client::request_build(rig.enroll, who, id, "hello", Some("main"), &rig.signer_pub).await.map_err(|e| e.to_string())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn build_needs_the_capability_and_returns_a_signed_artifact_without_the_key() {
    let rig = rig().await;
    let alpha = enroll_node(&rig, "alpha").await;

    // (a) neutral and member cannot start a build.
    let r = build(&rig, &alpha, "alpha").await;
    assert!(r.as_ref().is_err_and(|e| e.contains("refused")), "neutral: {r:?}");
    status(&rig, "alpha", "member").await;
    let r = build(&rig, &alpha, "alpha").await;
    assert!(r.as_ref().is_err_and(|e| e.contains("refused")), "member: {r:?}");
    assert!(!rig.dir.p("work").join("out.bin").exists(), "nothing was built for a refused node");

    // (b) trusted: signed artifact, verified by the node against the operator-provided public key.
    status(&rig, "alpha", "trusted").await;
    let (receipt, artifact) = build(&rig, &alpha, "alpha").await.unwrap();
    assert_eq!(artifact, b"artifact-for:alpha\n");
    receipt.verify(&rig.signer_pub, Some(&artifact)).unwrap();
    // The node never holds the signing key: not in its state dir, not in the receipt.
    let seed_b64 = {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD.encode(rig.signer_seed)
    };
    for entry in std::fs::read_dir(rig.dir.p("keys-alpha")).unwrap() {
        let text = std::fs::read(entry.unwrap().path()).unwrap();
        assert!(!String::from_utf8_lossy(&text).contains(&seed_b64));
    }
    assert!(!serde_json::to_string(&receipt).unwrap().contains(&seed_b64));
    // A wrong trusted key makes the node reject the receipt.
    let wrong = Signer::from_seed([1; 32]).public_key();
    let r = enroll_client::request_build(rig.enroll, &alpha, "alpha", "hello", None, &wrong).await;
    assert!(r.is_err_and(|e| e.to_string().contains("receipt rejected")));

    // (c) suspended and revoked are refused by C2.
    status(&rig, "alpha", "suspended").await;
    assert!(build(&rig, &alpha, "alpha").await.is_err());
    status(&rig, "alpha", "trusted").await;
    build(&rig, &alpha, "alpha").await.unwrap();
    let (s, _) = http(rig.api, "POST", "/v1/enroll/nodes/alpha/revoke", Some(API_TOKEN), "").await;
    assert_eq!(s, 200);
    assert!(build(&rig, &alpha, "alpha").await.is_err());

    // ...and by the build host itself, independently of C2's forwarding decision.
    let direct = |bearer: Option<&str>, node: &str| {
        let body = serde_json::to_vec(&HostRequest { node_id: node.into(), recipe: "hello".into(), source_ref: None }).unwrap();
        let bearer = bearer.map(str::to_owned);
        let addr = rig.host_addr;
        async move { hatchery_build::http::post(addr, "/v1/build", bearer.as_deref(), &body, Duration::from_secs(10)).await.unwrap().0 }
    };
    assert_eq!(direct(None, "alpha").await, 401, "no bearer");
    assert_eq!(direct(Some("guess"), "alpha").await, 401, "wrong bearer");
    assert_eq!(direct(Some(&rig.host_bearer), "alpha").await, 403, "revoked node, valid bearer");
    assert_eq!(direct(Some(&rig.host_bearer), "nobody").await, 403, "unknown node, valid bearer");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn forged_replayed_stale_and_foreign_requests_are_refused() {
    let rig = rig().await;
    let alpha = enroll_node(&rig, "alpha").await;
    let beta = enroll_node(&rig, "beta").await;
    status(&rig, "alpha", "trusted").await;
    status(&rig, "beta", "trusted").await;
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64;
    let post = |req: &SignedBuildRequest| {
        let body = serde_json::to_vec(req).unwrap();
        let addr = rig.enroll;
        async move { hatchery_build::http::post(addr, "/v1/build", None, &body, Duration::from_secs(20)).await.unwrap().0 }
    };
    let sign = |who: &NodeIdentity, node: &str, ts: u64, nonce: &str| {
        let msg = SignedBuildRequest::signing_bytes(node, "hello", None, ts, nonce);
        SignedBuildRequest { node_id: node.into(), recipe: "hello".into(), source_ref: None, ts_ms: ts, nonce: nonce.into(), signature: who.sign_bytes(&msg) }
    };
    let good = sign(&alpha, "alpha", now, "aaaa1111");
    assert_eq!(post(&good).await, 200);
    assert_eq!(post(&good).await, 403, "replay of the same request");
    assert_eq!(post(&sign(&alpha, "alpha", now - 10 * 60 * 1000, "bbbb2222")).await, 403, "stale timestamp");
    assert_eq!(post(&sign(&beta, "alpha", now, "cccc3333")).await, 403, "beta signing as alpha");
    let mut tampered = sign(&alpha, "alpha", now, "dddd4444");
    tampered.recipe = "other".into();
    assert_eq!(post(&tampered).await, 403, "recipe changed after signing");
    assert_eq!(post(&sign(&alpha, "ghost", now, "eeee5555")).await, 403, "unknown node");
    let mut bad = sign(&alpha, "alpha", now, "ffff6666");
    bad.recipe = "../x".into();
    assert_eq!(post(&bad).await, 400, "malformed recipe name");
}
