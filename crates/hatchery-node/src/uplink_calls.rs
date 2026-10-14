//! Node calls (`mail.*`, `task.*`) over the restreamer uplink.
//!
//! Since A2, C2 no longer forwards signed node calls to core over HTTP: it answers 410.
//! These calls now ride a dedicated `m4a` channel on the node's existing uplink. That
//! channel is Noise-encrypted end to end to core and opaque to C2. Core takes the node
//! id from the stream, never from the payload. The legacy HTTP path stays as a fallback
//! for nodes without an uplink.

use crate::restream_session::{NodeSession, NodeUplink, UpstreamKeys};
use hatchery_core::ChannelKind;
use hatchery_enroll::NodeIdentity;
use serde_json::Value;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

/// Channel id of the node-call channel; core runs its `m4a` drain consumer on it.
pub const NODE_CALL_CHANNEL: u32 = 2;
const HANDSHAKE: Duration = Duration::from_secs(10);
const CALL_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Default)]
struct Link {
    uplink: Option<Arc<NodeUplink>>,
    session: Option<NodeSession>,
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
    }

    pub async fn call(&self, op: &str, payload: &Value) -> Result<Value, CallError> {
        if let Some(keys) = self.keys {
            let mut l = self.link.lock().await;
            if let Some(uplink) = l.uplink.clone() {
                if l.session.is_none() {
                    match uplink.open(&self.node_id, NODE_CALL_CHANNEL, ChannelKind::M4a, keys, HANDSHAKE).await {
                        Ok(s) => {
                            tracing::info!("node calls over the uplink: channel open");
                            l.session = Some(s);
                        }
                        Err(e) => tracing::warn!(error = %e, "node-call channel open failed"),
                    }
                }
                if let Some(session) = l.session.as_mut() {
                    // Calls are strictly serialized under the lock: one request, one reply.
                    return match session.call(op, payload, CALL_TIMEOUT).await {
                        Ok(v) => Ok(v),
                        Err(e) if e.starts_with("403") => Err(CallError::Refused),
                        Err(e) if e.contains(':') && e[..3].chars().all(|c| c.is_ascii_digit()) => Err(CallError::Unavailable(e)),
                        Err(e) => {
                            // Transport fault or lost reply: drop the session so replies cannot pair
                            // with the wrong request; the next call opens a fresh one.
                            l.session = None;
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
