use async_trait::async_trait;
use hatchery_build::*;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

struct Fake(Mutex<BTreeMap<String, bool>>);
#[async_trait]
impl Admission for Fake {
    async fn admitted(&self, node_id: &str) -> Result<bool, String> {
        Ok(self.0.lock().unwrap().get(node_id).copied().unwrap_or(false))
    }
}

fn host(dir: &std::path::Path, seed: [u8; 32]) -> (BuildHost, Arc<Fake>) {
    std::fs::write(dir.join("build.sh"), "echo \"built:$HATCHERY_SOURCE_REF:$HATCHERY_NODE_ID\" > out.bin; env > env.txt\n").unwrap();
    let recipes = Recipes {
        recipes: BTreeMap::from([(
            "hello".to_owned(),
            serde_json::from_value(serde_json::json!({
                "argv": ["sh", "build.sh"], "workdir": dir, "artifact": "out.bin", "timeout_secs": 20
            }))
            .unwrap(),
        )]),
    };
    let fake = Arc::new(Fake(Mutex::new(BTreeMap::from([("trusted1".to_owned(), true), ("gone".to_owned(), false)]))));
    (BuildHost::new(recipes, fake.clone(), Signer::from_seed(seed)), fake)
}

fn req(node: &str, recipe: &str) -> HostRequest {
    HostRequest { node_id: node.into(), recipe: recipe.into(), source_ref: Some("main".into()) }
}

#[tokio::test]
async fn admitted_node_gets_artifact_hash_and_a_signature_that_verifies() {
    let dir = tempfile::tempdir().unwrap();
    let (host, _) = host(dir.path(), [7; 32]);
    let response = host.build(&req("trusted1", "hello")).await.unwrap();
    let artifact = base64_decode(&response.artifact_b64);
    assert_eq!(artifact, b"built:main:trusted1\n");
    response.receipt.verify(&host.signer_public_key(), Some(&artifact)).unwrap();
    // The seed never appears in what the node receives.
    let wire = serde_json::to_string(&response).unwrap();
    let seed_b64 = base64_encode(&[7; 32]);
    assert!(!wire.contains(&seed_b64));
    // And the build did not see it either (cleared environment, only the documented variables).
    let env = std::fs::read_to_string(dir.path().join("env.txt")).unwrap();
    assert!(!env.contains(&seed_b64));
    assert!(env.lines().all(|l| ["PATH=", "HOME=", "HATCHERY_NODE_ID=", "HATCHERY_SOURCE_REF=", "PWD=", "SHLVL=", "_=", "OLDPWD="].iter().any(|p| l.starts_with(p))), "{env}");
}

#[tokio::test]
async fn tampering_or_a_foreign_signer_is_detected() {
    let dir = tempfile::tempdir().unwrap();
    let (host, _) = host(dir.path(), [7; 32]);
    let good = host.build(&req("trusted1", "hello")).await.unwrap();
    let key = host.signer_public_key();
    let mut bad = good.receipt.clone();
    bad.artifact_sha256 = "0".repeat(64);
    assert_eq!(bad.verify(&key, None), Err(ReceiptError::BadSignature));
    assert_eq!(good.receipt.verify(&key, Some(b"other")), Err(ReceiptError::ArtifactMismatch));
    let other = Signer::from_seed([9; 32]).public_key();
    assert_eq!(good.receipt.verify(&other, None), Err(ReceiptError::UntrustedSigner));
}

#[tokio::test]
async fn unadmitted_and_unknown_requests_are_refused_and_nothing_runs() {
    let dir = tempfile::tempdir().unwrap();
    let (host, fake) = host(dir.path(), [7; 32]);
    assert!(matches!(host.build(&req("gone", "hello")).await, Err(BuildError::Forbidden)));
    assert!(matches!(host.build(&req("stranger", "hello")).await, Err(BuildError::Forbidden)));
    assert!(!dir.path().join("out.bin").exists());
    assert!(matches!(host.build(&req("trusted1", "nope")).await, Err(BuildError::UnknownRecipe)));
    assert!(matches!(host.build(&req("trusted1", "../x")).await, Err(BuildError::BadRequest)));
    let mut with_bad_ref = req("trusted1", "hello");
    with_bad_ref.source_ref = Some("a/../../etc".into());
    assert!(matches!(host.build(&with_bad_ref).await, Err(BuildError::BadRequest)));
    // Suspended after having been trusted.
    host.build(&req("trusted1", "hello")).await.unwrap();
    fake.0.lock().unwrap().insert("trusted1".into(), false);
    assert!(matches!(host.build(&req("trusted1", "hello")).await, Err(BuildError::Forbidden)));
}

#[tokio::test]
async fn failing_build_reports_failure_and_no_receipt() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _) = host(dir.path(), [7; 32]);
    std::fs::write(dir.path().join("build.sh"), "echo boom >&2; exit 3\n").unwrap();
    let r = h.build(&req("trusted1", "hello")).await;
    assert!(matches!(r, Err(BuildError::Failed(ref m)) if m.contains("boom")), "{r:?}");
    let _ = &mut h;
}

#[test]
fn signing_key_file_must_be_private_and_is_never_overwritten() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sign.key");
    let made = Signer::generate_to_file(&path).unwrap();
    assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
    assert_eq!(Signer::from_file(&path).unwrap().public_key(), made.public_key());
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(Signer::from_file(&path).is_err());
}

#[test]
fn recipe_files_reject_escapes() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("r.json");
    std::fs::write(&p, r#"{"recipes":{"x":{"argv":["true"],"workdir":"/tmp","artifact":"../etc/passwd"}}}"#).unwrap();
    assert!(Recipes::load(&p).is_err());
    std::fs::write(&p, r#"{"recipes":{"x":{"argv":["true"],"workdir":"/tmp","artifact":"out"}}}"#).unwrap();
    assert!(Recipes::load(&p).is_ok());
}

fn base64_encode(b: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(b)
}
fn base64_decode(s: &str) -> Vec<u8> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.decode(s).unwrap()
}
