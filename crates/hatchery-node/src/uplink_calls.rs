//! Node calls (`mail.*`, `task.*`) over the restreamer uplink.
//!
//! Since A2, C2 no longer forwards signed node calls to core over HTTP: it answers 410.
//! These calls ride a dedicated `m4a` channel on the node's existing uplink. That channel
//! is Noise-encrypted end to end to core and opaque to C2. Core takes the node id from
//! the stream, never from the payload. The legacy HTTP path stays as a fallback for
//! nodes without an uplink.
//!
//! Important: this channel shares the node's single TCP uplink with g4a. A handshake
//! retry storm (or a wedged Noise session on core's m4a drain) can fill the TCP send
//! buffer and stall PTY control. Open failures therefore back off hard.

use crate::restream_session::{NodeSession, NodeUplink, UpstreamKeys};
use hatchery_core::ChannelKind;
use hatchery_enroll::NodeIdentity;
use serde_json::Value;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

/// Channel id of the node-call channel; core runs its `m4a` drain consumer on it.
pub const NODE_CALL_CHANNEL: u32 = 2;
const HANDSHAKE: Duration = Duration::from_secs(8);
const CALL_TIMEOUT: Duration = Duration::from_secs(15);
/// After an open/handshake failure, wait before trying again (shared uplink).
const OPEN_BACKOFF: Duration = Duration::from_secs(60);

#[derive(Default)]
struct Link {
    uplink: Option<Arc<NodeUplink>>,
    session: Option<NodeSession>,
    /// Do not attempt OPEN before this instant (protects the shared uplink).
    open_not_before: Option<Instant>,
}

/// Shared by the door and the node-mail wake; the restream bridge publishes its uplink here.
#[derive(Clone)]
pub struct NodeCalls {
    link: Arc<Mutex<Link>>,
    node_id: String,
    keys: Option<UpstreamKeys>,
    legacy: Option<(SocketAddr, Arc<NodeIdentity>)>,
}

#[derive(Debug)]
pub enum CallError {
    Refused,
    Unavailable(String),
}

impl NodeCalls {
    pub fn new(node_id: String, keys: Option<UpstreamKeys>, legacy: Option<(SocketAddr, Arc<NodeIdentity>)>) -> Self {
        Self { link: Arc::default(), node_id, keys, legacy }
    }

    /// The bridge calls this on every (re)connect; `None` when the uplink is gone.
    pub async fn set_uplink(&self, uplink: Option<Arc<NodeUplink>>) {
        let mut l = self.link.lock().await;
        l.uplink = uplink;
        l.session = None;
        // Fresh uplink: allow an immediate open attempt.
        l.open_not_before = None;
    }

    pub async fn call(&self, op: &str, payload: &Value) -> Result<Value, CallError> {
        if let Some(keys) = self.keys {
            let mut l = self.link.lock().await;
            if let Some(uplink) = l.uplink.clone() {
                if l.session.is_none() {
                    let gated = l.open_not_before.is_some_and(|t| Instant::now() < t);
                    if gated {
                        // Skip OPEN storm; fall through to legacy if any.
                    } else {
                        match uplink.open(&self.node_id, NODE_CALL_CHANNEL, ChannelKind::M4a, keys, HANDSHAKE).await {
                            Ok(s) => {
                                tracing::info!("node calls over the uplink: channel open");
                                l.session = Some(s);
                                l.open_not_before = None;
                            }
                            Err(e) => {
                                tracing::warn!(error = %e, "node-call channel open failed; backing off {}s", OPEN_BACKOFF.as_secs());
                                l.open_not_before = Some(Instant::now() + OPEN_BACKOFF);
                            }
                        }
                    }
                }
                if let Some(session) = l.session.as_mut() {
                    return match session.call(op, payload, CALL_TIMEOUT).await {
                        Ok(v) => Ok(v),
                        Err(e) if e.starts_with("403") => Err(CallError::Refused),
                        Err(e) if e.chars().take(3).all(|c| c.is_ascii_digit()) && e.contains(':') => {
                            // Application-level error from core; keep the session.
                            Err(CallError::Unavailable(e))
                        }
                        Err(e) => {
                            // Transport fault: drop session and back off so we do not
                            // flood OPEN/handshake1 onto a shared uplink C2 is not draining.
                            l.session = None;
                            l.open_not_before = Some(Instant::now() + OPEN_BACKOFF);
                            Err(CallError::Unavailable(e))
                        }
                    };
                }
            }
        }
        match &self.legacy {
            Some((c2, identity)) => match crate::enroll_client::node_call(*c2, identity, &self.node_id, op, payload).await {
                Ok(v) => Ok(v),
                Err(crate::enroll_client::EnrollClientError::Refused) => Err(CallError::Refused),
                Err(e) => Err(CallError::Unavailable(format!("{e:?}"))),
            },
            None => Err(CallError::Unavailable("no uplink".into())),
        }
    }
}
