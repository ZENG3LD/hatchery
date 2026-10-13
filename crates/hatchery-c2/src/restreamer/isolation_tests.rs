//! A2 isolation invariants (unit-level).

use crate::restreamer::{
    c2_cannot_complete_as_hq, c2_cannot_open_inner, c2_strip_outer, core_open_inner,
    noise_session_seal, BufferLimits, HqRecipientSecret, MemoryJournal, Restreamer, RestreamerKeys,
};
use std::sync::Arc;

fn token() -> String {
    "isolation-core-token!!".to_owned()
}

#[test]
fn node_sees_only_upstream_keys() {
    let hq = HqRecipientSecret::from_seed([21u8; 32]);
    let keys = RestreamerKeys::from_seeds([22u8; 32], hq.public_bytes().0);
    let (c2_pub, hq_pub) = keys.upstream_public_keys();
    assert_eq!(c2_pub.as_bytes().len(), 32);
    assert_eq!(hq_pub.as_bytes().len(), 32);
    let sealed = noise_session_seal(&keys, &hq, b"body").unwrap();
    let inner = c2_strip_outer(&keys, &sealed.outer).unwrap();
    assert!(c2_cannot_open_inner(&keys, &inner));
    assert!(c2_cannot_complete_as_hq(&keys, &sealed.handshake_msg1));
    let mut hq_tx = sealed.hq_transport;
    assert_eq!(core_open_inner(&mut hq_tx, &inner).unwrap(), b"body");
}

#[test]
fn restreamer_addresses_nodes_opaquely() {
    let journal = Arc::new(MemoryJournal::new());
    let rs = Restreamer::new(BufferLimits::default(), journal, token(), None);
    rs.admit_from_node("opaque-node", 1, b"x".to_vec()).unwrap();
    let envs = rs.buffered_after("opaque-node", 1, 0);
    assert_eq!(envs[0].node_id, "opaque-node");
    let bytes = envs[0].encode().unwrap();
    let text = String::from_utf8_lossy(&bytes);
    assert!(!text.contains("127.0.0.1"));
    assert!(!text.contains("::1"));
}

#[test]
fn c2_never_records_push_to_core() {
    let journal = Arc::new(MemoryJournal::new());
    let rs = Restreamer::new(BufferLimits::default(), journal, token(), None);
    assert!(!rs.ever_pushed_to_core());
    rs.admit_from_node("n", 1, b"p".to_vec()).unwrap();
    assert!(!rs.ever_pushed_to_core());
}
