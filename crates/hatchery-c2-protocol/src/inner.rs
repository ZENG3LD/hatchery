//! Inner-layer tagging inside restreamer DATA payloads (A2 / A3).
//!
//! After C2 strips its outer layer the remaining bytes are `tag || body`.
//! C2 never reads the tag; only the two ends of the inner Noise session do.

/// Noise_NK message 1 (`-> e, es`), node to core.
pub const TAG_HANDSHAKE_1: u8 = 1;
/// Noise_NK message 2 (`<- e, ee`), core to node.
pub const TAG_HANDSHAKE_2: u8 = 2;
/// Noise transport ciphertext, either direction.
pub const TAG_TRANSPORT: u8 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InnerKind {
    Handshake1,
    Handshake2,
    Transport,
}

impl InnerKind {
    pub fn tag(self) -> u8 {
        match self {
            Self::Handshake1 => TAG_HANDSHAKE_1,
            Self::Handshake2 => TAG_HANDSHAKE_2,
            Self::Transport => TAG_TRANSPORT,
        }
    }
}

pub fn encode_inner(kind: InnerKind, body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(1 + body.len());
    out.push(kind.tag());
    out.extend_from_slice(body);
    out
}

pub fn decode_inner(bytes: &[u8]) -> Option<(InnerKind, &[u8])> {
    let (&tag, body) = bytes.split_first()?;
    let kind = match tag {
        TAG_HANDSHAKE_1 => InnerKind::Handshake1,
        TAG_HANDSHAKE_2 => InnerKind::Handshake2,
        TAG_TRANSPORT => InnerKind::Transport,
        _ => return None,
    };
    Some((kind, body))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_unknown_tag() {
        let b = encode_inner(InnerKind::Transport, b"x");
        assert_eq!(decode_inner(&b), Some((InnerKind::Transport, &b"x"[..])));
        assert_eq!(decode_inner(&[9, 1]), None);
        assert_eq!(decode_inner(&[]), None);
    }
}
