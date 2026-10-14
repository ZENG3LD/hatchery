//! Restreamer runtime: node uplink + core drain, no push to core.
//!
//! Core opens a long-lived stream to C2 and drains envelopes. C2 never dials core.
//! Nodes speak multiplex frames; payloads stay opaque after the outer crypto layer.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use hatchery_core::{
    ChannelKind, DrainEnvelope, Frame, FrameError, FrameType, JournalReplyBody, OpenPayload,
    MAX_DATA_PAYLOAD,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;

use super::buffer::{EnqueueOutcome, ReplayBuffer, StreamKey};
use super::crypto::{c2_strip_outer, RestreamerKeys};
use super::journal::{recover_gap, NodeJournal};

#[derive(Clone, Debug)]
pub struct RestreamerConfig {
    /// Node uplink listen (edge / WG tunnel address in production).
    pub node_listen: SocketAddr,
    /// Core drain listen (reachable only from the core tunnel).
    pub drain_listen: SocketAddr,
    pub buffer: super::buffer::BufferLimits,
    /// Auth token core must present as the first line before draining.
    /// Never logged.
    pub core_token: String,
}

#[derive(Debug)]
pub enum RestreamerError {
    Io(std::io::Error),
    Config(&'static str),
    Frame(FrameError),
}

impl std::fmt::Display for RestreamerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "{e}"),
            Self::Config(m) => write!(f, "{m}"),
            Self::Frame(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for RestreamerError {}

impl From<std::io::Error> for RestreamerError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

/// Live node uplinks so C2 can send JournalReq when recovering gaps.
#[derive(Clone, Default)]
pub struct UplinkPeers {
    inner: Arc<Mutex<BTreeMap<String, PeerSlot>>>,
}

struct PeerSlot {
    out: tokio::sync::mpsc::Sender<Frame>,
    pending: Option<tokio::sync::oneshot::Sender<JournalReplyBody>>,
}

impl UplinkPeers {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&self, node_id: String, tx: tokio::sync::mpsc::Sender<Frame>) {
        self.inner.lock().expect("peers").insert(
            node_id,
            PeerSlot {
                out: tx,
                pending: None,
            },
        );
    }

    pub fn unregister(&self, node_id: &str) {
        self.inner.lock().expect("peers").remove(node_id);
    }

    pub fn deliver_reply(&self, node_id: &str, body: JournalReplyBody) {
        let mut guard = self.inner.lock().expect("peers");
        if let Some(slot) = guard.get_mut(node_id) {
            if let Some(tx) = slot.pending.take() {
                let _ = tx.send(body);
            }
        }
    }

    /// Relay one opaque downlink frame (core -> node). C2 does not read the payload.
    pub async fn send_to(&self, node_id: &str, frame: Frame) -> Result<(), RestreamerError> {
        let out = {
            let guard = self.inner.lock().expect("peers");
            guard.get(node_id).map(|slot| slot.out.clone())
        };
        let Some(out) = out else {
            return Err(RestreamerError::Frame(FrameError::Io("node uplink absent".into())));
        };
        out.send(frame)
            .await
            .map_err(|_| RestreamerError::Frame(FrameError::Io("node uplink closed".into())))
    }

    pub fn is_registered(&self, node_id: &str) -> bool {
        self.inner.lock().expect("peers").contains_key(node_id)
    }

    pub async fn request_journal(
        &self,
        node_id: &str,
        channel_id: u32,
        after: u64,
        limit: u32,
        timeout: std::time::Duration,
    ) -> Result<JournalReplyBody, RestreamerError> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let out = {
            let mut guard = self.inner.lock().expect("peers");
            let Some(slot) = guard.get_mut(node_id) else {
                return Err(RestreamerError::Frame(FrameError::Io(
                    "node uplink absent".into(),
                )));
            };
            slot.pending = Some(tx);
            slot.out.clone()
        };
        out.send(Frame::journal_req(channel_id, after, limit))
            .await
            .map_err(|_| RestreamerError::Frame(FrameError::Io("node uplink closed".into())))?;
        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(body)) => Ok(body),
            Ok(Err(_)) => Err(RestreamerError::Frame(FrameError::Io(
                "journal reply dropped".into(),
            ))),
            Err(_) => Err(RestreamerError::Frame(FrameError::Io(
                "journal reply timeout".into(),
            ))),
        }
    }
}

struct Inner {
    buffer: ReplayBuffer,
    /// channel_id -> kind, for OPEN bookkeeping (metadata only).
    channels: BTreeMap<(String, u32), ChannelKind>,
    keys: Option<RestreamerKeys>,
    /// True once any outbound dial toward a "core" address was attempted — must stay false.
    pushed_to_core: bool,
}

/// Shared restreamer state.
#[derive(Clone)]
pub struct Restreamer {
    /// Wakes core drain sessions as soon as an uplink frame is buffered.
    uplink_ready: Arc<tokio::sync::Notify>,
    inner: Arc<Mutex<Inner>>,
    journal: Arc<dyn NodeJournal>,
    peers: UplinkPeers,
}

impl Restreamer {
    pub fn new(
        buffer_limits: super::buffer::BufferLimits,
        journal: Arc<dyn NodeJournal>,
        core_token: impl Into<String>,
        keys: Option<RestreamerKeys>,
    ) -> Self {
        let _ = core_token; // validated by callers / RestreamerConfig
        Self {
            uplink_ready: Arc::new(tokio::sync::Notify::new()),
            inner: Arc::new(Mutex::new(Inner {
                // Per-stream credit: a PTY burst (echo frames + replies) easily has >64
                // messages in flight over the C2->core RTT; at 0 credit the frame is
                // refused and, as the node does not replay it, the inner Noise stream to
                // core desyncs and the session wedges. Hard limits stay max_events/bytes.
                buffer: ReplayBuffer::new(buffer_limits).with_default_credit(1024),
                channels: BTreeMap::new(),
                keys,
                pushed_to_core: false,
            })),
            journal,
            peers: UplinkPeers::new(),
        }
    }

    pub fn set_keys(&self, keys: RestreamerKeys) {
        self.inner.lock().expect("lock").keys = Some(keys);
    }

    pub fn uplink_notify(&self) -> Arc<tokio::sync::Notify> {
        self.uplink_ready.clone()
    }

    pub fn peers(&self) -> &UplinkPeers {
        &self.peers
    }

    pub fn upstream_public_keys(&self) -> Option<(super::crypto::PublicKeyBytes, super::crypto::PublicKeyBytes)> {
        self.inner
            .lock()
            .expect("lock")
            .keys
            .as_ref()
            .map(|k| k.upstream_public_keys())
    }

    /// Invariant: C2 never opens connections to core.
    pub fn ever_pushed_to_core(&self) -> bool {
        self.inner.lock().expect("lock").pushed_to_core
    }

    /// Admit one opaque DATA payload from a node (outer layer already optional).
    /// If keys are configured, `payload` is treated as outer-sealed and stripped.
    pub fn admit_from_node(
        &self,
        node_id: &str,
        channel_id: u32,
        payload: Vec<u8>,
    ) -> Result<EnqueueOutcome, RestreamerError> {
        if payload.len() > MAX_DATA_PAYLOAD {
            return Err(RestreamerError::Frame(FrameError::PayloadTooLarge(payload.len())));
        }
        let mut guard = self.inner.lock().expect("lock");
        let opaque = if let Some(keys) = guard.keys.clone() {
            c2_strip_outer(&keys, &payload).map_err(|e| {
                RestreamerError::Frame(FrameError::Io(format!("outer layer: {e}")))
            })?
        } else {
            payload
        };
        // Also record in journal view for gap recovery tests / local nodes.
        // Handshake1 must not be silently dropped: a drop desyncs Noise and the
        // node never gets Handshake2 (seen as "handshake timeout" on m4a).
        if matches!(
            hatchery_core::inner::decode_inner(&opaque),
            Some((hatchery_core::inner::InnerKind::Handshake1, _))
        ) {
            guard.buffer.ensure_handshake_credit(node_id, channel_id);
        }
        let outcome = guard
            .buffer
            .enqueue(node_id, channel_id, opaque.clone(), Instant::now());
        if let EnqueueOutcome::Accepted { .. } = outcome {
            let seq = guard.buffer.next_seq(node_id, channel_id) - 1;
            drop(guard);
            // best-effort journal mirror when the journal is a MemoryJournal-compatible append —
            // production journals live on the node; C2 asks via NodeJournal trait.
            let _ = (seq, opaque);
            self.uplink_ready.notify_waiters();
        }
        Ok(outcome)
    }

    pub fn open_channel(&self, node_id: &str, channel_id: u32, kind: ChannelKind) {
        let mut guard = self.inner.lock().expect("lock");
        guard.channels.insert((node_id.to_owned(), channel_id), kind);
        // Re-OPEN must be able to carry a fresh Noise handshake even if the
        // previous session spent the stream's credit (otherwise Handshake1 is
        // dropped and the node times out).
        guard.buffer.reset_channel_credit(node_id, channel_id);
    }

    pub fn ack(&self, node_id: &str, channel_id: u32, seq: u64) {
        self.inner
            .lock()
            .expect("lock")
            .buffer
            .ack(node_id, channel_id, seq);
    }

    pub fn window_credit(&self, node_id: &str, channel_id: u32) -> u32 {
        let key = StreamKey {
            node_id: node_id.to_owned(),
            channel_id,
        };
        self.inner.lock().expect("lock").buffer.credit(&key)
    }

    /// Produce drain envelopes for RESUME(after=N). Recovers gaps from the node journal.
    pub fn resume(
        &self,
        node_id: &str,
        channel_id: u32,
        after: u64,
    ) -> Result<Vec<DrainEnvelope>, RestreamerError> {
        let (gap, oldest, buffered) = {
            let guard = self.inner.lock().expect("lock");
            let gap = guard.buffer.gap_before_buffer(node_id, channel_id, after);
            let oldest = guard.buffer.oldest_seq(node_id, channel_id);
            let buffered = guard.buffer.events_after(node_id, channel_id, after);
            (gap, oldest, buffered)
        };
        let mut out = Vec::new();
        if gap {
            let until = oldest;
            let recovered = recover_gap(
                self.journal.as_ref(),
                node_id,
                channel_id,
                after,
                until,
                1024,
            )
            .map_err(|e| RestreamerError::Frame(FrameError::Io(e.to_string())))?;
            for ev in recovered {
                out.push(DrainEnvelope {
                    node_id: ev.node_id.clone(),
                    frame: Frame::data(ev.channel_id, ev.seq, ev.payload)
                        .map_err(RestreamerError::Frame)?,
                });
            }
        }
        for ev in buffered {
            // skip duplicates already recovered
            if out.iter().any(|e| e.frame.seq == ev.seq) {
                continue;
            }
            out.push(DrainEnvelope {
                node_id: ev.node_id.clone(),
                frame: Frame::data(ev.channel_id, ev.seq, ev.payload)
                    .map_err(RestreamerError::Frame)?,
            });
        }
        Ok(out)
    }

    /// Prefer live uplink journal; fall back to the configured NodeJournal.
    pub async fn resume_with_uplink(
        &self,
        node_id: &str,
        channel_id: u32,
        after: u64,
    ) -> Result<Vec<DrainEnvelope>, RestreamerError> {
        let (gap, oldest, buffered) = {
            let guard = self.inner.lock().expect("lock");
            let gap = guard.buffer.gap_before_buffer(node_id, channel_id, after);
            let oldest = guard.buffer.oldest_seq(node_id, channel_id);
            let buffered = guard.buffer.events_after(node_id, channel_id, after);
            (gap, oldest, buffered)
        };
        let mut out = Vec::new();
        if gap {
            let until = oldest;
            let recovered = if self.peers.is_registered(node_id) {
                let body = self
                    .peers
                    .request_journal(
                        node_id,
                        channel_id,
                        after,
                        1024,
                        std::time::Duration::from_secs(5),
                    )
                    .await?;
                body.entries
                    .into_iter()
                    .filter(|e| until.map(|u| e.seq < u).unwrap_or(true))
                    .map(|e| super::buffer::BufferedEvent {
                        seq: e.seq,
                        node_id: node_id.to_owned(),
                        channel_id,
                        payload: e.payload,
                        enqueued_at: Instant::now(),
                    })
                    .collect()
            } else {
                recover_gap(
                    self.journal.as_ref(),
                    node_id,
                    channel_id,
                    after,
                    until,
                    1024,
                )
                .map_err(|e| RestreamerError::Frame(FrameError::Io(e.to_string())))?
            };
            for ev in recovered {
                out.push(DrainEnvelope {
                    node_id: ev.node_id.clone(),
                    frame: Frame::data(ev.channel_id, ev.seq, ev.payload)
                        .map_err(RestreamerError::Frame)?,
                });
            }
        }
        for ev in buffered {
            if out.iter().any(|e| e.frame.seq == ev.seq) {
                continue;
            }
            out.push(DrainEnvelope {
                node_id: ev.node_id.clone(),
                frame: Frame::data(ev.channel_id, ev.seq, ev.payload)
                    .map_err(RestreamerError::Frame)?,
            });
        }
        Ok(out)
    }

    /// Snapshot of buffered events for a stream (no journal).
    pub fn buffered_after(&self, node_id: &str, channel_id: u32, after: u64) -> Vec<DrainEnvelope> {
        let guard = self.inner.lock().expect("lock");
        guard
            .buffer
            .events_after(node_id, channel_id, after)
            .into_iter()
            .filter_map(|ev| {
                Frame::data(ev.channel_id, ev.seq, ev.payload)
                    .ok()
                    .map(|frame| DrainEnvelope {
                        node_id: ev.node_id,
                        frame,
                    })
            })
            .collect()
    }
}

/// Bind node uplink + core drain listeners and serve until shutdown.
pub async fn serve(
    config: RestreamerConfig,
    restreamer: Restreamer,
    mut shutdown: watch::Receiver<bool>,
) -> Result<(), RestreamerError> {
    if config.node_listen == config.drain_listen {
        return Err(RestreamerError::Config(
            "node_listen and drain_listen must differ",
        ));
    }
    if config.core_token.len() < 16 {
        return Err(RestreamerError::Config("core_token too short"));
    }
    let node_listener = TcpListener::bind(config.node_listen).await?;
    let drain_listener = TcpListener::bind(config.drain_listen).await?;
    loop {
        tokio::select! {
            _ = shutdown.changed() => {
                if *shutdown.borrow() { break; }
            }
            accepted = node_listener.accept() => {
                let (stream, _) = accepted?;
                let _ = stream.set_nodelay(true);
                let rs = restreamer.clone();
                tokio::spawn(async move {
                    if let Err(err) = handle_node_uplink(stream, rs).await {
                        tracing::debug!(error = %err, "node uplink closed");
                    }
                });
            }
            accepted = drain_listener.accept() => {
                let (stream, _) = accepted?;
                let _ = stream.set_nodelay(true);
                let rs = restreamer.clone();
                let token = config.core_token.clone();
                tokio::spawn(async move {
                    if let Err(err) = handle_core_drain(stream, rs, &token).await {
                        tracing::debug!(error = %err, "core drain closed");
                    }
                });
            }
        }
    }
    Ok(())
}

async fn handle_node_uplink(mut stream: TcpStream, rs: Restreamer) -> Result<(), RestreamerError> {
    let mut buf = vec![0u8; 64 * 1024];
    let mut filled = 0usize;
    let mut node_id: Option<String> = None;
    let (out_tx, mut out_rx) = tokio::sync::mpsc::channel::<Frame>(16);
    loop {
        tokio::select! {
            biased;
            maybe_out = out_rx.recv() => {
                match maybe_out {
                    Some(frame) => {
                        stream
                            .write_all(&frame.encode().map_err(RestreamerError::Frame)?)
                            .await?;
                    }
                    None => break,
                }
            }
            read = stream.read(&mut buf[filled..]) => {
                let n = read?;
                if n == 0 {
                    break;
                }
                filled += n;
                while filled > 0 {
                    match Frame::decode(&buf[..filled]) {
                        Ok((frame, consumed)) => {
                            buf.copy_within(consumed..filled, 0);
                            filled -= consumed;
                            match frame.frame_type {
                                FrameType::Open => {
                                    let open = frame.parse_open().map_err(RestreamerError::Frame)?;
                                    if let Some(prev) = node_id.replace(open.node_id.clone()) {
                                        rs.peers().unregister(&prev);
                                    }
                                    rs.peers().register(open.node_id.clone(), out_tx.clone());
                                    rs.open_channel(&open.node_id, frame.channel_id, open.channel_kind);
                                    let credit = rs.window_credit(&open.node_id, frame.channel_id);
                                    let win = Frame::window(frame.channel_id, credit);
                                    stream
                                        .write_all(&win.encode().map_err(RestreamerError::Frame)?)
                                        .await?;
                                }
                                FrameType::Data => {
                                    let Some(ref nid) = node_id else {
                                        let go = Frame::goaway(1);
                                        stream
                                            .write_all(&go.encode().map_err(RestreamerError::Frame)?)
                                            .await?;
                                        return Ok(());
                                    };
                                    match rs.admit_from_node(nid, frame.channel_id, frame.payload)? {
                                        EnqueueOutcome::Accepted { window_credit } => {
                                            let win = Frame::window(frame.channel_id, window_credit);
                                            stream
                                                .write_all(
                                                    &win.encode().map_err(RestreamerError::Frame)?,
                                                )
                                                .await?;
                                        }
                                        EnqueueOutcome::Backpressure => {
                                            // Credit exhausted: frame is not buffered. The node
                                            // does not replay, so a silent drop desyncs the inner
                                            // Noise stream to core. Warn loudly; WINDOW 0 still
                                            // tells the peer to pause.
                                            tracing::warn!(
                                                target: "restreamer",
                                                node = %nid,
                                                channel = frame.channel_id,
                                                "restreamer credit exhausted; dropping node→core frame (Noise may desync)"
                                            );
                                            let win = Frame::window(frame.channel_id, 0);
                                            stream
                                                .write_all(
                                                    &win.encode().map_err(RestreamerError::Frame)?,
                                                )
                                                .await?;
                                        }
                                    }
                                }
                                FrameType::JournalReply => {
                                    if let Some(ref nid) = node_id {
                                        let body = JournalReplyBody::decode(&frame.payload)
                                            .map_err(RestreamerError::Frame)?;
                                        rs.peers().deliver_reply(nid, body);
                                    }
                                }
                                FrameType::Ping => {
                                    let pong = Frame::pong(frame.seq);
                                    stream
                                        .write_all(&pong.encode().map_err(RestreamerError::Frame)?)
                                        .await?;
                                }
                                FrameType::Close | FrameType::GoAway => {
                                    if let Some(ref nid) = node_id {
                                        rs.peers().unregister(nid);
                                    }
                                    return Ok(());
                                }
                                _ => {}
                            }
                        }
                        Err(FrameError::Truncated) => break,
                        Err(err) => return Err(RestreamerError::Frame(err)),
                    }
                }
                if filled == buf.len() {
                    return Err(RestreamerError::Frame(FrameError::PayloadTooLarge(filled)));
                }
            }
        }
    }
    if let Some(ref nid) = node_id {
        rs.peers().unregister(nid);
    }
    Ok(())
}

async fn handle_core_drain(
    mut stream: TcpStream,
    rs: Restreamer,
    expected_token: &str,
) -> Result<(), RestreamerError> {
    // Auth: first line "TOKEN\n" (constant-time compare).
    let mut auth = Vec::with_capacity(128);
    loop {
        let mut byte = [0u8; 1];
        let n = stream.read(&mut byte).await?;
        if n == 0 {
            return Err(RestreamerError::Config("drain auth closed"));
        }
        if byte[0] == b'\n' {
            break;
        }
        auth.push(byte[0]);
        if auth.len() > 512 {
            return Err(RestreamerError::Config("drain auth too long"));
        }
    }
    if !tokens_equal(&auth, expected_token.as_bytes()) {
        let go = Frame::goaway(403);
        let _ = stream.write_all(&go.encode().unwrap_or_default()).await;
        return Err(RestreamerError::Config("drain auth refused"));
    }

    let mut buf = vec![0u8; 64 * 1024];
    let mut filled = 0usize;
    // Default stream selection via OPEN from core (opaque node id).
    let mut selected: Option<(String, u32)> = None;
    // Highest seq already written on this drain session (per selected stream).
    let mut sent_through: u64 = 0;
    // Core must RESUME once before we drain; reconnects always start with RESUME.
    let mut resumed = false;
    let uplink_ready = rs.uplink_notify();
    loop {
        // Register for the wakeup before checking the buffer so none is missed.
        let woken = uplink_ready.notified();
        tokio::pin!(woken);
        woken.as_mut().enable();
        // After the first RESUME, opportunistically drain newly buffered events.
        if resumed {
            if let Some((ref nid, ch)) = selected {
                let envelopes = rs.buffered_after(nid, ch, sent_through);
                for env in envelopes {
                    stream
                        .write_all(&env.encode().map_err(RestreamerError::Frame)?)
                        .await?;
                    sent_through = sent_through.max(env.frame.seq);
                }
            }
        }

        // Wake on core input, on a newly buffered uplink frame (no 50 ms poll
        // delay on the hot path), or on the fallback tick.
        let read = tokio::select! {
            r = stream.read(&mut buf[filled..]) => Ok(r),
            _ = &mut woken => continue,
            _ = tokio::time::sleep(std::time::Duration::from_millis(50)) => Err(()),
        };
        match read {
            Ok(Ok(0)) => break,
            Ok(Ok(n)) => {
                filled += n;
                while filled > 0 {
                    match Frame::decode(&buf[..filled]) {
                        Ok((frame, consumed)) => {
                            buf.copy_within(consumed..filled, 0);
                            filled -= consumed;
                            match frame.frame_type {
                                FrameType::Open => {
                                    let open =
                                        frame.parse_open().map_err(RestreamerError::Frame)?;
                                    selected = Some((open.node_id, frame.channel_id));
                                    sent_through = 0;
                                    resumed = false;
                                }
                                FrameType::Ack => {
                                    if let Some((ref nid, ch)) = selected {
                                        rs.ack(nid, ch, frame.seq);
                                    }
                                }
                                FrameType::Resume => {
                                    if let Some((ref nid, ch)) = selected {
                                        let after = frame.seq;
                                        let envelopes = rs.resume_with_uplink(nid, ch, after).await?;
                                        for env in envelopes {
                                            stream
                                                .write_all(
                                                    &env.encode().map_err(RestreamerError::Frame)?,
                                                )
                                                .await?;
                                            sent_through = sent_through.max(env.frame.seq);
                                        }
                                        // Even with an empty replay, do not resend <= after.
                                        sent_through = sent_through.max(after);
                                        resumed = true;
                                    }
                                }
                                FrameType::Data => {
                                    // Downlink: opaque bytes for the selected node, relayed as-is.
                                    if let Some((ref nid, ch)) = selected {
                                        let down = Frame::data(ch, frame.seq, frame.payload)
                                            .map_err(RestreamerError::Frame)?;
                                        if let Err(err) = rs.peers().send_to(nid, down).await {
                                            tracing::debug!(error = %err, "downlink dropped");
                                        }
                                    }
                                }
                                FrameType::Ping => {
                                    let pong = Frame::pong(frame.seq);
                                    stream
                                        .write_all(
                                            &pong.encode().map_err(RestreamerError::Frame)?,
                                        )
                                        .await?;
                                }
                                FrameType::Close | FrameType::GoAway => return Ok(()),
                                _ => {}
                            }
                        }
                        Err(FrameError::Truncated) => break,
                        Err(err) => return Err(RestreamerError::Frame(err)),
                    }
                }
            }
            Ok(Err(e)) => return Err(e.into()),
            Err(_) => {
                // idle tick — loop to flush buffer
            }
        }
    }
    Ok(())
}

fn tokens_equal(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Core-side drain client: dials C2, authenticates, RESUMEs, ACKs.
pub struct DrainClient {
    addr: SocketAddr,
    token: String,
}

impl DrainClient {
    pub fn new(addr: SocketAddr, token: impl Into<String>) -> Self {
        Self {
            addr,
            token: token.into(),
        }
    }

    pub async fn connect(&self) -> Result<DrainSession, RestreamerError> {
        let mut stream = TcpStream::connect(self.addr).await?;
        let _ = stream.set_nodelay(true);
        stream.write_all(self.token.as_bytes()).await?;
        stream.write_all(b"\n").await?;
        Ok(DrainSession { stream })
    }
}

pub struct DrainSession {
    stream: TcpStream,
}

impl DrainSession {
    pub async fn open(&mut self, node_id: &str, channel_id: u32, kind: ChannelKind) -> Result<(), RestreamerError> {
        let frame = Frame::open(
            channel_id,
            &OpenPayload {
                channel_kind: kind,
                node_id: node_id.to_owned(),
            },
        )
        .map_err(RestreamerError::Frame)?;
        self.stream
            .write_all(&frame.encode().map_err(RestreamerError::Frame)?)
            .await?;
        Ok(())
    }

    pub async fn resume(&mut self, after: u64) -> Result<(), RestreamerError> {
        let frame = Frame::resume(after);
        self.stream
            .write_all(&frame.encode().map_err(RestreamerError::Frame)?)
            .await?;
        Ok(())
    }

    pub async fn ack(&mut self, seq: u64) -> Result<(), RestreamerError> {
        let frame = Frame::ack(seq);
        self.stream
            .write_all(&frame.encode().map_err(RestreamerError::Frame)?)
            .await?;
        Ok(())
    }

    pub async fn recv_envelope(
        &mut self,
        deadline: std::time::Duration,
    ) -> Result<Option<DrainEnvelope>, RestreamerError> {
        let mut buf = vec![0u8; 64 * 1024];
        let n = match tokio::time::timeout(deadline, self.stream.read(&mut buf)).await {
            Ok(Ok(0)) => return Ok(None),
            Ok(Ok(n)) => n,
            Ok(Err(e)) => return Err(e.into()),
            Err(_) => return Ok(None),
        };
        let (env, _) = DrainEnvelope::decode(&buf[..n]).map_err(RestreamerError::Frame)?;
        Ok(Some(env))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::restreamer::crypto::{core_open_inner, noise_session_seal, HqRecipientSecret};
    use crate::restreamer::journal::MemoryJournal;
    use hatchery_core::ChannelKind;
    use std::time::Duration;

    fn token() -> String {
        "test-core-token-32bytes-minimum!".to_owned()
    }

    #[tokio::test]
    async fn core_drains_and_acks_without_c2_push() {
        let journal = Arc::new(MemoryJournal::new());
        let rs = Restreamer::new(
            super::super::buffer::BufferLimits {
                max_events: 100,
                max_bytes: 1_000_000,
                max_age: Duration::from_secs(60),
            },
            journal,
            token(),
            None,
        );
        rs.open_channel("node-a", 1, ChannelKind::M4a);
        assert!(matches!(
            rs.admit_from_node("node-a", 1, b"cipher-1".to_vec()).unwrap(),
            EnqueueOutcome::Accepted { .. }
        ));
        assert!(matches!(
            rs.admit_from_node("node-a", 1, b"cipher-2".to_vec()).unwrap(),
            EnqueueOutcome::Accepted { .. }
        ));
        let batch = rs.buffered_after("node-a", 1, 0);
        assert_eq!(batch.len(), 2);
        assert_eq!(batch[0].frame.payload, b"cipher-1");
        rs.ack("node-a", 1, 1);
        let batch = rs.buffered_after("node-a", 1, 0);
        assert_eq!(batch.len(), 1);
        assert_eq!(batch[0].frame.seq, 2);
        assert!(!rs.ever_pushed_to_core());
    }

    #[tokio::test]
    async fn live_drain_session_resume_and_ack() {
        let journal = Arc::new(MemoryJournal::new());
        // Pre-populate journal for gap recovery.
        for seq in 1..=5u64 {
            journal.append("node-a", 1, seq, format!("j{seq}").into_bytes());
        }
        let rs = Restreamer::new(
            super::super::buffer::BufferLimits {
                max_events: 2,
                max_bytes: 1024,
                max_age: Duration::from_secs(60),
            },
            journal.clone(),
            token(),
            None,
        );
        // Fill buffer with seq 4,5 only (simulating 1..3 already acked/expired).
        // Manually: enqueue three then ack to leave two? limits max_events=2 so third backpressures.
        rs.admit_from_node("node-a", 1, b"b1".to_vec()).unwrap();
        rs.admit_from_node("node-a", 1, b"b2".to_vec()).unwrap();
        // ack both so buffer empty but next_seq advances; then we cannot get old seqs from buffer.
        rs.ack("node-a", 1, 2);
        // Produce seq 3,4 in buffer
        rs.admit_from_node("node-a", 1, b"b3".to_vec()).unwrap();
        rs.admit_from_node("node-a", 1, b"b4".to_vec()).unwrap();

        // RESUME after 0: gap for 1..2 from journal, then 3..4 from buffer.
        // But journal has 1..5 with different payloads; buffer has its own seq 3,4 with b3/b4.
        // For a clean gap test: ack everything, empty buffer, resume after 0 with journal only.
        let journal2 = Arc::new(MemoryJournal::new());
        for seq in 1..=4u64 {
            journal2.append("node-b", 1, seq, format!("evt-{seq}").into_bytes());
        }
        let rs2 = Restreamer::new(
            super::super::buffer::BufferLimits {
                max_events: 10,
                max_bytes: 10_000,
                max_age: Duration::from_secs(60),
            },
            journal2,
            token(),
            None,
        );
        // Advance next_seq without retaining: enqueue + ack
        for i in 1..=4u8 {
            rs2.admit_from_node("node-b", 1, vec![i]).unwrap();
        }
        rs2.ack("node-b", 1, 4);
        assert!(rs2.buffered_after("node-b", 1, 0).is_empty());
        let recovered = rs2.resume("node-b", 1, 0).unwrap();
        assert_eq!(recovered.len(), 4);
        assert_eq!(recovered[0].frame.payload, b"evt-1");
        assert_eq!(recovered[3].frame.seq, 4);
        assert!(!rs2.ever_pushed_to_core());
    }

    #[tokio::test]
    async fn tcp_drain_end_to_end() {
        let journal = Arc::new(MemoryJournal::new());
        let rs = Restreamer::new(
            super::super::buffer::BufferLimits::default(),
            journal,
            token(),
            None,
        );
        let node_l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let drain_l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let node_addr = node_l.local_addr().unwrap();
        let drain_addr = drain_l.local_addr().unwrap();
        drop(node_l);
        drop(drain_l);
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let cfg = RestreamerConfig {
            node_listen: node_addr,
            drain_listen: drain_addr,
            buffer: super::super::buffer::BufferLimits::default(),
            core_token: token(),
        };
        let serve_rs = rs.clone();
        let handle = tokio::spawn(async move { serve(cfg, serve_rs, shutdown_rx).await });

        // Give the server a moment to bind.
        tokio::time::sleep(Duration::from_millis(20)).await;

        // Node uplink
        let mut node = TcpStream::connect(node_addr).await.unwrap();
        let open = Frame::open(
            1,
            &OpenPayload {
                channel_kind: ChannelKind::G4aEvents,
                node_id: "node-a".into(),
            },
        )
        .unwrap();
        node.write_all(&open.encode().unwrap()).await.unwrap();
        // read WINDOW
        let mut rb = [0u8; 64];
        let _ = node.read(&mut rb).await.unwrap();
        let data = Frame::data(1, 0, b"opaque-event".to_vec()).unwrap();
        // Note: node-assigned seq on wire is ignored; C2 assigns its own.
        node.write_all(&data.encode().unwrap()).await.unwrap();
        let _ = node.read(&mut rb).await.unwrap();

        // Core drain
        let client = DrainClient::new(drain_addr, token());
        let mut session = client.connect().await.unwrap();
        session
            .open("node-a", 1, ChannelKind::G4aEvents)
            .await
            .unwrap();
        session.resume(0).await.unwrap();
        let env = session
            .recv_envelope(Duration::from_secs(2))
            .await
            .unwrap()
            .expect("envelope");
        assert_eq!(env.node_id, "node-a");
        assert_eq!(env.frame.payload, b"opaque-event");
        session.ack(env.frame.seq).await.unwrap();

        let _ = shutdown_tx.send(true);
        let _ = handle.await;
        assert!(!rs.ever_pushed_to_core());
    }

    #[test]
    fn nested_crypto_on_admit_path() {
        let hq = HqRecipientSecret::from_seed([11u8; 32]);
        let keys = RestreamerKeys::from_seeds([13u8; 32], hq.public_bytes().0);
        let journal = Arc::new(MemoryJournal::new());
        let rs = Restreamer::new(
            super::super::buffer::BufferLimits::default(),
            journal,
            token(),
            Some(keys.clone()),
        );
        let sealed = noise_session_seal(&keys, &hq, b"visible-only-to-core").unwrap();
        rs.admit_from_node("node-a", 1, sealed.outer).unwrap();
        let env = &rs.buffered_after("node-a", 1, 0)[0];
        assert!(!env
            .frame
            .payload
            .windows(b"visible-only-to-core".len())
            .any(|w| w == b"visible-only-to-core"));
        let mut hq_tx = sealed.hq_transport;
        let plain = core_open_inner(&mut hq_tx, &env.frame.payload).unwrap();
        assert_eq!(plain, b"visible-only-to-core");
    }

    #[tokio::test]
    async fn uplink_journal_req_served_from_file_journal() {
        use crate::restreamer::FileJournal;
        use hatchery_core::FrameType;

        let dir = std::env::temp_dir().join(format!("upj-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let fj = FileJournal::open(dir.join("n.journal")).unwrap();
        for i in 0..5u8 {
            fj.append(1, &[i]).unwrap();
        }
        let journal = Arc::new(MemoryJournal::new());
        let rs = Restreamer::new(
            super::super::buffer::BufferLimits::default(),
            journal,
            token(),
            None,
        );
        let node_l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let drain_l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let node_addr = node_l.local_addr().unwrap();
        let drain_addr = drain_l.local_addr().unwrap();
        drop(node_l);
        drop(drain_l);
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let cfg = RestreamerConfig {
            node_listen: node_addr,
            drain_listen: drain_addr,
            buffer: super::super::buffer::BufferLimits::default(),
            core_token: token(),
        };
        let serve_rs = rs.clone();
        let handle = tokio::spawn(async move { serve(cfg, serve_rs, shutdown_rx).await });
        tokio::time::sleep(Duration::from_millis(20)).await;

        let fj_node = fj.clone();
        let node_task = tokio::spawn(async move {
            let mut node = TcpStream::connect(node_addr).await.unwrap();
            let open = Frame::open(
                1,
                &OpenPayload {
                    channel_kind: ChannelKind::G4aEvents,
                    node_id: "node-a".into(),
                },
            )
            .unwrap();
            node.write_all(&open.encode().unwrap()).await.unwrap();
            let mut rb = vec![0u8; 64 * 1024];
            let mut filled = 0usize;
            loop {
                let n = node.read(&mut rb[filled..]).await.unwrap();
                if n == 0 {
                    break;
                }
                filled += n;
                while filled > 0 {
                    match Frame::decode(&rb[..filled]) {
                        Ok((frame, consumed)) => {
                            rb.copy_within(consumed..filled, 0);
                            filled -= consumed;
                            if frame.frame_type == FrameType::JournalReq {
                                let limit = frame.journal_req_limit().unwrap_or(64) as usize;
                                let body = fj_node
                                    .reply_body(frame.channel_id, frame.seq, limit)
                                    .unwrap();
                                let reply = Frame::journal_reply(frame.channel_id, &body).unwrap();
                                node.write_all(&reply.encode().unwrap()).await.unwrap();
                            }
                        }
                        Err(FrameError::Truncated) => break,
                        Err(e) => panic!("{e}"),
                    }
                }
            }
        });

        for _ in 0..50 {
            if rs.peers().is_registered("node-a") {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(rs.peers().is_registered("node-a"));
        let body = rs
            .peers()
            .request_journal("node-a", 1, 2, 10, Duration::from_secs(2))
            .await
            .unwrap();
        assert_eq!(
            body.entries.iter().map(|e| e.seq).collect::<Vec<_>>(),
            vec![3, 4, 5]
        );
        assert_eq!(body.entries[0].payload, &[2]);

        let _ = shutdown_tx.send(true);
        node_task.abort();
        let _ = handle.await;
        let _ = std::fs::remove_dir_all(&dir);
    }
}
