#![allow(deprecated)]
use hatchery_m4a_signer::*;

fn tmp(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("m4a-signer-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d.join("store.sealed")
}
fn human() -> HumanKey {
    HumanKey::from_bytes([7u8; 32])
}

#[test]
fn locked_signer_neither_provisions_nor_signs_and_wrong_human_key_cannot_unlock() {
    let p = tmp("locked");
    let mut s = Signer::init(&p, &human()).unwrap(); for n in ["n1", "box", "n"] { s.register_node(n, &human()).unwrap(); }
    let a = s.provision("n1", "codex", "sa", default_rights(), None).unwrap();
    s.lock();
    assert_eq!(s.sign(&a.token, RIGHT_SEND, b"x").unwrap_err(), SignerError::Locked);
    assert_eq!(s.provision("n1", "codex", "sb", default_rights(), None).unwrap_err(), SignerError::Locked);
    let mut reopened = Signer::open(&p);
    assert_eq!(reopened.unlock(&HumanKey::from_bytes([8u8; 32])).unwrap_err(), SignerError::WrongHumanKey);
    reopened.unlock(&human()).unwrap();
    assert!(reopened.sign(&a.token, RIGHT_SEND, b"x").is_ok());
}

#[test]
fn sealed_store_on_disk_contains_no_key_material() {
    let p = tmp("disk");
    let mut s = Signer::init(&p, &human()).unwrap(); for n in ["n1", "box", "n"] { s.register_node(n, &human()).unwrap(); }
    let a = s.provision("n1", "codex", "sa", default_rights(), None).unwrap();
    let raw = std::fs::read(&p).unwrap();
    assert!(!raw.windows(a.nick.len()).any(|w| w == a.nick.as_bytes()), "store must be sealed");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o600);
    }
}

#[test]
fn a_session_signs_only_as_itself_never_as_its_neighbour() {
    let mut s = Signer::init(tmp("neigh"), &human()).unwrap(); for n in ["n1", "box", "n"] { s.register_node(n, &human()).unwrap(); }
    let a = s.provision("n1", "codex", "sa", default_rights(), None).unwrap();
    let b = s.provision("n1", "claude-code", "sb", default_rights(), None).unwrap();
    let sig = s.sign(&a.token, RIGHT_SEND, b"hello").unwrap();
    assert_eq!(sig.nick, a.nick);
    assert_ne!(sig.nick, b.nick);
    assert!(verify_signature(&sig, RIGHT_SEND, b"hello"));
    assert_ne!(sig.public_key, s.public_key_of("sb").unwrap());
    // A forged token (own token re-encoded with another nick) is rejected by the root key.
    let forged = a.token.replace('A', "B");
    assert!(s.sign(&forged, RIGHT_SEND, b"x").is_err());
    // Biscuit attenuation by the holder can only narrow: appending a block that claims the
    // neighbour's right gives no authority (facts in appended blocks are not trusted).
    let ub = biscuit_auth::UnverifiedBiscuit::from_base64(&a.token).unwrap();
    let blk = biscuit_auth::macros::block!(r#"right("send_as", {n}); nick({n});"#, n = b.nick.as_str());
    let widened = ub.append(blk).unwrap().to_base64().unwrap();
    let sig = s.sign(&widened, RIGHT_SEND, b"x");
    assert!(sig.as_ref().map(|g| g.nick == a.nick).unwrap_or(true), "appended block must not grant the neighbour nick: {sig:?}");
}

#[test]
fn respawn_rotates_and_stop_revokes_the_capability() {
    let mut s = Signer::init(tmp("rot"), &human()).unwrap(); for n in ["n1", "box", "n"] { s.register_node(n, &human()).unwrap(); }
    let first = s.provision("n1", "codex", "sa", default_rights(), None).unwrap();
    let second = s.provision("n1", "codex", "sa", default_rights(), None).unwrap();
    assert_eq!(first.nick, second.nick, "same session keeps its nick");
    assert!(matches!(s.sign(&first.token, RIGHT_SEND, b"x"), Err(SignerError::Denied(_))));
    assert!(s.sign(&second.token, RIGHT_SEND, b"x").is_ok());
    s.revoke_session("sa").unwrap();
    assert!(s.sign(&second.token, RIGHT_SEND, b"x").is_err());
}

#[test]
fn child_sessions_need_spawn_child_get_new_identity_and_no_wider_rights() {
    let mut s = Signer::init(tmp("child"), &human()).unwrap(); for n in ["n1", "box", "n"] { s.register_node(n, &human()).unwrap(); }
    // spawn_child needs the human key.
    let mut with_spawn = default_rights();
    with_spawn.insert(RIGHT_SPAWN_CHILD.into());
    assert_eq!(
        s.provision("n1", "codex", "p", with_spawn.clone(), None).unwrap_err(),
        SignerError::HumanApprovalRequired
    );
    assert_eq!(
        s.provision("n1", "codex", "p", with_spawn.clone(), Some(&HumanKey::from_bytes([1; 32]))).unwrap_err(),
        SignerError::WrongHumanKey
    );
    let parent = s.provision("n1", "codex", "p", with_spawn, Some(&human())).unwrap();
    let plain = s.provision("n1", "codex", "q", default_rights(), None).unwrap();
    // Without spawn_child, no children.
    assert!(s.provision_child(&plain.token, "n1", "codex", "q-child", default_rights()).is_err());
    // Child rights must be a subset.
    let mut wider = default_rights();
    wider.insert("admin".into());
    assert!(s.provision_child(&parent.token, "n1", "codex", "c0", wider).is_err());
    let child = s.provision_child(&parent.token, "n1", "codex", "c1", default_rights()).unwrap();
    assert_ne!(child.nick, parent.nick);
    assert_eq!(s.parent_of("c1").unwrap().as_deref(), Some("p"));
    // Parent's token cannot sign as the child; child's signature names the child.
    assert_eq!(s.sign(&parent.token, RIGHT_SEND, b"m").unwrap().nick, parent.nick);
    assert_eq!(s.sign(&child.token, RIGHT_SEND, b"m").unwrap().nick, child.nick);
    // Child cannot spawn further (spawn_child not granted to it).
    assert!(s.provision_child(&child.token, "n1", "codex", "c2", default_rights()).is_err());
}

#[test]
fn retire_needs_the_human_key() {
    let mut s = Signer::init(tmp("retire"), &human()).unwrap(); for n in ["n1", "box", "n"] { s.register_node(n, &human()).unwrap(); }
    let a = s.provision("n1", "codex", "sa", default_rights(), None).unwrap();
    assert!(s.retire("sa", &HumanKey::from_bytes([2; 32])).is_err());
    s.retire("sa", &human()).unwrap();
    assert_eq!(s.sign(&a.token, RIGHT_SEND, b"x").unwrap_err(), SignerError::Retired);
    assert_eq!(s.provision("n1", "codex", "sa", default_rights(), None).unwrap_err(), SignerError::Retired);
}
