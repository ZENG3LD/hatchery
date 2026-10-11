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
