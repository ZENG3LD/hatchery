//! Binary multiplex framing for the C2 byte restreamer (A2).
//!
//! C2 sees only the envelope: `seq`, destination, channel kind and size.
//! Payload bytes are opaque (inner ciphertext). No g4a / m4a parsing here.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Wire version for multiplex frames.
pub const MULTIPLEX_VERSION: u8 = 1;

/// Maximum DATA payload length (1 MiB). Larger payloads are split by the sender.
pub const MAX_DATA_PAYLOAD: usize = 1024 * 1024;

/// Frame header size before the payload: version, type, channel_id, seq, length.
pub const FRAME_HEADER_LEN: usize = 1 + 1 + 4 + 8 + 4;

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameType {
    Open = 1,
    Data = 2,
    Close = 3,
    Window = 4,
    Ping = 5,
    Pong = 6,
    GoAway = 7,
    Ack = 8,
    Resume = 9,
    /// C2 → node: request journal entries with seq > N.
    JournalReq = 10,
    /// node → C2: journal entries (payload = JournalReplyBody).
    JournalReply = 11,
}

impl FrameType {
    pub fn from_u8(v: u8) -> Option<Self> {
        Some(match v {
            1 => Self::Open,
            2 => Self::Data,
            3 => Self::Close,
            4 => Self::Window,
            5 => Self::Ping,
            6 => Self::Pong,
            7 => Self::GoAway,
            8 => Self::Ack,
            9 => Self::Resume,
            10 => Self::JournalReq,
            11 => Self::JournalReply,
            _ => return None,
        })
    }
}

/// Logical channel kinds C2 routes without inspecting contents.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelKind {
    G4aControl,
    G4aPty,
    G4aEvents,
    M4a,
    Vault,
    Observation,
}

impl ChannelKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::G4aControl => "g4a.control",
            Self::G4aPty => "g4a.pty",
            Self::G4aEvents => "g4a.events",
            Self::M4a => "m4a",
            Self::Vault => "vault",
            Self::Observation => "observation",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "g4a.control" => Self::G4aControl,
            "g4a.pty" => Self::G4aPty,
            "g4a.events" => Self::G4aEvents,
            "m4a" => Self::M4a,
            "vault" => Self::Vault,
            "observation" => Self::Observation,
            _ => return None,
        })
    }
}

impl fmt::Display for ChannelKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Payload of an OPEN frame. No secrets.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenPayload {
    pub channel_kind: ChannelKind,
    /// Opaque node identifier assigned by C2; core never sees node addresses.
    pub node_id: String,
}

/// One multiplex frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub version: u8,
    pub frame_type: FrameType,
    pub channel_id: u32,
    /// For DATA: event sequence. For ACK/RESUME: the acknowledged / resume-after number.
    pub seq: u64,
    pub payload: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FrameError {
    Truncated,
    BadVersion(u8),
    BadType(u8),
    PayloadTooLarge(usize),
    Io(String),
}

impl fmt::Display for FrameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Truncated => write!(f, "truncated frame"),
            Self::BadVersion(v) => write!(f, "unsupported multiplex version {v}"),
            Self::BadType(t) => write!(f, "unknown frame type {t}"),
            Self::PayloadTooLarge(n) => write!(f, "payload length {n} exceeds limit"),
            Self::Io(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for FrameError {}

impl Frame {
    pub fn open(channel_id: u32, payload: &OpenPayload) -> Result<Self, FrameError> {
        let body = serde_json::to_vec(payload).map_err(|e| FrameError::Io(e.to_string()))?;
        Ok(Self {
            version: MULTIPLEX_VERSION,
            frame_type: FrameType::Open,
            channel_id,
            seq: 0,
            payload: body,
        })
    }

    pub fn data(channel_id: u32, seq: u64, payload: Vec<u8>) -> Result<Self, FrameError> {
        if payload.len() > MAX_DATA_PAYLOAD {
            return Err(FrameError::PayloadTooLarge(payload.len()));
        }
        Ok(Self {
            version: MULTIPLEX_VERSION,
            frame_type: FrameType::Data,
            channel_id,
            seq,
            payload,
        })
    }

    pub fn ack(seq: u64) -> Self {
        Self {
            version: MULTIPLEX_VERSION,
            frame_type: FrameType::Ack,
            channel_id: 0,
            seq,
            payload: Vec::new(),
        }
    }

    pub fn resume(after: u64) -> Self {
        Self {
            version: MULTIPLEX_VERSION,
            frame_type: FrameType::Resume,
            channel_id: 0,
            seq: after,
            payload: Vec::new(),
        }
    }

    pub fn window(channel_id: u32, credit: u32) -> Self {
        Self {
            version: MULTIPLEX_VERSION,
            frame_type: FrameType::Window,
            channel_id,
            seq: u64::from(credit),
            payload: Vec::new(),
        }
    }

    pub fn ping(nonce: u64) -> Self {
        Self {
            version: MULTIPLEX_VERSION,
            frame_type: FrameType::Ping,
            channel_id: 0,
            seq: nonce,
            payload: Vec::new(),
        }
    }

    pub fn pong(nonce: u64) -> Self {
        Self {
            version: MULTIPLEX_VERSION,
            frame_type: FrameType::Pong,
            channel_id: 0,
            seq: nonce,
            payload: Vec::new(),
        }
    }

    pub fn goaway(code: u32) -> Self {
        Self {
            version: MULTIPLEX_VERSION,
            frame_type: FrameType::GoAway,
            channel_id: 0,
            seq: u64::from(code),
            payload: Vec::new(),
        }
    }

    pub fn close(channel_id: u32) -> Self {
        Self {
            version: MULTIPLEX_VERSION,
            frame_type: FrameType::Close,
            channel_id,
            seq: 0,
            payload: Vec::new(),
        }
    }

    /// Encode big-endian wire bytes.
    pub fn encode(&self) -> Result<Vec<u8>, FrameError> {
        if self.payload.len() > MAX_DATA_PAYLOAD {
            return Err(FrameError::PayloadTooLarge(self.payload.len()));
        }
        let mut out = Vec::with_capacity(FRAME_HEADER_LEN + self.payload.len());
        out.push(self.version);
        out.push(self.frame_type as u8);
        out.extend_from_slice(&self.channel_id.to_be_bytes());
        out.extend_from_slice(&self.seq.to_be_bytes());
        out.extend_from_slice(&(self.payload.len() as u32).to_be_bytes());
        out.extend_from_slice(&self.payload);
        Ok(out)
    }

    /// Decode one frame from a complete buffer. Returns `(frame, bytes_consumed)`.
    pub fn decode(buf: &[u8]) -> Result<(Self, usize), FrameError> {
        if buf.len() < FRAME_HEADER_LEN {
            return Err(FrameError::Truncated);
        }
        let version = buf[0];
        if version != MULTIPLEX_VERSION {
            return Err(FrameError::BadVersion(version));
        }
        let frame_type = FrameType::from_u8(buf[1]).ok_or(FrameError::BadType(buf[1]))?;
        let channel_id = u32::from_be_bytes([buf[2], buf[3], buf[4], buf[5]]);
        let seq = u64::from_be_bytes([
            buf[6], buf[7], buf[8], buf[9], buf[10], buf[11], buf[12], buf[13],
        ]);
        let length = u32::from_be_bytes([buf[14], buf[15], buf[16], buf[17]]) as usize;
        if length > MAX_DATA_PAYLOAD {
            return Err(FrameError::PayloadTooLarge(length));
        }
        let total = FRAME_HEADER_LEN + length;
        if buf.len() < total {
            return Err(FrameError::Truncated);
        }
        Ok((
            Self {
                version,
                frame_type,
                channel_id,
                seq,
                payload: buf[FRAME_HEADER_LEN..total].to_vec(),
            },
            total,
        ))
    }

    pub fn parse_open(&self) -> Result<OpenPayload, FrameError> {
        if self.frame_type != FrameType::Open {
            return Err(FrameError::Io("not an OPEN frame".into()));
        }
        serde_json::from_slice(&self.payload).map_err(|e| FrameError::Io(e.to_string()))
    }

    /// Journal request: `seq` = after, payload = u32 limit (big-endian).
    pub fn journal_req(channel_id: u32, after: u64, limit: u32) -> Self {
        Self {
            version: MULTIPLEX_VERSION,
            frame_type: FrameType::JournalReq,
            channel_id,
            seq: after,
            payload: limit.to_be_bytes().to_vec(),
        }
    }

    pub fn journal_req_limit(&self) -> Option<u32> {
        if self.frame_type != FrameType::JournalReq || self.payload.len() != 4 {
            return None;
        }
        Some(u32::from_be_bytes([
            self.payload[0],
            self.payload[1],
            self.payload[2],
            self.payload[3],
        ]))
    }

    pub fn journal_reply(channel_id: u32, body: &JournalReplyBody) -> Result<Self, FrameError> {
        let payload = body.encode()?;
        if payload.len() > MAX_DATA_PAYLOAD {
            return Err(FrameError::PayloadTooLarge(payload.len()));
        }
        Ok(Self {
            version: MULTIPLEX_VERSION,
            frame_type: FrameType::JournalReply,
            channel_id,
            seq: body.after,
            payload,
        })
    }
}

/// One journal entry on the wire (opaque payload).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JournalWireEntry {
    pub seq: u64,
    pub payload: Vec<u8>,
}

/// Body of JournalReply: entries with seq > after for one channel.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JournalReplyBody {
    pub after: u64,
    pub entries: Vec<JournalWireEntry>,
}

impl JournalReplyBody {
    pub fn encode(&self) -> Result<Vec<u8>, FrameError> {
        serde_json::to_vec(self).map_err(|e| FrameError::Io(e.to_string()))
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, FrameError> {
        serde_json::from_slice(bytes).map_err(|e| FrameError::Io(e.to_string()))
    }
}

/// Drain-stream envelope: C2 prefixes each uplink DATA with an opaque node id for core.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DrainEnvelope {
    /// Opaque destination / source node id. Not an address.
    pub node_id: String,
    pub frame: Frame,
}

impl DrainEnvelope {
    pub fn encode(&self) -> Result<Vec<u8>, FrameError> {
        let id = self.node_id.as_bytes();
        if id.len() > 255 {
            return Err(FrameError::Io("node_id too long".into()));
        }
        let frame_bytes = self.frame.encode()?;
        let mut out = Vec::with_capacity(1 + id.len() + frame_bytes.len());
        out.push(id.len() as u8);
        out.extend_from_slice(id);
        out.extend_from_slice(&frame_bytes);
        Ok(out)
    }

    pub fn decode(buf: &[u8]) -> Result<(Self, usize), FrameError> {
        if buf.is_empty() {
            return Err(FrameError::Truncated);
        }
        let id_len = buf[0] as usize;
        if buf.len() < 1 + id_len {
            return Err(FrameError::Truncated);
        }
        let node_id = String::from_utf8(buf[1..1 + id_len].to_vec())
            .map_err(|_| FrameError::Io("node_id is not utf-8".into()))?;
        let (frame, n) = Frame::decode(&buf[1 + id_len..])?;
        Ok((Self { node_id, frame }, 1 + id_len + n))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_data_ack_resume() {
        let data = Frame::data(7, 42, b"opaque-ciphertext".to_vec()).unwrap();
        let bytes = data.encode().unwrap();
        let (decoded, n) = Frame::decode(&bytes).unwrap();
        assert_eq!(n, bytes.len());
        assert_eq!(decoded, data);

        let ack = Frame::ack(42);
        let (d, _) = Frame::decode(&ack.encode().unwrap()).unwrap();
        assert_eq!(d.frame_type, FrameType::Ack);
        assert_eq!(d.seq, 42);

        let resume = Frame::resume(41);
        let (d, _) = Frame::decode(&resume.encode().unwrap()).unwrap();
        assert_eq!(d.frame_type, FrameType::Resume);
        assert_eq!(d.seq, 41);
    }

    #[test]
    fn open_payload_roundtrip() {
        let open = Frame::open(
            1,
            &OpenPayload {
                channel_kind: ChannelKind::M4a,
                node_id: "node-a".into(),
            },
        )
        .unwrap();
        let (decoded, _) = Frame::decode(&open.encode().unwrap()).unwrap();
        let payload = decoded.parse_open().unwrap();
        assert_eq!(payload.channel_kind, ChannelKind::M4a);
        assert_eq!(payload.node_id, "node-a");
    }

    #[test]
    fn rejects_oversized_payload() {
        let big = vec![0u8; MAX_DATA_PAYLOAD + 1];
        assert!(matches!(
            Frame::data(1, 1, big),
            Err(FrameError::PayloadTooLarge(_))
        ));
    }

    #[test]
    fn drain_envelope_prefixes_opaque_node_id() {
        let env = DrainEnvelope {
            node_id: "node-b".into(),
            frame: Frame::data(3, 9, b"inner".to_vec()).unwrap(),
        };
        let bytes = env.encode().unwrap();
        let (decoded, n) = DrainEnvelope::decode(&bytes).unwrap();
        assert_eq!(n, bytes.len());
        assert_eq!(decoded.node_id, "node-b");
        assert_eq!(decoded.frame.seq, 9);
        assert_eq!(decoded.frame.payload, b"inner");
    }

    #[test]
    fn journal_req_roundtrip() {
        let f = Frame::journal_req(3, 10, 64);
        let (d, _) = Frame::decode(&f.encode().unwrap()).unwrap();
        assert_eq!(d.frame_type, FrameType::JournalReq);
        assert_eq!(d.seq, 10);
        assert_eq!(d.journal_req_limit(), Some(64));
        let body = JournalReplyBody {
            after: 10,
            entries: vec![JournalWireEntry {
                seq: 11,
                payload: b"x".to_vec(),
            }],
        };
        let reply = Frame::journal_reply(3, &body).unwrap();
        let (d, _) = Frame::decode(&reply.encode().unwrap()).unwrap();
        let decoded = JournalReplyBody::decode(&d.payload).unwrap();
        assert_eq!(decoded.entries[0].seq, 11);
    }

    #[test]
    fn truncated_frame_errors() {
        assert!(matches!(Frame::decode(&[1, 2, 3]), Err(FrameError::Truncated)));
        let mut partial = Frame::data(1, 1, b"hello".to_vec()).unwrap().encode().unwrap();
        partial.pop();
        assert!(matches!(Frame::decode(&partial), Err(FrameError::Truncated)));
    }
}
