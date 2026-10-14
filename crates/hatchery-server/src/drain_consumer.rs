//! Core drain consumer (A2 / A3): core dials C2, drains one node stream, and answers on
//! the same stream. C2 never dials core and never sees plaintext.
//!
//! Per stream: Noise_NK msg1 arrives as a DATA envelope, core answers msg2 as downlink
//! DATA, then every envelope is Noise transport. Each plaintext goes to a
//! [`CoreHandler`] together with the node id taken from the stream (never from the
//! payload); the reply is encrypted back down. Every processed seq is ACKed, and a
//! reconnect RESUMEs after the last ACK.

use hatchery_c2_client::{DrainClient, DrainSession};
use hatchery_core::inner::{decode_inner, encode_inner, InnerKind};
use hatchery_core::nested::{HqRecipientSecret, NoiseResponder, NoiseTransport};
use hatchery_core::ChannelKind;
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

pub struct DrainConsumer {
    client: DrainClient,
    recipient: HqRecipientSecret,
    stream: NodeStream,
    handler: CoreHandler,
    last_acked: u64,
    transport: Option<NoiseTransport>,
    /// Core-initiated messages (operator control). Sent once the node's handshake landed.
    outbound: Option<tokio::sync::mpsc::Receiver<Vec<u8>>>,
}

impl DrainConsumer {
    pub fn new(client: DrainClient, recipient: HqRecipientSecret, stream: NodeStream, handler: CoreHandler) -> Self {
        Self { client, recipient, stream, handler, last_acked: 0, transport: None, outbound: None }
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
                Err(err) => tracing::debug!(error = %err, "drain session ended"),
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
            let Some(env) = env else { continue };
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
                self.last_acked = seq;
            }
        }
    }

    async fn handle(&mut self, s: &mut DrainSession, inner: &[u8]) -> Result<(), String> {
        match decode_inner(inner) {
            Some((InnerKind::Handshake1, msg1)) => {
                let responder = NoiseResponder::new(&self.recipient).map_err(|e| e.to_string())?;
                match responder.complete(msg1) {
                    Ok((msg2, transport)) => {
                        self.transport = Some(transport);
                        s.send_data(self.stream.channel_id, encode_inner(InnerKind::Handshake2, &msg2))
                            .await
                            .map_err(|e| e.to_string())?;
                    }
                    // Wrong recipient key or garbage: drop it, keep draining.
                    Err(err) => tracing::debug!(error = %err, "handshake refused"),
                }
            }
            Some((InnerKind::Transport, ct)) => {
                let Some(transport) = self.transport.as_mut() else { return Ok(()) };
                let Ok(plain) = transport.decrypt(ct) else {
                    tracing::debug!("inner decrypt failed");
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
}
