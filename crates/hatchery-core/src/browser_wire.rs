//! Opaque browser-stream wire (dig2-compatible).
//!
//! Carried inside the restream G4aControl sidecar as `{"browser": ...}` JSON, or as
//! raw input bytes inside that JSON. C2 never parses these bytes.

use serde::{Deserialize, Serialize};

const WIRE_VERSION: u8 = 1;
const KIND_MOUSE: u8 = 1;
const KIND_KEY: u8 = 2;
const KIND_SCROLL: u8 = 3;
const KIND_NAV: u8 = 4;
const KIND_TEXT: u8 = 5;
const MOUSE_CLICK: u8 = 4;
const KEY_PRESS: u8 = 3;
const MAX_KEY: usize = 64;
const MAX_TEXT: usize = 2048;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserMouseButton {
    Left,
    Middle,
    Right,
}

/// HQ → node browser control (over the restream control channel).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum BrowserClientOp {
    Start {
        session: String,
        #[serde(default)]
        url: String,
        #[serde(default)]
        profile_dir: Option<String>,
    },
    /// Dig2-compatible input bytes (click / scroll / key / text / nav).
    Input {
        session: String,
        #[serde(with = "serde_bytes_b64")]
        bytes: Vec<u8>,
    },
    Stop {
        session: String,
    },
    /// Session-local server pool (tesserax-http). Reply body carries JSON.
    LocalServerStart {
        session: String,
        kind: String,
        #[serde(default)]
        root: Option<String>,
        #[serde(default)]
        cwd: Option<String>,
        #[serde(default)]
        bin: Option<String>,
        #[serde(default)]
        open: bool,
        #[serde(default)]
        spa: Option<bool>,
    },
    LocalServerStop {
        session: String,
        id: String,
    },
    LocalServerList {
        session: String,
    },
}

/// Node → HQ browser events (frames and lifecycle).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum BrowserStreamEvent {
    Started {
        node_id: String,
        session: String,
        url: String,
    },
    Frame {
        node_id: String,
        session: String,
        /// PNG or JPEG bytes.
        #[serde(with = "serde_bytes_b64")]
        bytes: Vec<u8>,
        width: u32,
        height: u32,
        #[serde(default)]
        url: String,
        #[serde(default)]
        title: String,
        /// Latency trace for the first frame after an input (node-side hops).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        trace: Option<BrowserFrameTrace>,
        /// Delta frame: `bytes` covers only this region of the full image.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        region: Option<BrowserFrameRegion>,
    },
    Stopped {
        node_id: String,
        session: String,
        #[serde(default)]
        reason: String,
    },
    Error {
        node_id: String,
        session: String,
        message: String,
    },
    /// Result of a LocalServer* control op (list/start/stop), for HQ sidebar.
    LocalServer {
        node_id: String,
        session: String,
        ok: bool,
        #[serde(default)]
        message: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        body: Option<serde_json::Value>,
    },
}

/// Changed rectangle of a delta frame, in source-image pixels.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserFrameRegion {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
    /// Full source image size the region is relative to.
    pub src_w: u32,
    pub src_h: u32,
}

/// Node-side timings (milliseconds) for the frame that first reflects an input.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserFrameTrace {
    /// Count of inputs this browser session has received (monotonic).
    pub input_n: u64,
    /// Bridge receipt → CDP dispatch start.
    pub queue_ms: u32,
    /// CDP Input.dispatch* round trip(s).
    pub cdp_ms: u32,
    /// CDP dispatch done → first changed frame available on the node.
    pub to_frame_ms: u32,
    /// Total node time (receipt → frame handed to the uplink).
    pub node_ms: u32,
    /// "screencast" or "poll".
    #[serde(default)]
    pub source: String,
}

/// Reply to a BrowserClientOp (correlated by op_id on the control channel).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserOpReply {
    pub ok: bool,
    pub session: String,
    #[serde(default)]
    pub message: String,
    /// JSON body for local_server list/start replies (opaque to C2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<serde_json::Value>,
}

mod serde_bytes_b64 {
    use base64::Engine;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &Vec<u8>, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&base64::engine::general_purpose::STANDARD.encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let text = String::deserialize(d)?;
        base64::engine::general_purpose::STANDARD
            .decode(text.as_bytes())
            .map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum BrowserInput {
    Click {
        x: f64,
        y: f64,
        button: BrowserMouseButton,
    },
    Scroll {
        x: f64,
        y: f64,
        dx: f64,
        dy: f64,
    },
    Key {
        key: String,
    },
    Text {
        text: String,
    },
    Navigate {
        url: String,
    },
}

pub fn encode_click(x: f64, y: f64, button: BrowserMouseButton) -> Vec<u8> {
    let mut out = vec![
        WIRE_VERSION,
        KIND_MOUSE,
        MOUSE_CLICK,
        match button {
            BrowserMouseButton::Left => 0,
            BrowserMouseButton::Middle => 1,
            BrowserMouseButton::Right => 2,
        },
    ];
    out.extend_from_slice(&x.to_le_bytes());
    out.extend_from_slice(&y.to_le_bytes());
    out
}

pub fn encode_scroll(x: f64, y: f64, dx: f64, dy: f64) -> Vec<u8> {
    let mut out = vec![WIRE_VERSION, KIND_SCROLL, 0];
    for v in [x, y, dx, dy] {
        out.extend_from_slice(&(v as f32).to_le_bytes());
    }
    out
}

pub fn encode_key(key: &str) -> Result<Vec<u8>, String> {
    let bytes = key.as_bytes();
    if bytes.is_empty() || bytes.len() > MAX_KEY {
        return Err("key length must be 1..=64".into());
    }
    let mut out = vec![WIRE_VERSION, KIND_KEY, KEY_PRESS, bytes.len() as u8];
    out.extend_from_slice(bytes);
    Ok(out)
}

pub fn encode_navigate(url: &str) -> Result<Vec<u8>, String> {
    encode_text_like(KIND_NAV, url)
}

pub fn encode_text(text: &str) -> Result<Vec<u8>, String> {
    encode_text_like(KIND_TEXT, text)
}

fn encode_text_like(kind: u8, value: &str) -> Result<Vec<u8>, String> {
    let bytes = value.as_bytes();
    if bytes.is_empty() || bytes.len() > MAX_TEXT {
        return Err("value length must be 1..=2048".into());
    }
    let mut out = vec![WIRE_VERSION, kind];
    out.extend_from_slice(&(bytes.len() as u16).to_le_bytes());
    out.extend_from_slice(bytes);
    Ok(out)
}

pub fn decode_input(bytes: &[u8]) -> Result<BrowserInput, String> {
    if bytes.len() < 2 || bytes[0] != WIRE_VERSION {
        return Err("bad browser wire version".into());
    }
    match bytes[1] {
        KIND_MOUSE => {
            if bytes.len() < 20 {
                return Err("short mouse event".into());
            }
            let button = match bytes[3] {
                0 => BrowserMouseButton::Left,
                1 => BrowserMouseButton::Middle,
                2 => BrowserMouseButton::Right,
                _ => return Err("bad mouse button".into()),
            };
            let x = f64::from_le_bytes(bytes[4..12].try_into().unwrap());
            let y = f64::from_le_bytes(bytes[12..20].try_into().unwrap());
            Ok(BrowserInput::Click { x, y, button })
        }
        KIND_KEY => {
            if bytes.len() < 4 {
                return Err("short key event".into());
            }
            let n = bytes[3] as usize;
            if bytes.len() < 4 + n || n == 0 || n > MAX_KEY {
                return Err("bad key length".into());
            }
            let key = String::from_utf8(bytes[4..4 + n].to_vec()).map_err(|e| e.to_string())?;
            Ok(BrowserInput::Key { key })
        }
        KIND_SCROLL => {
            if bytes.len() < 19 {
                return Err("short scroll event".into());
            }
            let mut vals = [0f64; 4];
            for (i, slot) in vals.iter_mut().enumerate() {
                let start = 3 + i * 4;
                *slot = f32::from_le_bytes(bytes[start..start + 4].try_into().unwrap()) as f64;
            }
            Ok(BrowserInput::Scroll {
                x: vals[0],
                y: vals[1],
                dx: vals[2],
                dy: vals[3],
            })
        }
        KIND_NAV | KIND_TEXT => {
            if bytes.len() < 4 {
                return Err("short text-like event".into());
            }
            let n = u16::from_le_bytes(bytes[2..4].try_into().unwrap()) as usize;
            if n == 0 || n > MAX_TEXT || bytes.len() < 4 + n {
                return Err("bad text-like length".into());
            }
            let value = String::from_utf8(bytes[4..4 + n].to_vec()).map_err(|e| e.to_string())?;
            if bytes[1] == KIND_NAV {
                Ok(BrowserInput::Navigate { url: value })
            } else {
                Ok(BrowserInput::Text { text: value })
            }
        }
        other => Err(format!("unknown browser input kind {other}")),
    }
}

pub fn validate_session_id(session: &str) -> Result<(), String> {
    if session.is_empty()
        || session.len() > 64
        || session.starts_with('.')
        || !session
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    {
        return Err("bad browser session id".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_inputs() {
        let click = encode_click(120.0, 90.0, BrowserMouseButton::Left);
        assert!(matches!(
            decode_input(&click).unwrap(),
            BrowserInput::Click {
                x,
                y,
                button: BrowserMouseButton::Left
            } if (x - 120.0).abs() < 0.01 && (y - 90.0).abs() < 0.01
        ));
        let scroll = encode_scroll(10.0, 20.0, 0.0, 300.0);
        assert!(matches!(decode_input(&scroll).unwrap(), BrowserInput::Scroll { dy, .. } if (dy - 300.0).abs() < 0.1));
        let key = encode_key("Enter").unwrap();
        assert!(matches!(decode_input(&key).unwrap(), BrowserInput::Key { key } if key == "Enter"));
        let nav = encode_navigate("https://example.org/").unwrap();
        assert!(matches!(decode_input(&nav).unwrap(), BrowserInput::Navigate { url } if url == "https://example.org/"));
        let text = encode_text("hi").unwrap();
        assert!(matches!(decode_input(&text).unwrap(), BrowserInput::Text { text } if text == "hi"));
    }

    #[test]
    fn frame_event_json_round_trip() {
        let ev = BrowserStreamEvent::Frame {
            node_id: "n1".into(),
            session: "s1".into(),
            bytes: vec![0x89, b'P', b'N', b'G'],
            width: 800,
            height: 600,
            url: "about:blank".into(),
            title: "t".into(),
            trace: None,
            region: None,
        };
        let v = serde_json::to_value(&ev).unwrap();
        let back: BrowserStreamEvent = serde_json::from_value(v).unwrap();
        assert_eq!(back, ev);
    }
}
