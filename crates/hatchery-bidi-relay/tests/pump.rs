//! Byte-pump integration test: opaque frames go node->hq, opaque input goes
//! hq->node, every byte is checked by SHA-256, and a dropped peer can
//! reconnect and still receive what was sent while it was away.

use futures_util::{SinkExt, StreamExt};
use hatchery_bidi_relay::{bind, serve, RelayLog};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

async fn open_session(addr: std::net::SocketAddr, id: &str) {
    let mut s = TcpStream::connect(addr).await.unwrap();
    let req = format!("POST /session/{id} HTTP/1.1\r\nHost: x\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
    s.write_all(req.as_bytes()).await.unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).await.unwrap();
    assert!(out.starts_with("HTTP/1.1 200"), "{out}");
}

async fn ws(addr: std::net::SocketAddr, id: &str, role: &str) -> Ws {
    connect_async(format!("ws://{addr}/session/{id}/{role}")).await.unwrap().0
}

fn payload(seed: u8, len: usize) -> Vec<u8> {
    (0..len).map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed)).collect()
}

async fn next_bin(w: &mut Ws) -> Vec<u8> {
    loop {
        match tokio::time::timeout(std::time::Duration::from_secs(5), w.next()).await.unwrap().unwrap().unwrap() {
            Message::Binary(b) => return b.to_vec(),
            _ => continue,
        }
    }
}

async fn start() -> std::net::SocketAddr {
    let (listener, addr) = bind("127.0.0.1:0".parse().unwrap()).await.unwrap();
    tokio::spawn(serve(listener, RelayLog::new(|_| {})));
    addr
}

#[tokio::test]
async fn both_directions_keep_every_byte() {
    let addr = start().await;
    open_session(addr, "p1").await;
    let mut node = ws(addr, "p1", "node").await;
    let mut hq = ws(addr, "p1", "hq").await;
    for round in 0..16u8 {
        let frame = payload(round, 150_000 + round as usize);
        let want = Sha256::digest(&frame);
        node.send(Message::binary(frame)).await.unwrap();
        assert_eq!(Sha256::digest(next_bin(&mut hq).await), want, "node->hq round {round}");
        let input = payload(round.wrapping_add(100), 20 + round as usize);
        let want = Sha256::digest(&input);
        hq.send(Message::binary(input)).await.unwrap();
        assert_eq!(Sha256::digest(next_bin(&mut node).await), want, "hq->node round {round}");
    }
}

#[tokio::test]
async fn reconnecting_peer_receives_what_waited_for_it() {
    let addr = start().await;
    open_session(addr, "p2").await;
    let mut node = ws(addr, "p2", "node").await;
    let mut hq = ws(addr, "p2", "hq").await;
    node.send(Message::binary(payload(1, 1000))).await.unwrap();
    assert_eq!(next_bin(&mut hq).await, payload(1, 1000));
    hq.close(None).await.ok();
    drop(hq);
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let waiting = [payload(2, 5000), payload(3, 6000)];
    for f in &waiting {
        node.send(Message::binary(f.clone())).await.unwrap();
    }
    let mut hq = ws(addr, "p2", "hq").await;
    for f in &waiting {
        assert_eq!(Sha256::digest(next_bin(&mut hq).await), Sha256::digest(f));
    }
    node.send(Message::binary(payload(4, 777))).await.unwrap();
    assert_eq!(next_bin(&mut hq).await, payload(4, 777));
}

#[tokio::test]
async fn frames_sent_before_the_peer_connects_arrive_in_order_both_ways() {
    let addr = start().await;
    for (id, first, second) in [("p3", "hq", "node"), ("p4", "node", "hq")] {
        open_session(addr, id).await;
        let mut early = ws(addr, id, first).await;
        let sent: Vec<Vec<u8>> = (0..40u8).map(|i| payload(i, 3000 + i as usize)).collect();
        for f in &sent {
            early.send(Message::binary(f.clone())).await.unwrap();
        }
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        let mut late = ws(addr, id, second).await;
        for (i, f) in sent.iter().enumerate() {
            assert_eq!(Sha256::digest(next_bin(&mut late).await), Sha256::digest(f), "{first}->{second} frame {i}");
        }
    }
}

#[tokio::test]
async fn overflow_drops_oldest_counts_and_warns() {
    use hatchery_bidi_relay::RelayLimits;
    use std::sync::{Arc, Mutex};
    let lines = Arc::new(Mutex::new(Vec::<String>::new()));
    let sink = Arc::clone(&lines);
    let (listener, addr) = bind("127.0.0.1:0".parse().unwrap()).await.unwrap();
    let log = RelayLog::new(move |l| sink.lock().unwrap().push(l))
        .with_limits(RelayLimits { max_pending_frames: 8, max_pending_bytes: 1 << 20 });
    tokio::spawn(serve(listener, log));
    open_session(addr, "p5").await;
    let mut early = ws(addr, "p5", "hq").await;
    for i in 0..20u8 {
        early.send(Message::binary(payload(i, 500))).await.unwrap();
    }
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let mut late = ws(addr, "p5", "node").await;
    // the newest 8 survive, in order
    for i in 12..20u8 {
        assert_eq!(next_bin(&mut late).await, payload(i, 500));
    }
    let seen = lines.lock().unwrap().join("\n");
    assert!(seen.contains("WARN session=p5"), "{seen}");
    assert!(seen.contains("total_dropped=12"), "{seen}");
}

#[tokio::test]
async fn input_reaches_a_node_that_keeps_streaming_to_a_late_hq() {
    let addr = start().await;
    open_session(addr, "p6").await;
    let node = ws(addr, "p6", "node").await;
    let (mut node_tx, mut node_rx) = node.split();
    let streamer = tokio::spawn(async move {
        for i in 0..200u8 {
            if node_tx.send(Message::binary(payload(i, 19_580))).await.is_err() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    });
    tokio::time::sleep(std::time::Duration::from_secs(2)).await; // ~40 frames pile up for hq
    let mut hq = ws(addr, "p6", "hq").await;
    for _ in 0..5 {
        next_bin(&mut hq).await;
    }
    let click = payload(9, 20);
    hq.send(Message::binary(click.clone())).await.unwrap();
    let got = tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            if let Some(Ok(Message::Binary(b))) = node_rx.next().await {
                return b.to_vec();
            }
        }
    })
    .await
    .expect("click did not reach the node");
    assert_eq!(got, click);
    streamer.abort();
}
