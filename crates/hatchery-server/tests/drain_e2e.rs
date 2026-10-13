//! A2 / A3 live path over TCP: node -> C2 uplink -> core drain -> core, and back down.
//! The Noise_NK handshake runs as DATA frames on the live streams; C2 strips only its
//! outer layer and never sees plaintext.

use hatchery_c2::restreamer::{serve, BufferLimits, MemoryJournal, Restreamer, RestreamerConfig, RestreamerKeys};
use hatchery_c2_client::DrainClient;
use hatchery_c2_protocol::nested::HqRecipientSecret;
use hatchery_c2_protocol::ChannelKind;
use hatchery_node::restream_session::{NodeSession, UpstreamKeys};
use hatchery_server::drain_consumer::{CoreHandler, DrainConsumer, NodeStream};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;

const CORE_TOKEN: &str = "drain-test-token-0123456789abcdef";

fn free_addr() -> SocketAddr {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap()
}

pub struct Edge {
    pub node: SocketAddr,
    pub drain: SocketAddr,
    pub keys: UpstreamKeys,
    pub rs: Restreamer,
    pub stop: watch::Sender<bool>,
}

pub async fn edge(recipient: &HqRecipientSecret) -> Edge {
    let keys = RestreamerKeys::generate(hatchery_c2::restreamer::PublicKeyBytes(recipient.public_bytes().0));
    let (c2_public, recipient_public) = keys.upstream_public_keys();
    let rs = Restreamer::new(BufferLimits::default(), Arc::new(MemoryJournal::new()), CORE_TOKEN, Some(keys));
    let (node, drain) = (free_addr(), free_addr());
    let (stop, rx) = watch::channel(false);
    let cfg = RestreamerConfig { node_listen: node, drain_listen: drain, buffer: BufferLimits::default(), core_token: CORE_TOKEN.into() };
    let srv = rs.clone();
    tokio::spawn(async move { serve(cfg, srv, rx).await });
    tokio::time::sleep(Duration::from_millis(30)).await;
    Edge {
        node,
        drain,
        keys: UpstreamKeys {
            c2_public: hatchery_c2_protocol::nested::PublicKeyBytes(c2_public.0),
            recipient_public: hatchery_c2_protocol::nested::PublicKeyBytes(recipient_public.0),
        },
        rs,
        stop,
    }
}

pub fn consume(edge: &Edge, recipient: &HqRecipientSecret, node_id: &str, channel: u32, handler: CoreHandler) -> watch::Sender<bool> {
    let (stop, rx) = watch::channel(false);
    let consumer = DrainConsumer::new(
        DrainClient::new(edge.drain, CORE_TOKEN),
        recipient.clone(),
        NodeStream { node_id: node_id.into(), channel_id: channel, kind: ChannelKind::M4a },
        handler,
    );
    tokio::spawn(consumer.run(rx));
    stop
}

pub async fn node(edge: &Edge, node_id: &str, channel: u32) -> NodeSession {
    NodeSession::connect(edge.node, node_id, channel, ChannelKind::M4a, edge.keys, None, Duration::from_secs(5))
        .await
        .expect("node session + handshake")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn noise_handshake_runs_as_data_frames_end_to_end() {
    let recipient = HqRecipientSecret::generate();
    let edge = edge(&recipient).await;
    let seen = Arc::new(std::sync::Mutex::new(Vec::<(String, Vec<u8>)>::new()));
    let log = Arc::clone(&seen);
    let handler: CoreHandler = Arc::new(move |node: &str, _k, plain: &[u8]| {
        log.lock().unwrap().push((node.to_owned(), plain.to_vec()));
        let mut r = b"echo:".to_vec();
        r.extend_from_slice(plain);
        Some(r)
    });
    let stop = consume(&edge, &recipient, "node-a", 1, Arc::clone(&handler));
    let stop2 = consume(&edge, &recipient, "node-a", 2, handler);

    let mut s = node(&edge, "node-a", 1).await;
    for msg in [&b"first secret body"[..], b"second secret body"] {
        s.send(msg).await.unwrap();
        let reply = s.recv(Duration::from_secs(5)).await.unwrap();
        assert_eq!(reply, [&b"echo:"[..], msg].concat());
    }
    assert_eq!(seen.lock().unwrap().len(), 2);
    assert_eq!(seen.lock().unwrap()[0].0, "node-a");

    // C2 holds only inner ciphertext: no plaintext in what it buffered or relayed.
    for env in edge.rs.buffered_after("node-a", 1, 0) {
        assert!(!env.frame.payload.windows(6).any(|w| w == b"secret"));
    }
    assert!(!edge.rs.ever_pushed_to_core());

    // A node pinned to the wrong recipient key never completes the handshake.
    let mut wrong = edge.keys;
    wrong.recipient_public = HqRecipientSecret::generate().public_bytes();
    let bad = NodeSession::connect(edge.node, "node-a", 2, ChannelKind::M4a, wrong, None, Duration::from_millis(600)).await;
    assert!(bad.is_err());
    assert_eq!(seen.lock().unwrap().len(), 2, "core handled nothing from the wrong-key session");
    let _ = stop2.send(true);

    let _ = stop.send(true);
    let _ = edge.stop.send(true);
}
