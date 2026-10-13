//! Core-side node controller (A2): g4a parsing lives here, not on C2.
//!
//! C2's restreamer forwards opaque multiplex DATA. Core drains the stream,
//! strips the inner Noise layer (caller-supplied transport), then this module
//! decodes g4a frames (`RoutedNodeEvent` / `RoutedNodeResponse` / `C2ServerFrame`).

use hatchery_c2_protocol::{
    ChannelKind, DrainEnvelope, RoutedNodeEvent, RoutedNodeRequest, RoutedNodeResponse,
    C2ServerFrame,
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

#[cfg(test)]
mod tests {
    use super::*;
    use hatchery_c2_protocol::Frame;

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
