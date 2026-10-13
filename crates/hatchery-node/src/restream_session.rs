//! Node end of a live restreamer session (A2 / A3).
//!
//! The node dials C2's uplink, OPENs a channel, then runs the inner Noise_NK
//! handshake toward the relayed recipient key as ordinary DATA frames:
//! msg1 goes up through C2 into the core drain, msg2 comes back down the same
//! stream. After that every request and reply is Noise transport ciphertext,
//! wrapped in C2's outer layer on the way up. C2 only ever sees opaque bytes,
//! and the node never learns what sits behind C2.

use crate::uplink_journal::UplinkJournal;
use hatchery_core::inner::{decode_inner, encode_inner, InnerKind};
use hatchery_core::nested::{node_seal_outer, NoiseInitiator, NoiseTransport, PublicKeyBytes};
use hatchery_core::{ChannelKind, Frame, FrameError, FrameType, OpenPayload};
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

/// Keys C2 hands a node at enrollment: its own outer key and the upstream recipient key.
#[derive(Clone, Copy, Debug)]
pub struct UpstreamKeys {
    pub c2_public: PublicKeyBytes,
    pub recipient_public: PublicKeyBytes,
}

pub struct NodeSession {
    channel_id: u32,
    keys: UpstreamKeys,
    out: mpsc::Sender<Frame>,
    down: mpsc::Receiver<Vec<u8>>,
    transport: NoiseTransport,
    journal: Option<Arc<UplinkJournal>>,
    tasks: Vec<JoinHandle<()>>,
}

impl Drop for NodeSession {
    fn drop(&mut self) {
        for t in &self.tasks {
            t.abort();
        }
    }
}

impl NodeSession {
    /// Dial, OPEN, and complete the Noise_NK handshake over DATA frames.
    pub async fn connect(
        uplink: SocketAddr,
        node_id: &str,
        channel_id: u32,
        kind: ChannelKind,
        keys: UpstreamKeys,
        journal: Option<Arc<UplinkJournal>>,
        handshake_timeout: Duration,
    ) -> Result<Self, String> {
        let stream = TcpStream::connect(uplink).await.map_err(|e| e.to_string())?;
        let (mut rd, mut wr) = stream.into_split();
        let (out_tx, mut out_rx) = mpsc::channel::<Frame>(64);
        let (down_tx, down_rx) = mpsc::channel::<Vec<u8>>(64);

        let writer = tokio::spawn(async move {
            while let Some(frame) = out_rx.recv().await {
                let Ok(bytes) = frame.encode() else { break };
                if wr.write_all(&bytes).await.is_err() {
                    break;
                }
            }
        });
        let reader_out = out_tx.clone();
        let reader_journal = journal.clone();
        let reader = tokio::spawn(async move {
            let mut buf = vec![0u8; 128 * 1024];
            let mut filled = 0usize;
            loop {
                let Ok(n) = rd.read(&mut buf[filled..]).await else { break };
                if n == 0 {
                    break;
                }
                filled += n;
                loop {
                    match Frame::decode(&buf[..filled]) {
                        Ok((frame, used)) => {
                            buf.copy_within(used..filled, 0);
                            filled -= used;
                            match frame.frame_type {
                                FrameType::Data => {
                                    if down_tx.send(frame.payload).await.is_err() {
                                        return;
                                    }
                                }
                                FrameType::JournalReq => {
                                    let Some(j) = reader_journal.as_ref() else { continue };
                                    let limit = frame.journal_req_limit().unwrap_or(64) as usize;
                                    if let Ok(body) = j.journal().reply_body(frame.channel_id, frame.seq, limit) {
                                        if let Ok(reply) = Frame::journal_reply(frame.channel_id, &body) {
                                            let _ = reader_out.send(reply).await;
                                        }
                                    }
                                }
                                FrameType::GoAway | FrameType::Close => return,
                                _ => {}
                            }
                        }
                        Err(FrameError::Truncated) => break,
                        Err(_) => return,
                    }
                }
                if filled == buf.len() {
                    return;
                }
            }
        });

        let open = Frame::open(
            channel_id,
            &OpenPayload { channel_kind: kind, node_id: node_id.to_owned() },
        )
        .map_err(|e| e.to_string())?;
        out_tx.send(open).await.map_err(|_| "uplink closed".to_owned())?;

        let mut initiator = NoiseInitiator::new(keys.recipient_public).map_err(|e| e.to_string())?;
        let msg1 = initiator.write_message1().map_err(|e| e.to_string())?;
        let mut session_tmp = Partial { channel_id, keys, out: out_tx.clone(), journal: journal.clone() };
        session_tmp.send_inner(&encode_inner(InnerKind::Handshake1, &msg1)).await?;

        let mut down_rx = down_rx;
        let msg2 = tokio::time::timeout(handshake_timeout, async {
            loop {
                let Some(bytes) = down_rx.recv().await else { return Err("uplink closed".to_owned()) };
                if let Some((InnerKind::Handshake2, body)) = decode_inner(&bytes) {
                    return Ok(body.to_vec());
                }
            }
        })
        .await
        .map_err(|_| "handshake timeout".to_owned())??;
        let transport = initiator.read_message2(&msg2).map_err(|e| e.to_string())?;

        Ok(Self {
            channel_id,
            keys,
            out: out_tx,
            down: down_rx,
            transport,
            journal,
            tasks: vec![writer, reader],
        })
    }

    /// Encrypt and send one inner message toward core.
    pub async fn send(&mut self, plaintext: &[u8]) -> Result<(), String> {
        let ct = self.transport.encrypt(plaintext).map_err(|e| e.to_string())?;
        let inner = encode_inner(InnerKind::Transport, &ct);
        Partial { channel_id: self.channel_id, keys: self.keys, out: self.out.clone(), journal: self.journal.clone() }
            .send_inner(&inner)
            .await
    }

    /// Next decrypted message from core.
    pub async fn recv(&mut self, timeout: Duration) -> Result<Vec<u8>, String> {
        tokio::time::timeout(timeout, async {
            loop {
                let Some(bytes) = self.down.recv().await else { return Err("uplink closed".to_owned()) };
                if let Some((InnerKind::Transport, ct)) = decode_inner(&bytes) {
                    return self.transport.decrypt(ct).map_err(|e| e.to_string());
                }
            }
        })
        .await
        .map_err(|_| "reply timeout".to_owned())?
    }

    /// Request/response helper for node calls (`mail.*`, `task.*`). The node id is never
    /// in the payload: core takes it from the stream the envelope arrived on.
    pub async fn call(&mut self, op: &str, payload: &Value, timeout: Duration) -> Result<Value, String> {
        let req = serde_json::to_vec(&json!({ "op": op, "payload": payload })).map_err(|e| e.to_string())?;
        self.send(&req).await?;
        let reply = self.recv(timeout).await?;
        let reply: Value = serde_json::from_slice(&reply).map_err(|e| e.to_string())?;
        let status = reply.get("status").and_then(Value::as_u64).unwrap_or(500);
        let body = reply.get("body").cloned().unwrap_or(Value::Null);
        if status == 200 {
            Ok(body)
        } else {
            let msg = body.get("error").and_then(Value::as_str).unwrap_or("error");
            Err(format!("{status}: {msg}"))
        }
    }
}

struct Partial {
    channel_id: u32,
    keys: UpstreamKeys,
    out: mpsc::Sender<Frame>,
    journal: Option<Arc<UplinkJournal>>,
}

impl Partial {
    async fn send_inner(&mut self, inner: &[u8]) -> Result<(), String> {
        // Journal the inner bytes (what C2 forwards after its layer) for gap recovery.
        if let Some(j) = &self.journal {
            j.append(self.channel_id, inner)?;
        }
        let sealed = node_seal_outer(self.keys.c2_public, inner).map_err(|e| e.to_string())?;
        let frame = Frame::data(self.channel_id, 0, sealed).map_err(|e| e.to_string())?;
        self.out.send(frame).await.map_err(|_| "uplink closed".to_owned())
    }
}
