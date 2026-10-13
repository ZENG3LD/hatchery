//! Production node-call sink on core (A2): applies node calls to the [`Hub`].
//!
//! Driven by the [`crate::drain_consumer`]: the node id comes from the drained stream,
//! the op and payload from the decrypted inner message. The hub re-checks the ledger.

use crate::drain_consumer::CoreHandler;
use crate::hub::Hub;
use hatchery_c2_protocol::ChannelKind;
use hatchery_enroll::call::ForwardedCall;
use serde_json::{json, Value};
use std::sync::Arc;

pub struct HubNodeSink {
    hub: Arc<Hub>,
}

impl HubNodeSink {
    pub fn new(hub: Arc<Hub>) -> Self {
        Self { hub }
    }

    /// Apply one call (blocking). Returns (status, JSON body).
    pub fn apply(&self, call: &ForwardedCall) -> (u16, Value) {
        match self.hub.node_call(&call.node_id, &call.op, &call.payload) {
            Ok(value) => (200, value),
            Err(err) => (err.status, json!({ "error": err.message })),
        }
    }

    /// Drain handler for `m4a` / g4a node-call channels: `{"op","payload"}` in,
    /// `{"status","body"}` out.
    pub fn handler(self: Arc<Self>) -> CoreHandler {
        Arc::new(move |node_id: &str, _kind: ChannelKind, plain: &[u8]| {
            let reply = match serde_json::from_slice::<Value>(plain) {
                Ok(req) => {
                    let op = req.get("op").and_then(Value::as_str).unwrap_or_default().to_owned();
                    let payload = req.get("payload").cloned().unwrap_or(Value::Null);
                    let (status, body) = self.apply(&ForwardedCall { node_id: node_id.to_owned(), op, payload });
                    json!({ "status": status, "body": body })
                }
                Err(_) => json!({ "status": 400, "body": { "error": "bad request" } }),
            };
            serde_json::to_vec(&reply).ok()
        })
    }
}
