#![cfg(all(unix, feature = "m4a"))]
//! P2 end to end: a real C2 with enrollment, nodes enroll with one-time codes, the server's
//! provisioning subservice polls C2 and keeps the m4a identities, mailboxes and grants in
//! line with every enrollment and status change.

use hatchery_c2::enroll_service::EnrollConfig;
use hatchery_c2::{C2Config, C2Running};
use hatchery_enroll::{NodeIdentity, NodeKind};
use hatchery_server::provision::{LedgerEntryView, ProvisionConfig, ProvisionSubservice};
use hatchery_server::Subservice;
use std::path::Path;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::{sleep, timeout};

const OPERATOR: &str = "operator-token-for-p2-test-0123";
const MASTER_VAR: &str = "HATCHERY_P2_TEST_MASTER";
const TOKEN_VAR: &str = "HATCHERY_P2_TEST_C2_TOKEN";

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

async fn http(addr: &str, method: &str, path: &str, bearer: Option<&str>, body: &str) -> (u16, String) {
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
    s.write_all(format!("{method} {path} HTTP/1.1\r\nHost: x\r\n{auth}Content-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes())
        .await
        .unwrap();
    let mut raw = Vec::new();
    s.read_to_end(&mut raw).await.unwrap();
    let text = String::from_utf8_lossy(&raw).into_owned();
    (text.split_whitespace().nth(1).unwrap().parse().unwrap(), text.split("\r\n\r\n").nth(1).unwrap_or_default().to_owned())
}

async fn enroll(api: &str, enroll_addr: &str, id: &str, seed: u8) -> NodeIdentity {
    let (_, body) = http(api, "POST", "/v1/enroll/codes", Some(OPERATOR), "{}").await;
    let code = serde_json::from_str::<serde_json::Value>(&body).unwrap()["code"].as_str().unwrap().to_owned();
    let identity = NodeIdentity::from_seeds([seed; 32], [seed + 1; 32]);
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64;
    let request = identity.sign_request(id, NodeKind::Qemu, Some(&code), now).unwrap();
    let (status, _) = http(enroll_addr, "POST", "/v1/enroll", None, &serde_json::to_string(&request).unwrap()).await;
    assert_eq!(status, 200);
    identity
}

fn ledger(dir: &Path) -> Vec<LedgerEntryView> {
    match std::fs::read(dir.join("provision/ledger.json")) {
        Ok(bytes) => {
            let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            v["nodes"].as_object().unwrap().values().map(|e| serde_json::from_value(e.clone()).unwrap()).collect()
        }
        Err(_) => Vec::new(),
    }
}

async fn wait_for(dir: &Path, what: &str, check: impl Fn(&[LedgerEntryView]) -> bool) {
    timeout(Duration::from_secs(20), async {
        while !check(&ledger(dir)) {
            sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("ledger never showed: {what}: {:?}", ledger(dir)));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn enrollment_and_status_flips_provision_the_server_side() {
    let _ = tracing_subscriber::fmt().with_test_writer().try_init();
    std::env::set_var(MASTER_VAR, "p2-test-master-secret-0123456789");
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
    let enroll_addr = format!("127.0.0.1:{}", free_port());
    let mut enroll_cfg = EnrollConfig::new(enroll_addr.parse().unwrap());
    enroll_cfg.registry_path = Some(dir.path().join("registry.json"));
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

    let mut service = ProvisionSubservice::new(ProvisionConfig {
        data_dir: data.clone(),
        master_key_var: MASTER_VAR.into(),
        c2_addr: api.clone(),
        c2_token_var: TOKEN_VAR.into(),
        interval: Duration::from_millis(200),
        server_ref: "https://messenger.test".into(),
        mail_domain: "nodes.test".into(),
        policy_path: None,
    });
    service.start().unwrap();

    // Nothing enrolled yet: nothing provisioned.
    sleep(Duration::from_millis(600)).await;
    assert!(ledger(&data).is_empty());

    // Enrollment (neutral): identity exists, almost no grants, no mailbox.
    enroll(&api, &enroll_addr, "lab1", 10).await;
    enroll(&api, &enroll_addr, "lab2", 20).await;
    wait_for(&data, "both neutral", |l| l.len() == 2 && l.iter().all(|e| e.status == "neutral")).await;
    let neutral = ledger(&data).into_iter().find(|e| e.node_id == "lab1").unwrap();
    assert!(neutral.m4a_public_key.is_some());
    assert!(neutral.mailbox.is_none());
    assert_eq!(neutral.grants.rooms, ["lobby"]);

    // Approval: member gets a mailbox and more rooms; the key is the same.
    let (s, _) = http(&api, "POST", "/v1/enroll/nodes/lab1/status", Some(OPERATOR), r#"{"status":"member"}"#).await;
    assert_eq!(s, 200);
    wait_for(&data, "lab1 member", |l| l.iter().any(|e| e.node_id == "lab1" && e.status == "member")).await;
    let member = ledger(&data).into_iter().find(|e| e.node_id == "lab1").unwrap();
    assert_eq!(member.mailbox.as_deref(), Some("lab1@nodes.test"));
    assert!(member.grants.rooms.len() > neutral.grants.rooms.len());
    assert_eq!(member.m4a_public_key, neutral.m4a_public_key);

    // Trusted gets more still.
    http(&api, "POST", "/v1/enroll/nodes/lab1/status", Some(OPERATOR), r#"{"status":"trusted"}"#).await;
    wait_for(&data, "lab1 trusted", |l| l.iter().any(|e| e.node_id == "lab1" && e.status == "trusted" && e.grants.rooms.contains(&"ops".to_owned()))).await;

    // Suspend lab2: grants gone, key kept. Revoke lab1: identity deleted, grants gone.
    http(&api, "POST", "/v1/enroll/nodes/lab2/status", Some(OPERATOR), r#"{"status":"suspended"}"#).await;
    http(&api, "POST", "/v1/enroll/nodes/lab1/revoke", Some(OPERATOR), "").await;
    wait_for(&data, "lab1 revoked and lab2 suspended", |l| {
        l.iter().any(|e| e.node_id == "lab1" && e.status == "revoked")
            && l.iter().any(|e| e.node_id == "lab2" && e.status == "suspended")
    })
    .await;
    let l = ledger(&data);
    let lab1 = l.iter().find(|e| e.node_id == "lab1").unwrap();
    let lab2 = l.iter().find(|e| e.node_id == "lab2").unwrap();
    assert!(lab1.m4a_public_key.is_none() && lab1.grants.rooms.is_empty() && lab1.mailbox.is_none());
    assert!(lab2.m4a_public_key.is_some() && lab2.grants.rooms.is_empty() && lab2.mailbox.is_none());

    // The ledger and the data directory never hold the master key or a private seed value.
    let ledger_text = std::fs::read_to_string(data.join("provision/ledger.json")).unwrap();
    assert!(!ledger_text.contains("p2-test-master-secret") && !ledger_text.contains("seed"));
    assert_eq!(service.status().state, hatchery_server::State::Running);

    // Server restart: a new provisioner reads the ledger and changes nothing.
    service.stop();
    let before = std::fs::read(data.join("provision/ledger.json")).unwrap();
    service.start().unwrap();
    sleep(Duration::from_secs(1)).await;
    assert_eq!(std::fs::read(data.join("provision/ledger.json")).unwrap(), before, "restart is idempotent");
    service.stop();
    drop(c2);
}
