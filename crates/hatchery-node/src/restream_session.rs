//! Node end of a live restreamer session (A2 / A3).
//!
//! The node dials C2's uplink, OPENs a channel, then runs the inner Noise_NK
//! handshake toward the relayed recipient key as ordinary DATA frames:
//! msg1 goes up through C2 into the core drain, msg2 comes back down the same
//! stream. After that every request and reply is Noise transport ciphertext,
//! wrapped in C2's outer layer on the way up. C2 only ever sees opaque bytes,
//! and the node never learns what sits behind C2.
//!
//! One TCP carries every channel (C2 routes downlink by node id). A single
//! FIFO send queue would let browser frames or a stuck mail channel wedge PTY
//! control (head-of-line blocking). Outbound traffic is therefore queued
//! per-channel with priority scheduling, browser frames are coalesced under
//! pressure, and a write timeout tears the socket down so the bridge reconnects.

use crate::uplink_journal::UplinkJournal;
use hatchery_core::inner::{decode_inner, encode_inner, InnerKind};
use hatchery_core::nested::{node_seal_outer, NoiseInitiator, NoiseTransport, PublicKeyBytes};
use hatchery_core::{grow_read_buf, ChannelKind, Frame, FrameError, FrameType, OpenPayload};
use serde_json::{json, Value};
use std::collections::{BTreeMap, VecDeque};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, Notify};
use tokio::task::JoinHandle;

/// Keys C2 hands a node at enrollment: its own outer key and the upstream recipient key.
#[derive(Clone, Copy, Debug)]
pub struct UpstreamKeys {
    pub c2_public: PublicKeyBytes,
    pub recipient_public: PublicKeyBytes,
}

/// Outbound priority on the shared uplink. Lower ordinal = sent first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SendPrio {
    /// OPEN, Noise handshake, journal replies, g4a request/response (incl. PTY input path).
    Control = 0,
    /// g4a terminal / event frames.
    Pty = 1,
    /// m4a mail/task calls.
    Mail = 2,
    /// dig2browser frames — droppable / coalesced under pressure.
    Browser = 3,
}

const WRITE_TIMEOUT: Duration = Duration::from_secs(5);
const CHAN_CAP_CONTROL: usize = 64;
const CHAN_CAP_PTY: usize = 128;
const CHAN_CAP_MAIL: usize = 16;
const CHAN_CAP_BROWSER: usize = 8;

struct Queued {
    prio: SendPrio,
    frame: Frame,
}

struct ChanQ {
    q: VecDeque<Queued>,
    /// Remaining WINDOW credit for DATA frames; None until the first WINDOW.
    credit: Option<u32>,
}

impl ChanQ {
    fn new() -> Self {
        Self { q: VecDeque::new(), credit: None }
    }

    fn cap_for(prio: SendPrio) -> usize {
        match prio {
            SendPrio::Control => CHAN_CAP_CONTROL,
            SendPrio::Pty => CHAN_CAP_PTY,
            SendPrio::Mail => CHAN_CAP_MAIL,
            SendPrio::Browser => CHAN_CAP_BROWSER,
        }
    }
}

struct OutState {
    chans: BTreeMap<u32, ChanQ>,
}

/// One TCP uplink to C2 carrying any number of channels.
pub struct NodeUplink {
    enqueue: mpsc::UnboundedSender<(u32, SendPrio, Frame)>,
    routes: Arc<Mutex<BTreeMap<u32, mpsc::UnboundedSender<Vec<u8>>>>>,
    /// Last WINDOW credit per channel (updated by the reader).
    credits: Arc<Mutex<BTreeMap<u32, u32>>>,
    credit_pulse: Arc<Notify>,
    journal: Option<Arc<UplinkJournal>>,
    alive: Arc<AtomicBool>,
    dead: Arc<Notify>,
    tasks: Vec<JoinHandle<()>>,
}

impl Drop for NodeUplink {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::SeqCst);
        for t in &self.tasks {
            t.abort();
        }
    }
}

impl NodeUplink {
    pub async fn connect(uplink: SocketAddr, journal: Option<Arc<UplinkJournal>>) -> Result<Arc<Self>, String> {
        let stream = TcpStream::connect(uplink).await.map_err(|e| e.to_string())?;
        let _ = stream.set_nodelay(true);
        let (mut rd, mut wr) = stream.into_split();
        let (enq_tx, mut enq_rx) = mpsc::unbounded_channel::<(u32, SendPrio, Frame)>();
        let routes: Arc<Mutex<BTreeMap<u32, mpsc::UnboundedSender<Vec<u8>>>>> = Arc::default();
        let credits: Arc<Mutex<BTreeMap<u32, u32>>> = Arc::default();
        let credit_pulse = Arc::new(Notify::new());
        let alive = Arc::new(AtomicBool::new(true));
        let dead = Arc::new(Notify::new());
        let wake = Arc::new(Notify::new());

        let out_state = Arc::new(Mutex::new(OutState { chans: BTreeMap::new() }));

        // Ingress into per-channel queues (never blocks the producer on TCP).
        let state_in = Arc::clone(&out_state);
        let wake_in = Arc::clone(&wake);
        let alive_in = Arc::clone(&alive);
        let ingress = tokio::spawn(async move {
            while let Some((ch, prio, frame)) = enq_rx.recv().await {
                if !alive_in.load(Ordering::SeqCst) {
                    break;
                }
                let mut st = state_in.lock().expect("out");
                let q = st.chans.entry(ch).or_insert_with(ChanQ::new);
                let cap = ChanQ::cap_for(prio);
                if prio == SendPrio::Browser {
                    // Coalesce: keep only the newest browser frame under pressure.
                    q.q.retain(|x| x.prio != SendPrio::Browser);
                    if q.q.len() >= cap {
                        // Still full of higher-prio work: drop this browser frame.
                        continue;
                    }
                } else {
                    while q.q.len() >= cap {
                        // Drop lowest-priority (browser first, then mail) before refusing control/PTY.
                        if let Some(i) = q
                            .q
                            .iter()
                            .enumerate()
                            .filter(|(_, x)| x.prio >= SendPrio::Mail)
                            .max_by_key(|(_, x)| x.prio)
                            .map(|(i, _)| i)
                        {
                            q.q.remove(i);
                        } else if prio <= SendPrio::Pty {
                            // Control/PTY: drop oldest same-or-lower in this channel.
                            q.q.pop_front();
                        } else {
                            break;
                        }
                    }
                    if q.q.len() >= cap {
                        continue;
                    }
                }
                q.q.push_back(Queued { prio, frame });
                drop(st);
                wake_in.notify_one();
            }
        });

        // Fair/priority writer: always pick the globally highest-priority waiting frame
        // whose channel has credit (DATA) or needs no credit (OPEN/control non-DATA).
        let state_wr = Arc::clone(&out_state);
        let wake_wr = Arc::clone(&wake);
        let alive_wr = Arc::clone(&alive);
        let dead_wr = Arc::clone(&dead);
        let routes_wr = Arc::clone(&routes);
        let writer = tokio::spawn(async move {
            loop {
                if !alive_wr.load(Ordering::SeqCst) {
                    break;
                }
                let next = {
                    let mut st = state_wr.lock().expect("out");
                    pick_next(&mut st)
                };
                let Some((ch, is_data, frame)) = next else {
                    wake_wr.notified().await;
                    continue;
                };
                let Ok(bytes) = frame.encode() else {
                    kill(&alive_wr, &dead_wr, &routes_wr);
                    break;
                };
                match tokio::time::timeout(WRITE_TIMEOUT, wr.write_all(&bytes)).await {
                    Ok(Ok(())) => {
                        if is_data {
                            let mut st = state_wr.lock().expect("out");
                            if let Some(q) = st.chans.get_mut(&ch) {
                                if let Some(c) = q.credit.as_mut() {
                                    *c = c.saturating_sub(1);
                                }
                            }
                        }
                    }
                    Ok(Err(_)) | Err(_) => {
                        tracing::warn!(%ch, "uplink write timed out or failed; reconnecting");
                        kill(&alive_wr, &dead_wr, &routes_wr);
                        break;
                    }
                }
            }
        });

        let enq_reader = enq_tx.clone();
        let reader_journal = journal.clone();
        let reader_routes = Arc::clone(&routes);
        let alive_rd = Arc::clone(&alive);
        let dead_rd = Arc::clone(&dead);
        let state_rd = Arc::clone(&out_state);
        let wake_rd = Arc::clone(&wake);
        let credits_rd = Arc::clone(&credits);
        let credit_pulse_rd = Arc::clone(&credit_pulse);
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
                                    let tx = reader_routes.lock().expect("routes").get(&frame.channel_id).cloned();
                                    if let Some(tx) = tx {
                                        let _ = tx.send(frame.payload);
                                    }
                                }
                                FrameType::Window => {
                                    let credit = frame.seq.min(u64::from(u32::MAX)) as u32;
                                    {
                                        let mut st = state_rd.lock().expect("out");
                                        st.chans.entry(frame.channel_id).or_insert_with(ChanQ::new).credit = Some(credit);
                                    }
                                    credits_rd.lock().expect("credits").insert(frame.channel_id, credit);
                                    credit_pulse_rd.notify_waiters();
                                    wake_rd.notify_one();
                                }
                                FrameType::JournalReq => {
                                    let Some(j) = reader_journal.as_ref() else { continue };
                                    let limit = frame.journal_req_limit().unwrap_or(64) as usize;
                                    if let Ok(body) = j.journal().reply_body(frame.channel_id, frame.seq, limit) {
                                        if let Ok(reply) = Frame::journal_reply(frame.channel_id, &body) {
                                            let _ = enq_reader.send((frame.channel_id, SendPrio::Control, reply));
                                        }
                                    }
                                }
                                FrameType::GoAway | FrameType::Close => {
                                    kill(&alive_rd, &dead_rd, &reader_routes);
                                    return;
                                }
                                _ => {}
                            }
                        }
                        Err(FrameError::Truncated) => break,
                        Err(_) => {
                            kill(&alive_rd, &dead_rd, &reader_routes);
                            return;
                        }
                    }
                }
                if filled == buf.len() {
                    // Same grow path as C2: do not tear the bridge when a
                    // single frame exceeds the initial 128 KiB read buf.
                    if grow_read_buf(&mut buf, filled).is_err() {
                        break;
                    }
                }
            }
            kill(&alive_rd, &dead_rd, &reader_routes);
        });

        Ok(Arc::new(Self {
            enqueue: enq_tx,
            routes,
            credits,
            credit_pulse,
            journal,
            alive,
            dead,
            tasks: vec![ingress, writer, reader],
        }))
    }

    pub fn is_alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }

    /// Wait until the uplink is marked dead (write timeout / peer close).
    pub async fn closed(&self) {
        if !self.is_alive() {
            return;
        }
        self.dead.notified().await;
    }

    fn push(&self, channel_id: u32, prio: SendPrio, frame: Frame) -> Result<(), String> {
        if !self.is_alive() {
            return Err("uplink closed".into());
        }
        self.enqueue
            .send((channel_id, prio, frame))
            .map_err(|_| "uplink closed".to_owned())
    }

    /// OPEN `channel_id`, wait for WINDOW credit, then complete Noise_NK.
    pub async fn open(
        self: &Arc<Self>,
        node_id: &str,
        channel_id: u32,
        kind: ChannelKind,
        keys: UpstreamKeys,
        handshake_timeout: Duration,
    ) -> Result<NodeSession, String> {
        let (down_tx, mut down_rx) = mpsc::unbounded_channel::<Vec<u8>>();
        self.routes.lock().expect("routes").insert(channel_id, down_tx);
        self.credits.lock().expect("credits").remove(&channel_id);
        let open = Frame::open(channel_id, &OpenPayload { channel_kind: kind, node_id: node_id.to_owned() })
            .map_err(|e| e.to_string())?;
        self.push(channel_id, SendPrio::Control, open)?;

        // Wait until C2 grants WINDOW credit after OPEN (avoids sending Handshake1
        // into a stream that still has credit 0 from a previous session).
        tokio::time::timeout(handshake_timeout, async {
            loop {
                if !self.is_alive() {
                    return Err("uplink closed".to_owned());
                }
                if self.credits.lock().expect("credits").get(&channel_id).copied().unwrap_or(0) > 0 {
                    return Ok(());
                }
                tokio::select! {
                    _ = self.credit_pulse.notified() => {}
                    _ = self.closed() => return Err("uplink closed".to_owned()),
                }
            }
        })
        .await
        .map_err(|_| "window credit timeout after OPEN".to_owned())??;

        let mut initiator = NoiseInitiator::new(keys.recipient_public).map_err(|e| e.to_string())?;
        let msg1 = initiator.write_message1().map_err(|e| e.to_string())?;
        self.send_inner(channel_id, keys, SendPrio::Control, &encode_inner(InnerKind::Handshake1, &msg1))?;

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
        Ok(NodeSession {
            channel_id,
            keys,
            default_prio: match kind {
                ChannelKind::M4a => SendPrio::Mail,
                ChannelKind::G4aControl | ChannelKind::G4aPty | ChannelKind::G4aEvents => SendPrio::Pty,
                ChannelKind::Vault | ChannelKind::Observation => SendPrio::Mail,
            },
            down: down_rx,
            transport,
            journal: self.journal.clone(),
            uplink: Arc::clone(self),
        })
    }

    fn send_inner(&self, channel_id: u32, keys: UpstreamKeys, prio: SendPrio, inner: &[u8]) -> Result<(), String> {
        if let Some(j) = &self.journal {
            j.append(channel_id, inner)?;
        }
        let sealed = node_seal_outer(keys.c2_public, inner).map_err(|e| e.to_string())?;
        let frame = Frame::data(channel_id, 0, sealed).map_err(|e| e.to_string())?;
        self.push(channel_id, prio, frame)
    }
}

fn kill(alive: &AtomicBool, dead: &Notify, routes: &Mutex<BTreeMap<u32, mpsc::UnboundedSender<Vec<u8>>>>) {
    if alive.swap(false, Ordering::SeqCst) {
        routes.lock().expect("routes").clear();
        dead.notify_waiters();
    }
}

/// Pick the highest-priority queued frame that may be sent now.
fn pick_next(st: &mut OutState) -> Option<(u32, bool, Frame)> {
    // Flatten candidates: (prio, channel, index)
    let mut best: Option<(SendPrio, u32, usize)> = None;
    for (&ch, cq) in &st.chans {
        for (i, item) in cq.q.iter().enumerate() {
            let needs_credit = matches!(item.frame.frame_type, FrameType::Data);
            if needs_credit {
                match cq.credit {
                    Some(0) => continue,
                    None => {
                        // No WINDOW yet: allow Control DATA (handshake) so OPEN→Handshake1 works;
                        // hold back bulk traffic.
                        if item.prio > SendPrio::Control {
                            continue;
                        }
                    }
                    Some(_) => {}
                }
            }
            let cand = (item.prio, ch, i);
            if best.is_none_or(|b| cand.0 < b.0 || (cand.0 == b.0 && cand.1 < b.1)) {
                best = Some(cand);
            }
        }
    }
    let (prio, ch, idx) = best?;
    let cq = st.chans.get_mut(&ch)?;
    let item = cq.q.remove(idx)?;
    debug_assert_eq!(item.prio, prio);
    let is_data = matches!(item.frame.frame_type, FrameType::Data);
    Some((ch, is_data, item.frame))
}

pub struct NodeSession {
    channel_id: u32,
    keys: UpstreamKeys,
    default_prio: SendPrio,
    down: mpsc::UnboundedReceiver<Vec<u8>>,
    transport: NoiseTransport,
    journal: Option<Arc<UplinkJournal>>,
    uplink: Arc<NodeUplink>,
}

impl NodeSession {
    pub async fn connect(
        uplink: SocketAddr,
        node_id: &str,
        channel_id: u32,
        kind: ChannelKind,
        keys: UpstreamKeys,
        journal: Option<Arc<UplinkJournal>>,
        handshake_timeout: Duration,
    ) -> Result<Self, String> {
        let link = NodeUplink::connect(uplink, journal).await?;
        link.open(node_id, channel_id, kind, keys, handshake_timeout).await
    }

    pub fn uplink(&self) -> Arc<NodeUplink> {
        Arc::clone(&self.uplink)
    }

    pub async fn send(&mut self, plaintext: &[u8]) -> Result<(), String> {
        self.send_prio(plaintext, self.default_prio).await
    }

    pub async fn send_prio(&mut self, plaintext: &[u8], prio: SendPrio) -> Result<(), String> {
        if !self.uplink.is_alive() {
            return Err("uplink closed".into());
        }
        let ct = self.transport.encrypt(plaintext).map_err(|e| e.to_string())?;
        let inner = encode_inner(InnerKind::Transport, &ct);
        self.uplink.send_inner(self.channel_id, self.keys, prio, &inner)
    }

    pub async fn recv(&mut self, timeout: Duration) -> Result<Vec<u8>, String> {
        tokio::time::timeout(timeout, async {
            loop {
                tokio::select! {
                    bytes = self.down.recv() => {
                        let Some(bytes) = bytes else { return Err("uplink closed".to_owned()) };
                        if let Some((InnerKind::Transport, ct)) = decode_inner(&bytes) {
                            return self.transport.decrypt(ct).map_err(|e| e.to_string());
                        }
                    }
                    _ = self.uplink.closed() => return Err("uplink closed".to_owned()),
                }
            }
        })
        .await
        .map_err(|_| "reply timeout".to_owned())?
    }

    pub async fn call(&mut self, op: &str, payload: &Value, timeout: Duration) -> Result<Value, String> {
        let req = serde_json::to_vec(&json!({ "op": op, "payload": payload })).map_err(|e| e.to_string())?;
        self.send_prio(&req, SendPrio::Mail).await?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pick_prefers_control_over_browser() {
        let mut st = OutState { chans: BTreeMap::new() };
        let q = st.chans.entry(1).or_insert_with(ChanQ::new);
        q.credit = Some(10);
        q.q.push_back(Queued {
            prio: SendPrio::Browser,
            frame: Frame::data(1, 0, vec![1]).unwrap(),
        });
        q.q.push_back(Queued {
            prio: SendPrio::Control,
            frame: Frame::data(1, 0, vec![2]).unwrap(),
        });
        q.q.push_back(Queued {
            prio: SendPrio::Pty,
            frame: Frame::data(1, 0, vec![3]).unwrap(),
        });
        let (_, _, f) = pick_next(&mut st).unwrap();
        assert_eq!(f.payload, vec![2]);
        let (_, _, f) = pick_next(&mut st).unwrap();
        assert_eq!(f.payload, vec![3]);
        let (_, _, f) = pick_next(&mut st).unwrap();
        assert_eq!(f.payload, vec![1]);
    }

    #[test]
    fn pick_holds_bulk_until_window() {
        let mut st = OutState { chans: BTreeMap::new() };
        let q = st.chans.entry(2).or_insert_with(ChanQ::new);
        q.credit = None;
        q.q.push_back(Queued {
            prio: SendPrio::Mail,
            frame: Frame::data(2, 0, vec![9]).unwrap(),
        });
        q.q.push_back(Queued {
            prio: SendPrio::Control,
            frame: Frame::data(2, 0, vec![1]).unwrap(),
        });
        let (_, _, f) = pick_next(&mut st).unwrap();
        assert_eq!(f.payload, vec![1]);
        assert!(pick_next(&mut st).is_none());
        st.chans.get_mut(&2).unwrap().credit = Some(4);
        let (_, _, f) = pick_next(&mut st).unwrap();
        assert_eq!(f.payload, vec![9]);
    }
}
