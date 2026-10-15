use hatchery_m4a_signer::keyring::Keyring;
use hatchery_m4a_signer::*;

fn dir(n: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("m4a-keyring-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}
fn alice() -> HumanKey { HumanKey::from_passkey_prf(b"alice-passkey-prf-output-32bytes") }
fn bob() -> HumanKey { HumanKey::from_passkey_prf(b"bob---passkey-prf-output-32bytes") }

#[test]
fn each_passkey_unlocks_only_its_own_account_tree() {
    let d = dir("iso");
    let mut k = Keyring::new(&d);
    k.enroll("alice", &alice()).unwrap();
    k.enroll("bob", &bob()).unwrap();
    k.branch("alice").unwrap().register_node("alice-laptop", &alice()).unwrap();
    k.branch("bob").unwrap().register_node("bob-box", &bob()).unwrap();
    // Bob's passkey cannot register a node in Alice's tree.
    assert!(k.branch("alice").unwrap().register_node("evil", &bob()).is_err());
    // Alice cannot provision a session on Bob's node from her tree.
    assert!(k.provision("alice", "bob-box", "codex", "s1", default_rights(), None).is_err());
    let a = k.provision("alice", "alice-laptop", "codex", "a1", default_rights(), None).unwrap();
    let b = k.provision("bob", "bob-box", "claude-code", "b1", default_rights(), None).unwrap();
    assert_eq!(Keyring::account_of(&a.token).unwrap(), "alice");
    assert_eq!(k.sign(&a.token, RIGHT_SEND, b"m").unwrap().nick, a.nick);
    assert_eq!(k.sign(&b.token, RIGHT_SEND, b"m").unwrap().nick, b.nick);

    // Locking Bob stops only Bob.
    k.lock("bob");
    assert_eq!(k.sign(&b.token, RIGHT_SEND, b"m").unwrap_err(), SignerError::Locked);
    assert!(k.sign(&a.token, RIGHT_SEND, b"m").is_ok());

    // Alice's passkey does not open Bob's store; Bob's own does.
    let mut k2 = Keyring::new(&d);
    assert_eq!(k2.unlock("bob", &alice()).unwrap_err(), SignerError::WrongHumanKey);
    k2.unlock("bob", &bob()).unwrap();
    assert!(k2.sign(&b.token, RIGHT_SEND, b"m").is_ok());
    // Public keys are shareable (room rules); the same key comes out of the tree after reopen.
    assert_eq!(k2.public_key_of("bob", "b1").unwrap(), b.public_key);
}

#[test]
fn a_token_cannot_be_rerouted_into_another_humans_branch() {
    let d = dir("route");
    let mut k = Keyring::new(&d);
    k.enroll("alice", &alice()).unwrap();
    k.enroll("bob", &bob()).unwrap();
    k.branch("alice").unwrap().register_node("n", &alice()).unwrap();
    let a = k.provision("alice", "n", "codex", "a1", default_rights(), None).unwrap();
    // Verifying Alice's token inside Bob's branch fails: different Biscuit root.
    assert!(k.branch("bob").unwrap().sign(&a.token, RIGHT_SEND, b"m").is_err());
}

#[test]
fn key_tree_is_deterministic_per_account_node_session_and_distinct_across_humans() {
    let mut k = Keyring::new(dir("tree"));
    k.enroll("alice", &alice()).unwrap();
    k.enroll("bob", &bob()).unwrap();
    k.branch("alice").unwrap().register_node("n", &alice()).unwrap();
    k.branch("bob").unwrap().register_node("n", &bob()).unwrap();
    let a = k.provision("alice", "n", "codex", "same", default_rights(), None).unwrap();
    let b = k.provision("bob", "n", "codex", "same", default_rights(), None).unwrap();
    assert_ne!(a.public_key, b.public_key, "same node/session ids under two humans -> different keys");
    let a2 = k.provision("alice", "n", "codex", "other", default_rights(), None).unwrap();
    assert_ne!(a.public_key, a2.public_key);
}

#[test]
fn a_node_cannot_use_another_nodes_capability_and_unregistered_nodes_get_nothing() {
    let mut k = Keyring::new(dir("nodes"));
    k.enroll("alice", &alice()).unwrap();
    assert!(k.provision("alice", "stranger", "codex", "x", default_rights(), None).is_err());
    k.branch("alice").unwrap().register_node("n1", &alice()).unwrap();
    k.branch("alice").unwrap().register_node("n2", &alice()).unwrap();
    let a = k.provision("alice", "n1", "codex", "a1", default_rights(), None).unwrap();
    assert!(k.sign_from_node("n1", &a.token, RIGHT_SEND, b"m").is_ok());
    assert!(k.sign_from_node("n2", &a.token, RIGHT_SEND, b"m").is_err());
    assert_eq!(k.account_for_node("n2").as_deref(), Some("alice"));
    assert_eq!(k.account_for_node("stranger"), None);
}
