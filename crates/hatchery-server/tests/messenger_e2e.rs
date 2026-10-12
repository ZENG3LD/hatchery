#![cfg(all(unix, feature = "m4a"))]
//! P6 end to end: real C2 + two enrolled nodes (member, trusted) and a neutral one. The server
//! provisions from C2 (P2), the messenger and task manager read the ledger, and every node call
//! goes node -> signed -> C2 (policy) -> server control API (ledger re-check).

use hatchery_c2::enroll_service::EnrollConfig;
use hatchery_c2::{C2Config, C2Running};
use hatchery_enroll::{NodeIdentity, NodeKind};
use hatchery_node::enroll_client::node_call;
use hatchery_server::hub::Hub;
use hatchery_server::provision::{ProvisionConfig, ProvisionSubservice};
use hatchery_server::{control, Subservice, Supervisor};
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::{sleep, timeout};

const OPERATOR: &str = "operator-token-for-p6-test-0123";
const CONTROL: &str = "server-control-token-for-p6-test";
const MASTER_VAR: &str = "HATCHERY_P6_TEST_MASTER";
const TOKEN_VAR: &str = "HATCHERY_P6_TEST_C2_TOKEN";

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

async fn http(addr: &str, method: &str, path: &str, bearer: Option<&str>, body: &str) -> (u16, Value) {
    let mut tries = 0;
    let mut s = loop {
        match TcpStream::connect(addr).await {
            Ok(s) => break s,
            Err(_) if tries < 50 => {
                tries += 1;
                sleep(Duration::from_millis(100)).await;
            }
            Err(e) => panic!("connect {addr}: {e}"),
        }
    };
    let auth = bearer.map(|t| format!("Authorization: Bearer {t}\r\n")).unwrap_or_default();
    s.write_all(format!("{method} {path} HTTP/1.1\r\nHost: x\r\n{auth}Content-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
    let mut raw = Vec::new();
    s.read_to_end(&mut raw).await.unwrap();
    let text = String::from_utf8_lossy(&raw).into_owned();
    let body = text.split("\r\n\r\n").nth(1).unwrap_or_default();
    (text.split_whitespace().nth(1).unwrap().parse().unwrap(), serde_json::from_str(body).unwrap_or(Value::Null))
}

struct Node {
    id: &'static str,
    identity: NodeIdentity,
}

async fn enroll(api: &str, enroll_addr: &str, id: &'static str, seed: u8) -> Node {
    let (_, body) = http(api, "POST", "/v1/enroll/codes", Some(OPERATOR), "{}").await;
    let code = body["code"].as_str().unwrap().to_owned();
    let identity = NodeIdentity::from_seeds([seed; 32], [seed + 1; 32]);
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64;
    let request = identity.sign_request(id, NodeKind::Qemu, Some(&code), now).unwrap();
    let (status, _) = http(enroll_addr, "POST", "/v1/enroll", None, &serde_json::to_string(&request).unwrap()).await;
    assert_eq!(status, 200);
    Node { id, identity }
}

fn ledger_status(dir: &Path, id: &str) -> Option<String> {
    let v: Value = serde_json::from_slice(&std::fs::read(dir.join("provision/ledger.json")).ok()?).ok()?;
    v["nodes"][id]["status"].as_str().map(str::to_owned)
}

async fn wait_ledger(dir: &Path, id: &str, status: &str) {
    timeout(Duration::from_secs(20), async {
        while ledger_status(dir, id).as_deref() != Some(status) {
            sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("ledger never showed {id}={status}"));
}

async fn set_status(api: &str, id: &str, status: &str) {
    let (s, _) = http(api, "POST", &format!("/v1/enroll/nodes/{id}/status"), Some(OPERATOR), &format!(r#"{{"status":"{status}"}}"#)).await;
    assert_eq!(s, 200);
}

struct Rig {
    api: String,
    enroll_addr: SocketAddr,
    control: String,
    data: std::path::PathBuf,
    _dir: tempfile::TempDir,
    _c2: C2Running,
    _svc: ProvisionSubservice,
}

async fn rig() -> Rig {
    std::env::set_var(MASTER_VAR, "p6-test-master-secret-0123456789");
    std::env::set_var(TOKEN_VAR, OPERATOR);
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    std::fs::create_dir_all(&data).unwrap();
    let sockdir = dir.path().join("sock");
    {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new().mode(0o700).create(&sockdir).unwrap();
    }
    let api = format!("127.0.0.1:{}", free_port());
    let enroll_addr: SocketAddr = format!("127.0.0.1:{}", free_port()).parse().unwrap();
    let control_addr: SocketAddr = format!("127.0.0.1:{}", free_port()).parse().unwrap();

    // Server: control API with the hub (messenger + tasks).
    let hub = {
        let data = data.clone();
        Arc::new(tokio::task::spawn_blocking(move || Hub::open(&data).unwrap()).await.unwrap())
    };
    let listener = control::bind(control_addr).await.unwrap();
    tokio::spawn(control::serve_with_hub(listener, Supervisor::new(Vec::new()), Some(CONTROL.to_owned()), Some(hub)));

    let mut enroll_cfg = EnrollConfig::new(enroll_addr);
    enroll_cfg.registry_path = Some(dir.path().join("registry.json"));
    enroll_cfg.server_upstream = Some((control_addr, CONTROL.to_owned()));
    let c2 = C2Running::start(
        C2Config::new(api.parse().unwrap(), OPERATOR, Vec::new())
            .unwrap()
            .with_control_endpoint(sockdir.join("c2.sock").to_string_lossy().into_owned())
            .unwrap()
            .with_node_listen(format!("127.0.0.1:{}", free_port()).parse().unwrap())
            .unwrap()
            .with_enrollment(enroll_cfg),
    )
    .await
    .unwrap();
    let mut svc = ProvisionSubservice::new(ProvisionConfig {
        data_dir: data.clone(),
        master_key_var: MASTER_VAR.into(),
        c2_addr: api.clone(),
        c2_token_var: TOKEN_VAR.into(),
        interval: Duration::from_millis(150),
        server_ref: "https://messenger.test".into(),
        mail_domain: "nodes.test".into(),
        policy_path: None,
    });
    svc.start().unwrap();
    Rig { api, enroll_addr, control: control_addr.to_string(), data, _dir: dir, _c2: c2, _svc: svc }
}

async fn call(rig: &Rig, n: &Node, op: &str, payload: Value) -> Result<Value, String> {
    node_call(rig.enroll_addr, &n.identity, n.id, op, &payload).await.map_err(|e| e.to_string())
}

fn refused(r: &Result<Value, String>) -> bool {
    r.as_ref().is_err_and(|e| e.contains("refused"))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn mail_tasks_status_and_revocation_end_to_end() {
    let rig = rig().await;
    let api = rig.api.clone();
    let enroll_addr = rig.enroll_addr.to_string();
    let alpha = enroll(&api, &enroll_addr, "alpha", 10).await;
    let beta = enroll(&api, &enroll_addr, "beta", 20).await;
    let gamma = enroll(&api, &enroll_addr, "gamma", 30).await;
    for id in ["alpha", "beta", "gamma"] {
        wait_ledger(&rig.data, id, "neutral").await;
    }

    // Neutral: nothing beyond lobby/heartbeat; every mail and task call is refused.
    for n in [&alpha, &beta, &gamma] {
        assert!(refused(&call(&rig, n, "mail.send", json!({"to":"beta","subject":"s","body":"b"})).await));
        assert!(refused(&call(&rig, n, "mail.inbox", json!({})).await));
        assert!(refused(&call(&rig, n, "task.list", json!({})).await));
    }

    // alpha -> member, beta -> trusted; gamma stays neutral.
    set_status(&api, "alpha", "member").await;
    set_status(&api, "beta", "trusted").await;
    wait_ledger(&rig.data, "alpha", "member").await;
    wait_ledger(&rig.data, "beta", "trusted").await;

    // Mail A -> B, delivered, read, acknowledged. The sender is the authenticated node.
    let sent = call(&rig, &alpha, "mail.send", json!({"to":"beta","subject":"hello","body":"from alpha to beta","idempotency_key":"k1"})).await.unwrap();
    let id = sent["message_id"].as_str().unwrap().to_owned();
    let again = call(&rig, &alpha, "mail.send", json!({"to":"beta","subject":"hello","body":"from alpha to beta","idempotency_key":"k1"})).await.unwrap();
    assert_eq!(again["message_id"], id, "idempotent retry creates nothing new");
    let inbox = call(&rig, &beta, "mail.inbox", json!({})).await.unwrap();
    let msgs = inbox["messages"].as_array().unwrap();
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0]["from"], "alpha");
    assert_eq!(msgs[0]["body"], "from alpha to beta");
    // A payload cannot choose the sender.
    let spoof = call(&rig, &alpha, "mail.send", json!({"to":"beta","subject":"s","body":"b","from":"beta"})).await.unwrap();
    let inbox = call(&rig, &beta, "mail.inbox", json!({})).await.unwrap();
    assert!(inbox["messages"].as_array().unwrap().iter().all(|m| m["from"] == "alpha"), "{spoof} {inbox}");
    call(&rig, &beta, "mail.ack", json!({"message_id": id})).await.unwrap();
    // The neutral node can neither be written to nor read for.
    assert!(refused(&call(&rig, &beta, "mail.send", json!({"to":"gamma","subject":"s","body":"b"})).await));
    assert!(refused(&call(&rig, &gamma, "mail.inbox", json!({})).await));

    // Room mail follows the ledger rooms: both are in `general`.
    call(&rig, &alpha, "mail.send", json!({"to":"room:general","subject":"all","body":"to the room"})).await.unwrap();
    let inbox = call(&rig, &beta, "mail.inbox", json!({})).await.unwrap();
    assert!(inbox["messages"].as_array().unwrap().iter().any(|m| m["body"] == "to the room"));

    // Operator (HQ) mail both ways through the control API.
    let (s, _) = http(&rig.control, "POST", "/v1/mail/send", Some(CONTROL), r#"{"to":"beta","subject":"hq","body":"hello from HQ"}"#).await;
    assert_eq!(s, 200);
    let inbox = call(&rig, &beta, "mail.inbox", json!({})).await.unwrap();
    assert!(inbox["messages"].as_array().unwrap().iter().any(|m| m["from"] == "hatchery.operator" && m["body"] == "hello from HQ"));
    call(&rig, &beta, "mail.send", json!({"to":"hatchery.operator","subject":"re","body":"reply"})).await.unwrap();
    let (s, body) = http(&rig.control, "POST", "/v1/mail/inbox", Some(CONTROL), "{}").await;
    assert_eq!(s, 200);
    assert!(body["messages"].as_array().unwrap().iter().any(|m| m["from"] == "beta" && m["body"] == "reply"));
    let (s, _) = http(&rig.control, "POST", "/v1/mail/send", None, r#"{"to":"beta","subject":"x","body":"x"}"#).await;
    assert_eq!(s, 401, "operator routes need the control bearer");

    // Tasks: only a trusted node (holds `tasks`) can be assigned; it sees exactly its own.
    let (s, _) = http(&rig.control, "POST", "/v1/tasks", Some(CONTROL), r#"{"title":"nope","assignee":"alpha"}"#).await;
    assert_eq!(s, 403, "member does not hold tasks");
    let (s, body) = http(&rig.control, "POST", "/v1/tasks", Some(CONTROL), r#"{"title":"build it","body":"details","assignee":"beta"}"#).await;
    assert_eq!(s, 200);
    let task_id = body["task"]["id"].as_i64().unwrap();
    let mine = call(&rig, &beta, "task.list", json!({})).await.unwrap();
    assert_eq!(mine["tasks"].as_array().unwrap().len(), 1);
    assert_eq!(mine["tasks"][0]["title"], "build it");
    assert!(refused(&call(&rig, &alpha, "task.list", json!({})).await), "member cannot read tasks");
    assert!(refused(&call(&rig, &alpha, "task.update", json!({"id": task_id, "status": "done"})).await));
    call(&rig, &beta, "task.update", json!({"id": task_id, "status": "in_progress"})).await.unwrap();
    call(&rig, &beta, "task.update", json!({"id": task_id, "status": "done"})).await.unwrap();
    assert!(call(&rig, &beta, "task.list", json!({})).await.unwrap()["tasks"].as_array().unwrap().is_empty());
    let (_, all) = http(&rig.control, "GET", "/v1/tasks?assignee=beta", Some(CONTROL), "").await;
    assert_eq!(all["tasks"][0]["status"], "done");

    // Restart persistence: a second hub on the same data dir sees the same mail and tasks.
    let data = rig.data.clone();
    tokio::task::spawn_blocking(move || {
        let reopened = Hub::open(&data).unwrap();
        let after = reopened.node_call("beta", "mail.inbox", &json!({})).unwrap();
        assert!(after["messages"].as_array().unwrap().len() >= 3);
        assert_eq!(reopened.operator_call("GET", "/v1/tasks", &json!({})).unwrap()["tasks"].as_array().unwrap().len(), 1);
    })
    .await
    .unwrap();

    // Replay of a signed call is refused (same bytes twice).
    {
        use hatchery_enroll::call::SignedCall;
        let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64;
        let payload = "{}".to_owned();
        let msg = SignedCall::signing_bytes("beta", "mail.inbox", &payload, ts, "replaynonce1");
        let signed = SignedCall { node_id: "beta".into(), op: "mail.inbox".into(), payload, ts_ms: ts, nonce: "replaynonce1".into(), signature: beta.identity.sign_bytes(&msg) };
        let body = serde_json::to_string(&signed).unwrap();
        let (first, _) = http(&enroll_addr, "POST", "/v1/node-call", None, &body).await;
        let (second, _) = http(&enroll_addr, "POST", "/v1/node-call", None, &body).await;
        assert_eq!((first, second), (200, 403));
        // Another node cannot sign as beta.
        let forged = SignedCall { signature: alpha.identity.sign_bytes(&msg), nonce: "replaynonce2".into(), ..signed.clone() };
        let (s, _) = http(&enroll_addr, "POST", "/v1/node-call", None, &serde_json::to_string(&forged).unwrap()).await;
        assert_eq!(s, 403);
    }

    // Revoked is cut off: C2 refuses, and the server's own ledger check refuses a direct call too.
    let (s, _) = http(&api, "POST", "/v1/enroll/nodes/beta/revoke", Some(OPERATOR), "").await;
    assert_eq!(s, 200);
    assert!(refused(&call(&rig, &beta, "mail.inbox", json!({})).await));
    wait_ledger(&rig.data, "beta", "revoked").await;
    let direct = http(&rig.control, "POST", "/v1/node-call", Some(CONTROL), r#"{"node_id":"beta","op":"mail.inbox","payload":{}}"#).await;
    assert_eq!(direct.0, 403);
    assert!(refused(&call(&rig, &alpha, "mail.send", json!({"to":"beta","subject":"s","body":"b"})).await), "revoked node is no longer a mailbox");
    // Suspension also removes a member from the mailbox, and a status flip back restores it.
    set_status(&api, "alpha", "suspended").await;
    wait_ledger(&rig.data, "alpha", "suspended").await;
    assert!(refused(&call(&rig, &alpha, "mail.inbox", json!({})).await));
}

/// The agent path: MCP tool -> node door (signs with the node identity) -> C2 -> hub. The agent
/// side holds no key; a neutral node's door answers "not available".
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn agent_tools_reach_the_hub_through_the_node_door() {
    use hatchery_harness_mcp::WorkDoor;
    let rig = rig().await;
    let enroll_addr = rig.enroll_addr.to_string();
    let beta = enroll(&rig.api, &enroll_addr, "beta", 20).await;
    let gamma = enroll(&rig.api, &enroll_addr, "gamma", 30).await;
    wait_ledger(&rig.data, "beta", "neutral").await;
    set_status(&rig.api, "beta", "trusted").await;
    wait_ledger(&rig.data, "beta", "trusted").await;
    let (s, _) = http(&rig.control, "POST", "/v1/tasks", Some(CONTROL), r#"{"title":"via door","assignee":"beta"}"#).await;
    assert_eq!(s, 200);

    let mut doors = Vec::new();
    for n in [beta, gamma] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(hatchery_node::door::serve(listener, rig.enroll_addr, Arc::new(n.identity), n.id.to_owned()));
        doors.push(WorkDoor::new(addr).unwrap());
    }
    let (beta_door, gamma_door) = (doors.remove(0), doors.remove(0));
    let (ok, no) = tokio::task::spawn_blocking(move || (beta_door.call("g4a_tasks", json!({})), gamma_door.call("g4a_tasks", json!({})))).await.unwrap();
    let ok: Value = serde_json::from_str(&ok.unwrap()).unwrap();
    assert_eq!(ok["tasks"][0]["title"], "via door");
    assert!(no.is_err(), "neutral node gets nothing through its door");
}
