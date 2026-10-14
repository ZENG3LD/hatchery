//! Local door for agents on this node: mail and task operations without ever touching a key.
//!
//! The node holds its identity key; agents (other users, other processes) do not. This loopback
//! listener accepts `POST /door {"op","payload"}` from them, forwards the call to core over the
//! node's restreamer uplink (Noise to core; C2 sees opaque bytes), which applies the status policy. A node that is neutral, suspended or
//! revoked simply gets "refused" back. The surface is exactly the `mail.*` and `task.*` operations.

use hatchery_build::http;
use hatchery_enroll::call::capability_for;
use serde_json::{json, Value};
use tokio::net::TcpListener;

/// Process-wide door listen address (for harness MCP env injection).
static DOOR_LISTEN: std::sync::OnceLock<std::net::SocketAddr> = std::sync::OnceLock::new();

/// Loopback door address if the node started `--door-listen`, else None.
pub fn door_listen_addr() -> Option<std::net::SocketAddr> {
    DOOR_LISTEN.get().copied()
}


/// `session.wake` (handled here, never forwarded): `{"token","nick","from","event_id","body"}`,
/// the letter event m4a's `M4A_LETTER_HOOK` emits plus the node's wake token.
pub async fn serve(listener: TcpListener, calls: crate::uplink_calls::NodeCalls, wake: Option<crate::server::node_wake::NodeWake>) {
    if let Ok(addr) = listener.local_addr() {
        let _ = DOOR_LISTEN.set(addr);
    }
    loop {
        let Ok((mut stream, peer)) = listener.accept().await else { continue };
        if !peer.ip().is_loopback() {
            continue;
        }
        let (calls, wake) = (calls.clone(), wake.clone());
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
            if op.starts_with("local_server.") {
                #[cfg(feature = "local-server")]
                {
                    let payload = request.get("payload").cloned().unwrap_or_else(|| json!({}));
                    let (status, reply) = match crate::local_server::handle_op(op, &payload).await {
                        Ok(v) => (200, v.to_string()),
                        Err(e) => (400, json!({"error": e}).to_string()),
                    };
                    let _ = http::respond(&mut stream, status, reply.as_bytes()).await;
                    return;
                }
                #[cfg(not(feature = "local-server"))]
                {
                    let _ = http::respond(&mut stream, 404, br#"{"error":"local-server feature off"}"#).await;
                    return;
                }
            }
            if op == "session.wake" {
                let p = request.get("payload").cloned().unwrap_or_else(|| json!({}));
                let field = |k: &str| p.get(k).and_then(Value::as_str).unwrap_or_default().to_owned();
                let (status, reply) = match &wake {
                    None => (404, json!({"error":"letter wake is off on this node"})),
                    Some(w) if !w.authorized(&field("token")) => (403, json!({"error":"refused"})),
                    Some(w) => {
                        let letter = crate::server::node_wake::NodeLetter { nick: field("nick"), from: field("from"), event_id: field("event_id"), body: field("body") };
                        match w.wake(letter).await {
                            Ok(done) => (200, json!({"ok": done})),
                            Err(why) => (409, json!({"error": why})),
                        }
                    }
                };
                let _ = http::respond(&mut stream, status, reply.to_string().as_bytes()).await;
                return;
            }
            if capability_for(op).is_none() {
                let _ = http::respond(&mut stream, 404, br#"{"error":"unknown operation"}"#).await;
                return;
            }
            let payload = request.get("payload").cloned().unwrap_or_else(|| json!({}));
            let (status, reply) = match calls.call(op, &payload).await {
                Ok(value) => (200, value.to_string()),
                Err(crate::uplink_calls::CallError::Refused) => (403, json!({"error":"refused"}).to_string()),
                Err(crate::uplink_calls::CallError::Unavailable(why)) => {
                    tracing::warn!(%op, reason = %why, "door call unavailable");
                    (502, json!({"error":"unavailable"}).to_string())
                }
            };
            let _ = http::respond(&mut stream, status, reply.as_bytes()).await;
        });
    }
}
