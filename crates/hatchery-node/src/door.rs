//! Local door for agents on this node: mail and task operations without ever touching a key.
//!
//! The node holds its identity key; agents (other users, other processes) do not. This loopback
//! listener accepts `POST /door {"op","payload"}` from them, signs the call with the node identity
//! and forwards it to C2, which applies the status policy. A node that is neutral, suspended or
//! revoked simply gets "refused" back. The surface is exactly the `mail.*` and `task.*` operations.

use hatchery_build::http;
use hatchery_enroll::call::capability_for;
use hatchery_enroll::NodeIdentity;
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;

pub async fn serve(listener: TcpListener, c2_enroll: SocketAddr, identity: Arc<NodeIdentity>, node_id: String) {
    loop {
        let Ok((mut stream, peer)) = listener.accept().await else { continue };
        if !peer.ip().is_loopback() {
            continue;
        }
        let (identity, node_id) = (Arc::clone(&identity), node_id.clone());
        tokio::spawn(async move {
            let Ok(Ok((method, path, _, body))) = tokio::time::timeout(std::time::Duration::from_secs(5), http::read_request(&mut stream, 64 * 1024)).await else {
                let _ = http::respond(&mut stream, 400, br#"{"error":"bad request"}"#).await;
                return;
            };
            if method != "POST" || path != "/door" {
                let _ = http::respond(&mut stream, 404, br#"{"error":"not found"}"#).await;
                return;
            }
            let Ok(request) = serde_json::from_slice::<Value>(&body) else {
                let _ = http::respond(&mut stream, 400, br#"{"error":"malformed"}"#).await;
                return;
            };
            let op = request.get("op").and_then(Value::as_str).unwrap_or_default();
            if capability_for(op).is_none() {
                let _ = http::respond(&mut stream, 404, br#"{"error":"unknown operation"}"#).await;
                return;
            }
            let payload = request.get("payload").cloned().unwrap_or_else(|| json!({}));
            let (status, reply) = match crate::enroll_client::node_call(c2_enroll, &identity, &node_id, op, &payload).await {
                Ok(value) => (200, value.to_string()),
                Err(crate::enroll_client::EnrollClientError::Refused) => (403, json!({"error":"refused"}).to_string()),
                Err(_) => (502, json!({"error":"unavailable"}).to_string()),
            };
            let _ = http::respond(&mut stream, status, reply.as_bytes()).await;
        });
    }
}
