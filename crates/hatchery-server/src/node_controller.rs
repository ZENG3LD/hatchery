//! Core-side node controller (A2): g4a parsing lives here, not on C2.
//!
//! C2's restreamer forwards opaque multiplex DATA. Core drains the stream,
//! strips the inner Noise layer (caller-supplied transport), then this module
//! decodes g4a frames (`RoutedNodeEvent` / `RoutedNodeResponse` / `C2ServerFrame`).

use hatchery_core::{
    BrowserClientOp, BrowserOpReply, BrowserStreamEvent, ChannelKind, DrainEnvelope,
    RoutedNodeEvent, RoutedNodeRequest, RoutedNodeResponse, C2ServerFrame,
};
use serde_json::Value;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum NodeControllerError {
    #[error("payload is not valid JSON")]
    InvalidJson,
    #[error("payload is not a known g4a frame")]
    UnknownFrame,
    #[error("channel kind does not carry g4a")]
    NotG4a,
}

/// Decoded g4a traffic from one drain envelope (after outer/inner crypto).
#[derive(Debug)]
pub enum G4aMessage {
    ServerFrame(C2ServerFrame),
    Event(RoutedNodeEvent),
    Response(RoutedNodeResponse),
    Request(RoutedNodeRequest),
    /// Opaque JSON that is not yet a typed frame (forward-compatible).
    Json(Value),
}

/// Returns true when the channel kind is one of the g4a.* multiplex kinds.
pub fn is_g4a_channel(kind: ChannelKind) -> bool {
    matches!(
        kind,
        ChannelKind::G4aControl | ChannelKind::G4aPty | ChannelKind::G4aEvents
    )
}

/// Parse opaque payload bytes as a g4a message. C2 must not call this.
pub fn parse_g4a_payload(payload: &[u8]) -> Result<G4aMessage, NodeControllerError> {
    let value: Value =
        serde_json::from_slice(payload).map_err(|_| NodeControllerError::InvalidJson)?;
    if let Ok(frame) = serde_json::from_value::<C2ServerFrame>(value.clone()) {
        return Ok(G4aMessage::ServerFrame(frame));
    }
    if let Ok(event) = serde_json::from_value::<RoutedNodeEvent>(value.clone()) {
        return Ok(G4aMessage::Event(event));
    }
    if let Ok(response) = serde_json::from_value::<RoutedNodeResponse>(value.clone()) {
        return Ok(G4aMessage::Response(response));
    }
    if let Ok(request) = serde_json::from_value::<RoutedNodeRequest>(value.clone()) {
        return Ok(G4aMessage::Request(request));
    }
    Ok(G4aMessage::Json(value))
}

/// Handle one drain envelope: only g4a channels are parsed; others stay opaque.
pub fn handle_drain_envelope(
    envelope: &DrainEnvelope,
    channel_kind: Option<ChannelKind>,
) -> Result<Option<G4aMessage>, NodeControllerError> {
    if let Some(kind) = channel_kind {
        if !is_g4a_channel(kind) {
            return Err(NodeControllerError::NotG4a);
        }
    }
    // DATA frames carry the payload; non-DATA is ignored here.
    if envelope.frame.payload.is_empty() {
        return Ok(None);
    }
    Ok(Some(parse_g4a_payload(&envelope.frame.payload)?))
}

/// Drain worker: pull envelopes from C2 and parse g4a on core.
pub struct DrainWorker {
    pub client: hatchery_c2_client::DrainClient,
}

impl DrainWorker {
    pub fn new(client: hatchery_c2_client::DrainClient) -> Self {
        Self { client }
    }

    pub async fn connect_and_open(
        &self,
        node_id: &str,
        channel_id: u32,
        kind: ChannelKind,
        after: u64,
    ) -> Result<hatchery_c2_client::DrainSession, std::io::Error> {
        let mut session = self
            .client
            .connect()
            .await
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        session
            .open(node_id, channel_id, kind)
            .await
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        session
            .resume(after)
            .await
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        Ok(session)
    }
}

/// Operator control routed through core (A2): core sends g4a requests down a node's
/// control stream and correlates replies. C2 relays the bytes without parsing them.
///
/// Wire (inner plaintext): `{"op_id": u64, "g4a": <frame>}` both ways. Messages without
/// `op_id` are node-initiated events and go to the events channel.
#[derive(Clone)]
pub struct OperatorLink {
    tx: tokio::sync::mpsc::Sender<Vec<u8>>,
    pending: std::sync::Arc<std::sync::Mutex<std::collections::HashMap<u64, tokio::sync::oneshot::Sender<G4aMessage>>>>,
    browser_pending: std::sync::Arc<std::sync::Mutex<std::collections::HashMap<u64, tokio::sync::oneshot::Sender<BrowserOpReply>>>>,
    next: std::sync::Arc<std::sync::atomic::AtomicU64>,
    events: Option<tokio::sync::mpsc::UnboundedSender<G4aMessage>>,
    browser_events: Option<tokio::sync::mpsc::UnboundedSender<BrowserStreamEvent>>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum OperatorError {
    #[error("node control stream closed")]
    Closed,
    #[error("node did not answer in time")]
    Timeout,
    #[error("request does not serialize")]
    Encode,
}

impl OperatorLink {
    /// Returns the link and the outbound queue to hand to `DrainConsumer::with_outbound`.
    pub fn new(events: Option<tokio::sync::mpsc::UnboundedSender<G4aMessage>>) -> (Self, tokio::sync::mpsc::Receiver<Vec<u8>>) {
        Self::new_with_browser(events, None)
    }

    pub fn new_with_browser(
        events: Option<tokio::sync::mpsc::UnboundedSender<G4aMessage>>,
        browser_events: Option<tokio::sync::mpsc::UnboundedSender<BrowserStreamEvent>>,
    ) -> (Self, tokio::sync::mpsc::Receiver<Vec<u8>>) {
        let (tx, rx) = tokio::sync::mpsc::channel(64);
        (
            Self {
                tx,
                pending: Default::default(),
                browser_pending: Default::default(),
                next: std::sync::Arc::new(1.into()),
                events,
                browser_events,
            },
            rx,
        )
    }

    /// Send a browser sidecar op and wait for the node's BrowserOpReply.
    pub async fn browser_op(
        &self,
        op: &BrowserClientOp,
        timeout: std::time::Duration,
    ) -> Result<BrowserOpReply, OperatorError> {
        let op_id = self.next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let bytes = serde_json::to_vec(&serde_json::json!({ "op_id": op_id, "browser": op }))
            .map_err(|_| OperatorError::Encode)?;
        let (done, wait) = tokio::sync::oneshot::channel();
        self.browser_pending.lock().expect("pending").insert(op_id, done);
        if self.tx.send(bytes).await.is_err() {
            self.browser_pending.lock().expect("pending").remove(&op_id);
            return Err(OperatorError::Closed);
        }
        match tokio::time::timeout(timeout, wait).await {
            Ok(Ok(msg)) => Ok(msg),
            Ok(Err(_)) => Err(OperatorError::Closed),
            Err(_) => {
                self.browser_pending.lock().expect("pending").remove(&op_id);
                Err(OperatorError::Timeout)
            }
        }
    }

    /// Enqueue one g4a request toward the node now (preserving send order) and
    /// return a future that waits for its reply, so callers can pipeline.
    pub async fn request_pipelined<T: serde::Serialize>(
        &self,
        frame: &T,
        timeout: std::time::Duration,
    ) -> Result<impl std::future::Future<Output = Result<G4aMessage, OperatorError>> + Send + 'static, OperatorError> {
        let op_id = self.next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let g4a = serde_json::to_value(frame).map_err(|_| OperatorError::Encode)?;
        let bytes = serde_json::to_vec(&serde_json::json!({ "op_id": op_id, "g4a": g4a })).map_err(|_| OperatorError::Encode)?;
        let (done, wait) = tokio::sync::oneshot::channel();
        self.pending.lock().expect("pending").insert(op_id, done);
        if self.tx.send(bytes).await.is_err() {
            self.pending.lock().expect("pending").remove(&op_id);
            return Err(OperatorError::Closed);
        }
        let pending = self.pending.clone();
        Ok(async move {
            match tokio::time::timeout(timeout, wait).await {
                Ok(Ok(msg)) => Ok(msg),
                Ok(Err(_)) => Err(OperatorError::Closed),
                Err(_) => {
                    pending.lock().expect("pending").remove(&op_id);
                    Err(OperatorError::Timeout)
                }
            }
        })
    }

    /// Enqueue a browser op now (ordered) and return a future for its reply.
    pub async fn browser_op_pipelined(
        &self,
        op: &BrowserClientOp,
        timeout: std::time::Duration,
    ) -> Result<impl std::future::Future<Output = Result<BrowserOpReply, OperatorError>> + Send + 'static, OperatorError> {
        let op_id = self.next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let bytes = serde_json::to_vec(&serde_json::json!({ "op_id": op_id, "browser": op }))
            .map_err(|_| OperatorError::Encode)?;
        let (done, wait) = tokio::sync::oneshot::channel();
        self.browser_pending.lock().expect("pending").insert(op_id, done);
        if self.tx.send(bytes).await.is_err() {
            self.browser_pending.lock().expect("pending").remove(&op_id);
            return Err(OperatorError::Closed);
        }
        let pending = self.browser_pending.clone();
        Ok(async move {
            match tokio::time::timeout(timeout, wait).await {
                Ok(Ok(msg)) => Ok(msg),
                Ok(Err(_)) => Err(OperatorError::Closed),
                Err(_) => {
                    pending.lock().expect("pending").remove(&op_id);
                    Err(OperatorError::Timeout)
                }
            }
        })
    }

    /// Send one g4a request (e.g. a `RoutedNodeRequest`) and wait for its reply.
    pub async fn request<T: serde::Serialize>(&self, frame: &T, timeout: std::time::Duration) -> Result<G4aMessage, OperatorError> {
        let op_id = self.next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let g4a = serde_json::to_value(frame).map_err(|_| OperatorError::Encode)?;
        let bytes = serde_json::to_vec(&serde_json::json!({ "op_id": op_id, "g4a": g4a })).map_err(|_| OperatorError::Encode)?;
        let (done, wait) = tokio::sync::oneshot::channel();
        self.pending.lock().expect("pending").insert(op_id, done);
        if self.tx.send(bytes).await.is_err() {
            self.pending.lock().expect("pending").remove(&op_id);
            return Err(OperatorError::Closed);
        }
        match tokio::time::timeout(timeout, wait).await {
            Ok(Ok(msg)) => Ok(msg),
            Ok(Err(_)) => Err(OperatorError::Closed),
            Err(_) => {
                self.pending.lock().expect("pending").remove(&op_id);
                Err(OperatorError::Timeout)
            }
        }
    }

    /// Drain handler for the node's g4a control channel: parses g4a on core only.
    pub fn handler(self) -> crate::drain_consumer::CoreHandler {
        std::sync::Arc::new(move |_node: &str, kind: ChannelKind, plain: &[u8]| {
            if !is_g4a_channel(kind) {
                return None;
            }
            let Ok(v) = serde_json::from_slice::<Value>(plain) else { return None };
            // Browser sidecar (same Noise channel, not g4a).
            if let Some(browser) = v.get("browser") {
                if let Some(id) = v.get("op_id").and_then(Value::as_u64) {
                    if let Ok(reply) = serde_json::from_value::<BrowserOpReply>(browser.clone()) {
                        if let Some(done) = self.browser_pending.lock().expect("pending").remove(&id) {
                            let _ = done.send(reply);
                        }
                    }
                } else if let Ok(ev) = serde_json::from_value::<BrowserStreamEvent>(browser.clone()) {
                    if let Some(tx) = &self.browser_events {
                        let _ = tx.send(ev);
                    }
                }
                return None;
            }
            let g4a = v.get("g4a").cloned().unwrap_or(Value::Null);
            let Ok(bytes) = serde_json::to_vec(&g4a) else { return None };
            let Ok(msg) = parse_g4a_payload(&bytes) else { return None };
            match v.get("op_id").and_then(Value::as_u64) {
                Some(id) => {
                    if let Some(done) = self.pending.lock().expect("pending").remove(&id) {
                        let _ = done.send(msg);
                    }
                }
                None => {
                    if let Some(ev) = &self.events {
                        let _ = ev.send(msg);
                    }
                }
            }
            None
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hatchery_core::Frame;

    #[test]
    fn parses_unknown_json_as_forward_compatible() {
        let msg = parse_g4a_payload(br#"{"hello":1}"#).unwrap();
        assert!(matches!(msg, G4aMessage::Json(_)));
    }

    #[test]
    fn rejects_non_g4a_channel() {
        let env = DrainEnvelope {
            node_id: "n".into(),
            frame: Frame::data(1, 1, br#"{"x":1}"#.to_vec()).unwrap(),
        };
        let err = handle_drain_envelope(&env, Some(ChannelKind::M4a)).unwrap_err();
        assert_eq!(err, NodeControllerError::NotG4a);
    }

    #[test]
    fn g4a_channel_parses_payload() {
        let env = DrainEnvelope {
            node_id: "n".into(),
            frame: Frame::data(1, 1, br#"{"x":1}"#.to_vec()).unwrap(),
        };
        let msg = handle_drain_envelope(&env, Some(ChannelKind::G4aEvents))
            .unwrap()
            .unwrap();
        assert!(matches!(msg, G4aMessage::Json(_)));
    }
}
