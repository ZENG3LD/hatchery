//! Core drain consumer (A2 / A3): core dials C2, drains one node stream, and answers on
//! the same stream. C2 never dials core and never sees plaintext.
//!
//! Per stream: Noise_NK msg1 arrives as a DATA envelope, core answers msg2 as downlink
//! DATA, then every envelope is Noise transport. Each plaintext goes to a
//! [`CoreHandler`] together with the node id taken from the stream (never from the
//! payload); the reply is encrypted back down. Every processed seq is ACKed, and a
//! reconnect RESUMEs after the last ACK.
//!
//! Restart safety: the last ACK per node/channel is persisted (see [`CursorStore`]) so a
//! restarted core does not replay the node's stream from zero. When a replay still
//! happens (no cursor, C2 reset), only the newest Handshake1 of the backlog is answered:
//! a Handshake1 is held until the backlog goes idle, a newer one replaces it, and
//! transport frames queued behind an unanswered Handshake1 belong to an older session
//! and are dropped. If no handshake lands for a while after resuming from a persisted
//! cursor (C2 restarted and its seqs restarted), the cursor is reset to zero.
//! A node does not notice a core restart (its C2 uplink stays up), so when core holds
//! no session it sends an undecryptable transport frame; the node drops its stale keys
//! and re-handshakes.

use hatchery_c2_client::{DrainClient, DrainSession};
use hatchery_core::inner::{decode_inner, encode_inner, InnerKind};
use hatchery_core::nested::{HqRecipientSecret, NoiseResponder, NoiseTransport};
use hatchery_core::ChannelKind;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;

/// Core logic for one decrypted inner message. Blocking work is fine: it runs on a
/// blocking thread. Return `None` for messages that need no reply.
pub type CoreHandler = Arc<dyn Fn(&str, ChannelKind, &[u8]) -> Option<Vec<u8>> + Send + Sync>;

pub struct NodeStream {
    pub node_id: String,
    pub channel_id: u32,
    pub kind: ChannelKind,
}

/// Env var naming the directory for persisted drain cursors (absolute path).
pub const DRAIN_STATE_DIR_ENV: &str = "HATCHERY_DRAIN_STATE_DIR";

/// How long after resuming from a persisted cursor we wait for a handshake before
/// assuming C2's sequence space restarted and replaying from zero.
const CURSOR_RESET_AFTER: Duration = Duration::from_secs(20);

/// Minimum gap between two rekey nudges on one stream.
const NUDGE_EVERY: Duration = Duration::from_secs(3);

/// File-backed last-ACK cursor for one node stream. Writes are atomic (tmp + rename).
#[derive(Clone, Debug)]
pub struct CursorStore {
    path: PathBuf,
}

impl CursorStore {
    pub fn new(dir: &Path, node_id: &str, channel_id: u32) -> Self {
        let safe: String = node_id
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
            .collect();
        Self { path: dir.join(format!("{safe}.{channel_id}.cursor")) }
    }

    /// Resolve the cursor directory: `HATCHERY_DRAIN_STATE_DIR`, else
    /// `<HATCHERY_SERVER_DATA_DIR>/drain-cursors`, else none (memory only).
    pub fn dir_from_env() -> Option<PathBuf> {
        let abs = |v: String| Some(PathBuf::from(v)).filter(|p| p.is_absolute());
        if let Some(d) = std::env::var(DRAIN_STATE_DIR_ENV).ok().filter(|s| !s.is_empty()).and_then(abs) {
            return Some(d);
        }
        std::env::var(crate::config::DATA_DIR_ENV)
            .ok()
            .filter(|s| !s.is_empty())
            .and_then(abs)
            .map(|d| d.join("drain-cursors"))
    }

    pub fn load(&self) -> u64 {
        std::fs::read_to_string(&self.path).ok().and_then(|s| s.trim().parse().ok()).unwrap_or(0)
    }

    pub fn save(&self, seq: u64) -> std::io::Result<()> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = self.path.with_extension("cursor.tmp");
        std::fs::write(&tmp, seq.to_string())?;
        std::fs::rename(&tmp, &self.path)
    }
}

pub struct DrainConsumer {
    client: DrainClient,
    recipient: HqRecipientSecret,
    stream: NodeStream,
    handler: CoreHandler,
    last_acked: u64,
    transport: Option<NoiseTransport>,
    /// Core-initiated messages (operator control). Sent once the node's handshake landed.
    outbound: Option<tokio::sync::mpsc::Receiver<Vec<u8>>>,
    cursor: Option<CursorStore>,
    cursor_write_failed: bool,
    /// Answer for the newest Handshake1 seen, held until the backlog is drained.
    pending: Option<(Vec<u8>, NoiseTransport)>,
    last_nudge: Option<tokio::time::Instant>,
}

impl DrainConsumer {
    pub fn new(client: DrainClient, recipient: HqRecipientSecret, stream: NodeStream, handler: CoreHandler) -> Self {
        Self {
            client,
            recipient,
            stream,
            handler,
            last_acked: 0,
            transport: None,
            outbound: None,
            cursor: None,
            cursor_write_failed: false,
            pending: None,
            last_nudge: None,
        }
    }

    /// Persist the last ACK under `dir` and start from the stored value.
    pub fn with_cursor_dir(mut self, dir: Option<PathBuf>) -> Self {
        if let Some(dir) = dir {
            let store = CursorStore::new(&dir, &self.stream.node_id, self.stream.channel_id);
            self.last_acked = store.load();
            if self.last_acked > 0 {
                tracing::info!(node = %self.stream.node_id, channel = self.stream.channel_id, cursor = self.last_acked, "drain cursor restored");
            }
            self.cursor = Some(store);
        }
        self
    }

    fn set_acked(&mut self, seq: u64) {
        self.last_acked = seq;
        if let Some(store) = &self.cursor {
            match store.save(seq) {
                Ok(()) => self.cursor_write_failed = false,
                Err(err) if !self.cursor_write_failed => {
                    self.cursor_write_failed = true;
                    tracing::warn!(node = %self.stream.node_id, error = %err, "drain cursor not persisted");
                }
                Err(_) => {}
            }
        }
    }

    /// Attach a queue of core-initiated plaintexts (see `node_controller::OperatorLink`).
    pub fn with_outbound(mut self, rx: tokio::sync::mpsc::Receiver<Vec<u8>>) -> Self {
        self.outbound = Some(rx);
        self
    }

    pub fn last_acked(&self) -> u64 {
        self.last_acked
    }

    /// Drain until shutdown, reconnecting with RESUME after the last ACK.
    pub async fn run(mut self, mut shutdown: watch::Receiver<bool>) {
        let mut backoff = Duration::from_millis(50);
        while !*shutdown.borrow() {
            match self.session(&mut shutdown).await {
                Ok(()) => backoff = Duration::from_millis(50),
                Err(err) => tracing::warn!(node = %self.stream.node_id, channel = self.stream.channel_id, error = %err, "drain session ended"),
            }
            if *shutdown.borrow() {
                break;
            }
            tokio::select! {
                _ = tokio::time::sleep(backoff) => {}
                _ = shutdown.changed() => {}
            }
            backoff = (backoff * 2).min(Duration::from_secs(5));
        }
    }

    async fn session(&mut self, shutdown: &mut watch::Receiver<bool>) -> Result<(), String> {
        // Prior Noise keys are invalid after a TCP reconnect; wait for a fresh msg1.
        self.transport = None;
        self.pending = None;
        let resumed_from = self.last_acked;
        let started = tokio::time::Instant::now();
        let mut s = self.client.connect().await.map_err(|e| e.to_string())?;
        s.open(&self.stream.node_id, self.stream.channel_id, self.stream.kind).await.map_err(|e| e.to_string())?;
        s.resume(self.last_acked).await.map_err(|e| e.to_string())?;
        loop {
            if *shutdown.borrow() {
                return Ok(());
            }
            let ready = self.transport.is_some();
            let env = tokio::select! {
                env = s.recv_envelope(Duration::from_millis(250)) => env.map_err(|e| e.to_string())?,
                Some(plain) = async {
                    match self.outbound.as_mut() {
                        Some(rx) if ready => rx.recv().await,
                        _ => std::future::pending().await,
                    }
                } => {
                    let ct = self.transport.as_mut().expect("transport").encrypt(&plain).map_err(|e| e.to_string())?;
                    s.send_data(self.stream.channel_id, encode_inner(InnerKind::Transport, &ct))
                        .await
                        .map_err(|e| e.to_string())?;
                    continue;
                }
                _ = shutdown.changed() => return Ok(()),
            };
            let Some(env) = env else {
                // Backlog drained: answer only the newest Handshake1.
                self.flush_pending(&mut s).await?;
                // Idle without a session: the node may still hold keys from before our
                // restart and will never re-handshake on its own. Nudge it.
                if self.transport.is_none() && started.elapsed() >= Duration::from_secs(1) {
                    self.nudge(&mut s).await?;
                }
                if resumed_from > 0
                    && self.transport.is_none()
                    && self.last_acked == resumed_from
                    && started.elapsed() >= CURSOR_RESET_AFTER
                {
                    tracing::warn!(node = %self.stream.node_id, channel = self.stream.channel_id, cursor = resumed_from, "no handshake after resume; resetting drain cursor to 0");
                    self.set_acked(0);
                    return Err("drain cursor reset".into());
                }
                continue;
            };
            if env.node_id != self.stream.node_id {
                continue;
            }
            let seq = env.frame.seq;
            if seq != 0 && seq <= self.last_acked {
                continue;
            }
            self.handle(&mut s, &env.frame.payload).await?;
            if seq != 0 {
                s.ack(seq).await.map_err(|e| e.to_string())?;
                self.set_acked(seq);
            }
        }
    }

    async fn handle(&mut self, s: &mut DrainSession, inner: &[u8]) -> Result<(), String> {
        match decode_inner(inner) {
            Some((InnerKind::Handshake1, msg1)) => {
                let responder = NoiseResponder::new(&self.recipient).map_err(|e| e.to_string())?;
                match responder.complete(msg1) {
                    Ok((msg2, transport)) => {
                        // Hold the answer: a newer Handshake1 later in the backlog wins.
                        if self.pending.is_some() {
                            tracing::debug!(node = %self.stream.node_id, "stale handshake superseded");
                        }
                        self.transport = None;
                        self.pending = Some((msg2, transport));
                    }
                    // Wrong recipient key or garbage: drop it, keep draining.
                    Err(err) => tracing::debug!(error = %err, "handshake refused"),
                }
            }
            Some((InnerKind::Transport, ct)) => {
                if self.pending.is_some() {
                    // The node cannot send transport before it got our msg2, so this
                    // frame belongs to an older session.
                    return Ok(());
                }
                let Some(transport) = self.transport.as_mut() else {
                    // Live traffic from a session we hold no keys for (core restarted).
                    return self.nudge(s).await;
                };
                let Ok(plain) = transport.decrypt(ct) else {
                    // Desync (often after C2 dropped a frame under credit pressure).
                    // Forget the session so the next Handshake1 can recover.
                    tracing::warn!(node = %self.stream.node_id, channel = self.stream.channel_id, "inner decrypt failed; resetting Noise session");
                    self.transport = None;
                    return Ok(());
                };
                let handler = Arc::clone(&self.handler);
                let node = self.stream.node_id.clone();
                let kind = self.stream.kind;
                let reply = tokio::task::spawn_blocking(move || handler(&node, kind, &plain))
                    .await
                    .map_err(|e| e.to_string())?;
                if let Some(reply) = reply {
                    let ct = self.transport.as_mut().expect("transport").encrypt(&reply).map_err(|e| e.to_string())?;
                    s.send_data(self.stream.channel_id, encode_inner(InnerKind::Transport, &ct))
                        .await
                        .map_err(|e| e.to_string())?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Send an undecryptable transport frame: the node's session fails to decrypt it,
    /// drops its stale keys and reconnects with a fresh Handshake1. Rate-limited.
    async fn nudge(&mut self, s: &mut DrainSession) -> Result<(), String> {
        if self.pending.is_some() || self.last_nudge.is_some_and(|t| t.elapsed() < NUDGE_EVERY) {
            return Ok(());
        }
        self.last_nudge = Some(tokio::time::Instant::now());
        tracing::info!(node = %self.stream.node_id, channel = self.stream.channel_id, "no Noise session; asking node to re-handshake");
        s.send_data(self.stream.channel_id, encode_inner(InnerKind::Transport, &[0u8; 48]))
            .await
            .map_err(|e| e.to_string())
    }

    async fn flush_pending(&mut self, s: &mut DrainSession) -> Result<(), String> {
        if let Some((msg2, transport)) = self.pending.take() {
            self.transport = Some(transport);
            s.send_data(self.stream.channel_id, encode_inner(InnerKind::Handshake2, &msg2))
                .await
                .map_err(|e| e.to_string())?;
            tracing::info!(node = %self.stream.node_id, channel = self.stream.channel_id, "drain handshake answered");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_roundtrip_and_sanitized_name() {
        let dir = tempfile::tempdir().unwrap();
        let store = CursorStore::new(dir.path(), "../evil/node", 7);
        assert_eq!(store.load(), 0);
        store.save(42).unwrap();
        assert_eq!(store.load(), 42);
        assert_eq!(CursorStore::new(dir.path(), "../evil/node", 7).load(), 42);
        assert_eq!(CursorStore::new(dir.path(), "../evil/node", 8).load(), 0);
        assert!(store.path.starts_with(dir.path()));
    }

    #[test]
    fn cursor_store_creates_dir() {
        let dir = tempfile::tempdir().unwrap();
        let store = CursorStore::new(&dir.path().join("a/b"), "n1", 1);
        store.save(5).unwrap();
        assert_eq!(store.load(), 5);
    }
}
