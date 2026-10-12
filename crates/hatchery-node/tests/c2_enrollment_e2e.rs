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

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn enrollment_neutral_first_approval_revoke_replay_and_restart() {
    let dir = Dir::new();
    let ports = Ports {
        api: format!("127.0.0.1:{}", free_port()).parse().unwrap(),
        enroll: format!("127.0.0.1:{}", free_port()).parse().unwrap(),
        call_home: format!("127.0.0.1:{}", free_port()).parse().unwrap(),
    };
    let prov = Arc::new(Recording::default());
    let c2 = C2Running::start(start_config(&dir, &ports, Arc::clone(&prov), true)).await.unwrap();
    let api = ports.api;

    // Operator endpoints need the bearer.
    assert_eq!(http(api, "GET", "/v1/enroll/nodes", None, "").await.0, 401);

    // 1. No code, no peer.
    let keys_a = dir.p("keys-a");
    let refused = enroll_client::enroll(ports.enroll, "alpha", NodeKind::Qemu, None, &keys_a).await;
    assert!(matches!(refused, Err(enroll_client::EnrollClientError::Refused)));
    let refused = enroll_client::enroll(ports.enroll, "alpha", NodeKind::Qemu, Some("not-a-real-code"), &keys_a).await;
    assert!(matches!(refused, Err(enroll_client::EnrollClientError::Refused)));
    assert!(prov.0.lock().unwrap().is_empty(), "no WireGuard peer without a valid code");
    assert_eq!(record_status(api, "alpha").await, None);

    // 2. Valid one-time code: enrolled as neutral, address assigned, peer added.
    let one_time = code(api).await;
    let state = enroll_client::enroll(ports.enroll, "alpha", NodeKind::Qemu, Some(&one_time), &keys_a).await.unwrap();
    assert_eq!(state.response.status, "neutral");
    assert_eq!(state.response.tunnel_ip.as_deref(), Some("10.79.0.2"));
    assert_eq!(state.response.c2_call_home.as_deref(), Some(ports.call_home.to_string().as_str()));
    assert_eq!(prov.0.lock().unwrap().len(), 1);
    assert!(prov.0.lock().unwrap()[0].ends_with("10.79.0.2"));
    // The keys stay on the node: private files exist with mode 0600 and the answer holds no secret.
    use std::os::unix::fs::PermissionsExt;
    for f in ["identity.key", "wg.key", "enrollment.json"] {
        assert_eq!(std::fs::metadata(keys_a.join(f)).unwrap().permissions().mode() & 0o777, 0o600, "{f}");
    }
    assert!(!std::fs::read_to_string(keys_a.join("enrollment.json")).unwrap().contains(&one_time));

    // 3. Replay: the same code cannot enroll another node; the same request again changes nothing.
    let other = enroll_client::enroll(ports.enroll, "beta", NodeKind::Docker, Some(&one_time), &dir.p("keys-b")).await;
    assert!(matches!(other, Err(enroll_client::EnrollClientError::Refused)), "code is single use");
    assert_eq!(record_status(api, "beta").await, None);
    let identity_a = Arc::new(NodeIdentity::load_or_create(&keys_a).unwrap());
    let replay = identity_a.sign_request("alpha", NodeKind::Qemu, Some(&one_time), 1).unwrap();
    let (status, _) = http(ports.enroll, "POST", "/v1/enroll", None, &serde_json::to_string(&replay).unwrap()).await;
    assert_eq!(status, 403, "a stale replayed request is refused");

    // 4. Signed call-home: neutral node is online with no standing secret.
    let node = node_server(&dir, "alpha", Some(Arc::clone(&identity_a)), ports.call_home, "local-only-token-alpha");
    let (shutdown, task) = (node.shutdown_handle(), tokio::spawn(node.run()));
    wait_online(api, "alpha", true, 30).await;
    assert_eq!(record_status(api, "alpha").await.as_deref(), Some("neutral"));

    // 5. Approval flips the status; the connection stays up.
    let (s, body) = http(api, "POST", "/v1/enroll/nodes/alpha/status", Some(API_TOKEN), r#"{"status":"member"}"#).await;
    assert_eq!(s, 200, "{body}");
    assert_eq!(record_status(api, "alpha").await.as_deref(), Some("member"));
    assert!(is_online(api, "alpha").await);

    // 6. Token mode is unchanged next to enrolled nodes.
    let tok = node_server(&dir, "tok1", None, ports.call_home, "token-node-secret-token");
    let (tok_shutdown, tok_task) = (tok.shutdown_handle(), tokio::spawn(tok.run()));
    wait_online(api, "tok1", true, 30).await;

    // 7. An impostor with another key under the same node id is refused.
    stop(shutdown, task).await;
    wait_online(api, "alpha", false, 30).await;
    let impostor = Arc::new(NodeIdentity::from_seeds([9; 32], [8; 32]));
    let fake = node_server(&dir, "alpha", Some(impostor), ports.call_home, "local-only-token-fake");
    let (fake_shutdown, fake_task) = (fake.shutdown_handle(), tokio::spawn(fake.run()));
    sleep(Duration::from_secs(3)).await;
    assert!(!is_online(api, "alpha").await, "wrong key must not authenticate");
    stop(fake_shutdown, fake_task).await;

    // 8. Restart persistence: registry reloads, peer re-applied, the node reconnects, status kept.
    let (s, _) = http(api, "POST", "/v1/enroll/nodes/alpha/status", Some(API_TOKEN), r#"{"status":"trusted"}"#).await;
    assert_eq!(s, 200);
    let node = node_server(&dir, "alpha", Some(Arc::clone(&identity_a)), ports.call_home, "local-only-token-alpha");
    let (shutdown, task) = (node.shutdown_handle(), tokio::spawn(node.run()));
    wait_online(api, "alpha", true, 30).await;
    drop(c2);
    let prov2 = Arc::new(Recording::default());
    sleep(Duration::from_millis(500)).await;
    let c2 = C2Running::start(start_config(&dir, &ports, Arc::clone(&prov2), true)).await.unwrap();
    assert_eq!(record_status(api, "alpha").await.as_deref(), Some("trusted"), "status survives the restart");
    assert!(prov2.0.lock().unwrap().iter().any(|l| l.starts_with("add ") && l.ends_with("10.79.0.2")), "peer re-applied at start");
    wait_online(api, "alpha", true, 40).await;

    // 9. Revoke is final: disconnects, peer removed, no way back.
    let (s, _) = http(api, "POST", "/v1/enroll/nodes/alpha/revoke", Some(API_TOKEN), "").await;
    assert_eq!(s, 200);
    wait_online(api, "alpha", false, 30).await;
    sleep(Duration::from_secs(3)).await;
    assert!(!is_online(api, "alpha").await, "a revoked node stays out although it keeps calling");
    assert!(prov2.0.lock().unwrap().iter().any(|l| l.starts_with("remove ")));
    let (s, _) = http(api, "POST", "/v1/enroll/nodes/alpha/status", Some(API_TOKEN), r#"{"status":"member"}"#).await;
    assert_eq!(s, 409, "revocation cannot be undone");
    let again = enroll_client::enroll(ports.enroll, "alpha", NodeKind::Qemu, Some(&code(api).await), &keys_a).await;
    assert!(matches!(again, Err(enroll_client::EnrollClientError::Refused)));
    assert_eq!(record_status(api, "alpha").await.as_deref(), Some("revoked"));
    assert!(is_online(api, "tok1").await, "token node unaffected by the revocation");

    stop(shutdown, task).await;
    stop(tok_shutdown, tok_task).await;
    drop(c2);
    let _ = Path::new("");
}
