//! A store and tokens made by hatchery-m4a-signer 0.2.15 keep working, and signatures stay
//! byte-compatible in both directions.
use std::collections::BTreeSet;

#[test]
fn store_and_tokens_from_0215_open_and_sign() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("a.sealed");
    let prf = [7u8; 32];
    let old_human = signer_v0215::HumanKey::from_passkey_prf(&prf);
    let mut old = signer_v0215::Signer::init(&p, &old_human).unwrap();
    old.set_account("acct").unwrap();
    old.register_node("n1", &old_human).unwrap();
    let prov = old.provision("n1", "codex", "s1", signer_v0215::default_rights(), None).unwrap();
    let old_sig = old.sign(&prov.token, signer_v0215::RIGHT_SEND, b"payload").unwrap();
    drop(old);

    let human = hatchery_keystore::HumanKey::from_passkey_prf(&prf);
    let mut s = hatchery_keystore::Signer::open(&p);
    s.unlock(&human).unwrap();
    assert_eq!(s.account().unwrap(), "acct");
    assert_eq!(s.public_key_of("s1").unwrap(), prov.public_key);
    // old token, legacy and namespaced op names
    for op in [hatchery_keystore::RIGHT_SEND, hatchery_keystore::grants::M4A_SEND] {
        let sig = s.sign_from_node("n1", &prov.token, op, b"payload").unwrap();
        assert_eq!(sig.signature, old_sig.signature, "ed25519 is deterministic: same key, same message");
        let old_view = signer_v0215::Signature { nick: sig.nick.clone(), public_key: sig.public_key, signature: sig.signature };
        assert!(signer_v0215::verify_signature(&old_view, signer_v0215::RIGHT_SEND, b"payload"));
    }
    // new token on the same store still verifies with the old library's verifier
    let p2 = s.provision("n1", "codex", "s2", hatchery_keystore::default_rights(), None).unwrap();
    let sig = s.sign(&p2.token, hatchery_keystore::grants::M4A_SEND, b"x").unwrap();
    let old_view = signer_v0215::Signature { nick: sig.nick, public_key: sig.public_key, signature: sig.signature };
    assert!(signer_v0215::verify_signature(&old_view, signer_v0215::RIGHT_SEND, b"x"));
    drop(s);

    // and 0.2.15 can still open a store written by the keystore (rollback path)
    let mut back = signer_v0215::Signer::open(&p);
    back.unlock(&old_human).unwrap();
    let t2 = back.sign(&p2.token, signer_v0215::RIGHT_SEND, b"x");
    assert!(t2.is_ok(), "rollback: 0.2.15 accepts tokens issued by the keystore: {t2:?}");
}

#[test]
fn prf_salt_is_unchanged() {
    assert_eq!(hatchery_keystore::PRF_SALT, signer_v0215::PRF_SALT);
}

#[test]
fn delegation_is_attenuated() {
    let dir = tempfile::tempdir().unwrap();
    let human = hatchery_keystore::HumanKey::from_bytes([1; 32]);
    let mut s = hatchery_keystore::Signer::init(dir.path().join("a"), &human).unwrap();
    s.set_account("a").unwrap();
    s.register_node("n", &human).unwrap();
    let mut r = hatchery_keystore::default_rights();
    r.insert(hatchery_keystore::grants::SESSION_SPAWN_CHILD.into());
    let p = s.provision("n", "codex", "s", r, Some(&human)).unwrap();
    let d = s.delegate(&p.token, "m4a").unwrap();
    assert!(s.sign(&d, hatchery_keystore::grants::M4A_SEND, b"m").is_ok());
    assert!(s.provision_child(&d, "n", "codex", "c", BTreeSet::new()).is_err(), "delegated m4a cap cannot spawn");
    assert!(s.delegate(&p.token, "nosuch").is_err());
}

#[test]
fn keyproof_signing_is_restricted() {
    let dir = tempfile::tempdir().unwrap();
    let human = hatchery_keystore::HumanKey::from_bytes([2; 32]);
    let mut s = hatchery_keystore::Signer::init(dir.path().join("a"), &human).unwrap();
    s.set_account("a").unwrap();
    s.register_node("n", &human).unwrap();
    s.register_node("m", &human).unwrap();
    let p = s.provision("n", "codex", "s", hatchery_keystore::default_rights(), None).unwrap();
    let msg = b"m4a-key-login-v1\naud\nkid\ncid\nnonce\n1\n";
    let sig = s.sign_m4a_keyproof("n", &p.token, msg).unwrap();
    assert_eq!(sig.public_key, p.public_key);
    use ed25519_dalek::Verifier;
    let vk = ed25519_dalek::VerifyingKey::from_bytes(&sig.public_key).unwrap();
    assert!(vk.verify(msg, &ed25519_dalek::Signature::from_bytes(&sig.signature)).is_ok());
    assert!(s.sign_m4a_keyproof("n", &p.token, b"hatchery-m4a/v1\0x").is_err(), "letters cannot be signed raw");
    assert!(s.sign_m4a_keyproof("m", &p.token, msg).is_err(), "other node");
}
