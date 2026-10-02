//! Tip-5/6 underlay: hatchery lab path + **real** tip-6 bridge_reach dial.
//!
//! - Tip-5 lab (`linux_path`): peer-symmetric UDP + AEAD + token barrier for
//!   local hatchery tests. Not a WireGuard daemon. Win/mac refuse clearly.
//! - Tip-6 **real wire**: [`dial_bridge_reach`] / [`BridgeReachClient`] call
//!   published `gate4agent-node-wire::mesh_underlay::{dial_peer,BridgeUnderlayClient}`
//!   (crates.io **0.4.6+**). HQ DialOnly dials node `--bridge-underlay-listen`;
//!   node hosts accept. See [`crate::mesh_role::BridgeReachCite`].
//!
//! Cite: mesh design §1.2 / §1.5 / tips 5–6; recon Linux TUN/WG vs Win/mac.

use crate::mesh_role::{
    assert_underlay_direction_allowed, MeshParticipantRole, MeshRoleError, MeshTcpDialDirection,
};
use std::fmt;

#[cfg(target_os = "linux")]
mod linux_path {
    use super::*;
    use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, CHACHA20_POLY1305};
    use std::net::SocketAddr;
    use tokio::net::UdpSocket;

    const NONCE_LEN: usize = 12;
    const TAG_LEN: usize = 16;
    const MAX_PLAINTEXT: usize = 4_096;

    #[derive(Clone)]
    pub struct TransportKey([u8; 32]);

    impl TransportKey {
        pub fn from_bytes(bytes: [u8; 32]) -> Self {
            Self(bytes)
        }
    }

    impl fmt::Debug for TransportKey {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("TransportKey([redacted])")
        }
    }

    #[derive(Clone)]
    pub struct AuthToken(String);

    impl AuthToken {
        pub fn new(value: impl Into<String>) -> Result<Self, MeshRoleError> {
            let value = value.into();
            if value.is_empty() || value.len() > 4_096 {
                return Err(MeshRoleError::Unauthorized);
            }
            Ok(Self(value))
        }

        fn as_str(&self) -> &str {
            &self.0
        }
    }

    impl fmt::Debug for AuthToken {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("AuthToken([redacted])")
        }
    }

    fn tokens_match(provided: &str, expected: &str) -> bool {
        let left = provided.as_bytes();
        let right = expected.as_bytes();
        if left.len() != right.len() {
            return false;
        }
        left.iter()
            .zip(right)
            .fold(0_u8, |acc, (a, b)| acc | (a ^ b))
            == 0
    }

    pub fn authorize_probe(token: &AuthToken, provided: Option<&str>) -> Result<(), MeshRoleError> {
        match provided {
            Some(got) if tokens_match(got, token.as_str()) => Ok(()),
            _ => Err(MeshRoleError::Unauthorized),
        }
    }

    fn sealing_key(key: &TransportKey) -> Result<LessSafeKey, MeshRoleError> {
        let unbound = UnboundKey::new(&CHACHA20_POLY1305, &key.0)
            .map_err(|_| MeshRoleError::UnderlayNotImplemented)?;
        Ok(LessSafeKey::new(unbound))
    }

    pub struct UnderlayListener {
        socket: UdpSocket,
        key: LessSafeKey,
        auth: AuthToken,
        send_counter: u64,
    }

    pub struct UnderlaySession {
        socket: UdpSocket,
        key: LessSafeKey,
        auth: AuthToken,
        send_counter: u64,
    }

    pub async fn accept_peer(
        role: MeshParticipantRole,
        transport: &TransportKey,
        auth: &AuthToken,
    ) -> Result<UnderlayListener, MeshRoleError> {
        assert_underlay_direction_allowed(role, MeshTcpDialDirection::Accept)?;
        let key = sealing_key(transport)?;
        let socket = UdpSocket::bind("127.0.0.1:0")
            .await
            .map_err(|_| MeshRoleError::UnderlayNotImplemented)?;
        Ok(UnderlayListener {
            socket,
            key,
            auth: auth.clone(),
            send_counter: 0,
        })
    }

    pub async fn dial_peer(
        _role: MeshParticipantRole,
        transport: &TransportKey,
        auth: &AuthToken,
        peer: SocketAddr,
    ) -> Result<UnderlaySession, MeshRoleError> {
        let key = sealing_key(transport)?;
        let socket = UdpSocket::bind("127.0.0.1:0")
            .await
            .map_err(|_| MeshRoleError::UnderlayNotImplemented)?;
        socket
            .connect(peer)
            .await
            .map_err(|_| MeshRoleError::UnderlayNotImplemented)?;
        let mut session = UnderlaySession {
            socket,
            key,
            auth: auth.clone(),
            send_counter: 0,
        };
        session.send_encrypted(b"hatchery-mesh-underlay-v1-hello").await?;
        let reply = session.recv_encrypted().await?;
        if reply != b"hatchery-mesh-underlay-v1-hello-ack" {
            return Err(MeshRoleError::UnderlayNotImplemented);
        }
        Ok(session)
    }

    impl UnderlayListener {
        pub fn local_addr(&self) -> Result<SocketAddr, MeshRoleError> {
            self.socket
                .local_addr()
                .map_err(|_| MeshRoleError::UnderlayNotImplemented)
        }

        pub async fn accept(self) -> Result<UnderlaySession, MeshRoleError> {
            let mut buf = vec![0u8; NONCE_LEN + MAX_PLAINTEXT + TAG_LEN];
            let (n, peer) = self
                .socket
                .recv_from(&mut buf)
                .await
                .map_err(|_| MeshRoleError::UnderlayNotImplemented)?;
            buf.truncate(n);
            let mut session = UnderlaySession {
                socket: self.socket,
                key: self.key,
                auth: self.auth,
                send_counter: self.send_counter,
            };
            session
                .socket
                .connect(peer)
                .await
                .map_err(|_| MeshRoleError::UnderlayNotImplemented)?;
            let plain = decrypt(&session.key, &buf)?;
            if plain != b"hatchery-mesh-underlay-v1-hello" {
                return Err(MeshRoleError::UnderlayNotImplemented);
            }
            session
                .send_encrypted(b"hatchery-mesh-underlay-v1-hello-ack")
                .await?;
            Ok(session)
        }
    }

    impl UnderlaySession {
        pub async fn send_encrypted(&mut self, plaintext: &[u8]) -> Result<(), MeshRoleError> {
            if plaintext.len() > MAX_PLAINTEXT {
                return Err(MeshRoleError::UnderlayNotImplemented);
            }
            let counter = self.send_counter;
            self.send_counter = self
                .send_counter
                .checked_add(1)
                .ok_or(MeshRoleError::UnderlayNotImplemented)?;
            let mut nonce_bytes = [0u8; NONCE_LEN];
            nonce_bytes[4..].copy_from_slice(&counter.to_be_bytes());
            let mut out = Vec::with_capacity(NONCE_LEN + plaintext.len() + TAG_LEN);
            out.extend_from_slice(&nonce_bytes);
            let nonce = Nonce::assume_unique_for_key(nonce_bytes);
            let mut body = plaintext.to_vec();
            self.key
                .seal_in_place_append_tag(nonce, Aad::empty(), &mut body)
                .map_err(|_| MeshRoleError::UnderlayNotImplemented)?;
            out.extend_from_slice(&body);
            self.socket
                .send(&out)
                .await
                .map_err(|_| MeshRoleError::UnderlayNotImplemented)?;
            Ok(())
        }

        pub async fn recv_encrypted(&self) -> Result<Vec<u8>, MeshRoleError> {
            let mut buf = vec![0u8; NONCE_LEN + MAX_PLAINTEXT + TAG_LEN];
            let n = self
                .socket
                .recv(&mut buf)
                .await
                .map_err(|_| MeshRoleError::UnderlayNotImplemented)?;
            buf.truncate(n);
            decrypt(&self.key, &buf)
        }

        pub fn probe(&self, provided: Option<&str>) -> Result<(), MeshRoleError> {
            authorize_probe(&self.auth, provided)
        }
    }

    fn decrypt(key: &LessSafeKey, packet: &[u8]) -> Result<Vec<u8>, MeshRoleError> {
        if packet.len() < NONCE_LEN + TAG_LEN {
            return Err(MeshRoleError::UnderlayNotImplemented);
        }
        let (nonce_bytes, ct) = packet.split_at(NONCE_LEN);
        let mut nonce_arr = [0u8; NONCE_LEN];
        nonce_arr.copy_from_slice(nonce_bytes);
        let nonce = Nonce::assume_unique_for_key(nonce_arr);
        let mut body = ct.to_vec();
        let plain = key
            .open_in_place(nonce, Aad::empty(), &mut body)
            .map_err(|_| MeshRoleError::UnderlayNotImplemented)?;
        Ok(plain.to_vec())
    }

    pub fn open_wireguard_daemon_stub() -> Result<(), MeshRoleError> {
        Err(MeshRoleError::UnderlayNotImplemented)
    }
}

#[cfg(target_os = "linux")]
pub use linux_path::*;

/// Non-Linux: refuse underlay open with a clear platform error.
#[cfg(not(target_os = "linux"))]
pub async fn accept_peer(
    role: MeshParticipantRole,
    _transport: &[u8; 32],
    _auth: &str,
) -> Result<(), MeshRoleError> {
    assert_underlay_direction_allowed(role, MeshTcpDialDirection::Accept)?;
    Err(MeshRoleError::PlatformUnsupported)
}

#[cfg(not(target_os = "linux"))]
pub fn open_wireguard_daemon_stub() -> Result<(), MeshRoleError> {
    Err(MeshRoleError::UnderlayNotImplemented)
}

/// Map g4a mesh underlay errors into hatchery [`MeshRoleError`] (no secrets).
fn map_underlay_err(err: gate4agent_node_wire::mesh_underlay::MeshUnderlayError) -> MeshRoleError {
    use gate4agent_node_wire::mesh_underlay::MeshUnderlayError as E;
    match err {
        E::HqMustNotAcceptUnderlay => MeshRoleError::HqMustNotAcceptUnderlay,
        E::Unauthorized => MeshRoleError::Unauthorized,
        E::PlatformUnsupported { .. } => MeshRoleError::PlatformUnsupported,
        E::WireGuardDaemonNotInThisTip => MeshRoleError::UnderlayNotImplemented,
        E::InvalidTransportKey | E::InvalidAuthToken | E::Path(_) => {
            MeshRoleError::UnderlayNotImplemented
        }
    }
}

/// Live tip-6 client after HQ DialOnly underlay dial + bridge OPEN.
///
/// Wraps `gate4agent_node_wire::mesh_underlay::BridgeUnderlayClient`.
#[cfg(target_os = "linux")]
pub struct BridgeReachClient {
    inner: gate4agent_node_wire::mesh_underlay::BridgeUnderlayClient,
}

#[cfg(target_os = "linux")]
impl BridgeReachClient {
    pub async fn write_all(&mut self, data: &[u8]) -> Result<(), MeshRoleError> {
        self.inner
            .write_all(data)
            .await
            .map_err(map_underlay_err)
    }

    pub async fn read_at_least(&mut self, min_bytes: usize) -> Result<Vec<u8>, MeshRoleError> {
        self.inner
            .read_at_least(min_bytes)
            .await
            .map_err(map_underlay_err)
    }

    pub async fn close(self) -> Result<(), MeshRoleError> {
        self.inner.close().await.map_err(map_underlay_err)
    }
}

/// HQ DialOnly: tip-5 underlay dial to node `--bridge-underlay-listen`, then
/// tip-6 `BridgeUnderlayClient::open` (real crates API — not cite/stub).
///
/// `peer` is `host:port` of the node underlay UDP bind. `transport_key` is
/// 32 bytes (AEAD). `underlay_token` is the underlay auth barrier (distinct
/// from application `GATE4AGENT_BRIDGE_TOKEN`).
#[cfg(target_os = "linux")]
pub async fn dial_bridge_reach(
    peer: &str,
    transport_key: [u8; 32],
    underlay_token: &str,
) -> Result<BridgeReachClient, MeshRoleError> {
    use gate4agent_node_wire::mesh_underlay::{
        dial_peer, BridgeUnderlayClient, MeshUnderlayRole, UnderlayAuthToken, UnderlayTransportKey,
    };

    // Doctrine: HQ dials; never accept.
    assert_underlay_direction_allowed(
        MeshParticipantRole::HqClientAdmin,
        MeshTcpDialDirection::Dial,
    )?;

    let transport = UnderlayTransportKey::from_bytes(transport_key);
    let auth = UnderlayAuthToken::new(underlay_token).map_err(map_underlay_err)?;
    let session = dial_peer(
        MeshUnderlayRole::HqClientAdmin,
        &transport,
        &auth,
        peer,
    )
    .await
    .map_err(map_underlay_err)?;
    let inner = BridgeUnderlayClient::open(session, underlay_token)
        .await
        .map_err(map_underlay_err)?;
    Ok(BridgeReachClient { inner })
}

/// Non-Linux: tip-6 dial refused with platform error.
#[cfg(not(target_os = "linux"))]
pub async fn dial_bridge_reach(
    _peer: &str,
    _transport_key: [u8; 32],
    _underlay_token: &str,
) -> Result<(), MeshRoleError> {
    assert_underlay_direction_allowed(
        MeshParticipantRole::HqClientAdmin,
        MeshTcpDialDirection::Dial,
    )?;
    Err(MeshRoleError::PlatformUnsupported)
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh_role::MeshParticipantRole;

    #[test]
    fn wireguard_product_refused() {
        assert_eq!(
            open_wireguard_daemon_stub(),
            Err(MeshRoleError::UnderlayNotImplemented)
        );
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn linux_c2_node_path_keeps_token_barrier() {
        let transport = TransportKey::from_bytes([4u8; 32]);
        let auth = AuthToken::new("hatchery-underlay-probe").unwrap();

        assert_eq!(
            accept_peer(MeshParticipantRole::HqClientAdmin, &transport, &auth)
                .await
                .err(),
            Some(MeshRoleError::HqMustNotAcceptUnderlay)
        );

        let listener = accept_peer(MeshParticipantRole::C2Peer, &transport, &auth)
            .await
            .expect("c2 accept");
        let addr = listener.local_addr().unwrap();

        let dial = {
            let transport = transport.clone();
            let auth = auth.clone();
            tokio::spawn(async move {
                dial_peer(MeshParticipantRole::NodePeer, &transport, &auth, addr).await
            })
        };

        let server = listener.accept().await.expect("accept session");
        let mut client = dial.await.expect("join").expect("dial");

        client
            .send_encrypted(b"probe-plane-ping")
            .await
            .expect("enc send");
        assert_eq!(
            server.recv_encrypted().await.expect("enc recv"),
            b"probe-plane-ping"
        );

        assert_eq!(client.probe(None), Err(MeshRoleError::Unauthorized));
        assert_eq!(
            client.probe(Some("wrong")),
            Err(MeshRoleError::Unauthorized)
        );
        client
            .probe(Some("hatchery-underlay-probe"))
            .expect("authorized probe");
    }

    #[cfg(not(target_os = "linux"))]
    #[tokio::test]
    async fn non_linux_refuses_accept_with_platform_error() {
        let err = accept_peer(MeshParticipantRole::C2Peer, &[0u8; 32], "t")
            .await
            .unwrap_err();
        assert_eq!(err, MeshRoleError::PlatformUnsupported);
    }

    #[test]
    fn tip6_health_flag_still_node_side_after_real_wire() {
        use crate::mesh_role::{
            may_host_bridge_underlay_accept, BridgeReachCite, MeshParticipantRole,
            TIP6_BRIDGE_REACH_HEALTH,
        };
        assert!(may_host_bridge_underlay_accept(MeshParticipantRole::NodePeer));
        assert!(!may_host_bridge_underlay_accept(MeshParticipantRole::HqClientAdmin));
        assert_eq!(
            BridgeReachCite::node().tip6_health_flag(),
            Some(TIP6_BRIDGE_REACH_HEALTH)
        );
        assert_eq!(BridgeReachCite::hq().tip6_health_flag(), None);
    }
}
