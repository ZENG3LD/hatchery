#![cfg(unix)]
//! (helpers copied from the enrollment e2e)
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


use hatchery_c2_client::{connect_local, C2Client};
use hatchery_node::protocol::NodeRequest;

fn start_with_policy(dir: &Dir, ports: &Ports, prov: Arc<Recording>, policy: &Path) -> C2Config {
    let mut config = start_config(dir, ports, prov, false);
    if let Some(enroll) = config.enrollment.as_mut() {
        enroll.policy_path = Some(policy.to_owned());
    }
    config
}

async fn snapshot_verdict(api: std::net::SocketAddr, control: &hatchery_c2_client::C2ControlHandle, id: &str) -> String {
    let client = C2Client::new(api, API_TOKEN).unwrap().with_deadline(Duration::from_secs(2));
    let status = client.status().await.unwrap();
    let node_id = NodeId::new(id).unwrap();
    let cursor = status.nodes[&node_id].cursor.expect("online node has a cursor");
    let route = hatchery_c2::protocol::NodeRoute { node_id, expected_incarnation_id: cursor.incarnation_id };
    format!("{:?}", control.request(route, NodeRequest::Snapshot).await)
}

async fn inventory_visible(api: std::net::SocketAddr, id: &str) -> bool {
    let (_, body) = http(api, "GET", "/status", Some(API_TOKEN), "").await;
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    !v["nodes"][id]["inventory"].is_null()
}

async fn allows(api: std::net::SocketAddr, id: &str, cap: &str) -> bool {
    let (s, body) = http(api, "POST", &format!("/v1/enroll/nodes/{id}/allows"), Some(API_TOKEN), &format!(r#"{{"capability":"{cap}"}}"#)).await;
    assert_eq!(s, 200, "{body}");
    serde_json::from_str::<serde_json::Value>(&body).unwrap()["allowed"].as_bool().unwrap()
}

async fn set_status(api: std::net::SocketAddr, id: &str, status: &str) -> u16 {
    http(api, "POST", &format!("/v1/enroll/nodes/{id}/status"), Some(API_TOKEN), &format!(r#"{{"status":"{status}"}}"#)).await.0
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn policy_is_enforced_per_status_reloaded_and_survives_restart() {
    let dir = Dir::new();
    let ports = Ports {
        api: format!("127.0.0.1:{}", free_port()).parse().unwrap(),
        enroll: format!("127.0.0.1:{}", free_port()).parse().unwrap(),
        call_home: format!("127.0.0.1:{}", free_port()).parse().unwrap(),
    };
    let api = ports.api;
    let policy_file = dir.p("policy.json");
    std::fs::write(&policy_file, "{}").unwrap();
    let c2 = C2Running::start(start_with_policy(&dir, &ports, Arc::new(Recording::default()), &policy_file)).await.unwrap();

    // Enroll node A and run it.
    let keys = dir.p("keys-a");
    let one = code(api).await;
    enroll_client::enroll(ports.enroll, "pola", NodeKind::Qemu, Some(&one), &keys).await.unwrap();
    let identity = Arc::new(NodeIdentity::load_or_create(&keys).unwrap());
    let node = node_server(&dir, "pola", Some(Arc::clone(&identity)), ports.call_home, "local-only-token-pola");
    let (shutdown, task) = (node.shutdown_handle(), tokio::spawn(node.run()));
    wait_online(api, "pola", true, 30).await;
    let (control, _events) = timeout(Duration::from_secs(5), connect_local(&dir.p("c2.sock").to_string_lossy(), API_TOKEN)).await.unwrap().unwrap();

    // NEUTRAL: may call home and heartbeat; no inventory, operator requests refused, nothing else.
    assert!(allows(api, "pola", "heartbeat").await);
    for cap in ["attach", "tasks", "tools"] {
        assert!(!allows(api, "pola", cap).await, "neutral must not hold {cap}");
    }
    assert!(!inventory_visible(api, "pola").await, "neutral shows liveness only");
    let verdict = snapshot_verdict(api, &control, "pola").await;
    assert!(verdict.contains("RequestForbidden"), "neutral request must be forbidden: {verdict}");

    // MEMBER: attach works, inventory visible, tasks/tools still refused.
    assert_eq!(set_status(api, "pola", "member").await, 200);
    assert!(allows(api, "pola", "attach").await && !allows(api, "pola", "tasks").await);
    timeout(Duration::from_secs(10), async { while !inventory_visible(api, "pola").await { sleep(Duration::from_millis(100)).await } }).await.unwrap();
    let verdict = snapshot_verdict(api, &control, "pola").await;
    assert!(verdict.contains("Snapshot"), "member may attach: {verdict}");

    // TRUSTED: tasks and tools too.
    assert_eq!(set_status(api, "pola", "trusted").await, 200);
    assert!(allows(api, "pola", "tasks").await && allows(api, "pola", "tools").await);

    // SUSPENDED: nothing; the node is dropped and stays out.
    assert_eq!(set_status(api, "pola", "suspended").await, 200);
    wait_online(api, "pola", false, 30).await;
    assert!(!allows(api, "pola", "heartbeat").await);
    sleep(Duration::from_secs(3)).await;
    assert!(!is_online(api, "pola").await, "a suspended node cannot call home");

    // Back to MEMBER (suspension is reversible): it reconnects by itself.
    assert_eq!(set_status(api, "pola", "member").await, 200);
    wait_online(api, "pola", true, 40).await;

    // POLICY RELOAD: member loses attach -> requests refused at once; the node stays connected.
    std::fs::write(&policy_file, r#"{"member":{"mailbox":true,"rooms":["general"],"capabilities":["heartbeat","mail"]}}"#).unwrap();
    let (s, _) = http(api, "POST", "/v1/enroll/policy/reload", Some(API_TOKEN), "").await;
    assert_eq!(s, 200);
    assert!(!allows(api, "pola", "attach").await);
    assert!(is_online(api, "pola").await);
    timeout(Duration::from_secs(10), async { while inventory_visible(api, "pola").await { sleep(Duration::from_millis(100)).await } }).await.unwrap();
    let verdict = snapshot_verdict(api, &control, "pola").await;
    assert!(verdict.contains("RequestForbidden"), "{verdict}");

    // A broken policy file is refused and the previous policy stays in force.
    std::fs::write(&policy_file, r#"{"member":{"capabilities":["atach"]}}"#).unwrap();
    let (s, body) = http(api, "POST", "/v1/enroll/policy/reload", Some(API_TOKEN), "").await;
    assert_eq!(s, 409, "{body}");
    assert!(!allows(api, "pola", "attach").await && allows(api, "pola", "mail").await);

    // Policy without heartbeat for member: the node is dropped, and a C2 restart keeps it out.
    std::fs::write(&policy_file, r#"{"member":{"capabilities":["mail"]}}"#).unwrap();
    assert_eq!(http(api, "POST", "/v1/enroll/policy/reload", Some(API_TOKEN), "").await.0, 200);
    wait_online(api, "pola", false, 30).await;
    drop(c2);
    sleep(Duration::from_millis(500)).await;
    let c2 = C2Running::start(start_with_policy(&dir, &ports, Arc::new(Recording::default()), &policy_file)).await.unwrap();
    assert_eq!(record_status(api, "pola").await.as_deref(), Some("member"));
    assert!(!allows(api, "pola", "heartbeat").await, "policy file is read again at start");
    sleep(Duration::from_secs(4)).await;
    assert!(!is_online(api, "pola").await);

    // Fix the file, reload: the node comes back.
    std::fs::write(&policy_file, "{}").unwrap();
    assert_eq!(http(api, "POST", "/v1/enroll/policy/reload", Some(API_TOKEN), "").await.0, 200);
    wait_online(api, "pola", true, 40).await;

    // REVOKED: dropped for good, with no capability whatever the policy says.
    assert_eq!(http(api, "POST", "/v1/enroll/nodes/pola/revoke", Some(API_TOKEN), "").await.0, 200);
    wait_online(api, "pola", false, 30).await;
    assert!(!allows(api, "pola", "heartbeat").await);
    assert_eq!(set_status(api, "pola", "member").await, 409);

    // Policy endpoints need the operator bearer.
    assert_eq!(http(api, "GET", "/v1/enroll/policy", None, "").await.0, 401);
    stop(shutdown, task).await;
    drop(c2);
}
