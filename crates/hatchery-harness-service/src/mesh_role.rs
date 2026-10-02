//! Mesh dial-role types for Hatchery HQ (types + stub only).
//!
//! Locks owner doctrine in code **without** implementing a WireGuard / mesh
//! daemon and **without** making HQ a mesh server:
//!
//! - HQ = always client / admin → [`MeshDialCapability::DialOnly`] (HQ dials C2)
//! - C2 + node = mesh peers → [`MeshDialCapability::DialOrAccept`] (NAT call-home)
//! - Tip-6 **bridge reach** is **node-side** (g4a `--bridge-underlay-listen` /
//!   health `transport_udp_underlay=tip-6-bridge-reach`); HQ never hosts it
//! - Transport crypto ≠ authorization (token barriers stay inside any future path)
//! - Tip 5 live Linux UDP+AEAD permit path: [`crate::mesh_underlay`]
//!
//! Cite:
//! - `hq-dials-c2-mesh-role-inventory-2026-10-02.md`
//! - `mesh-connectivity-daemon-design-2026-10-02.md` §1.5 / tips 5–6
//! - `oss-perimeter-mesh-roles-and-versioning-2026-10-02.md` §1
//!
//! **Crates pin note:** hatchery stays on crates.io **0.4.4**. Tip-5/6
//! `gate4agent-node-wire::mesh_underlay` is **not** in that published tree.
//! This module cites tip-6 flags + role wiring only — **owner: bump when
//! hatchery needs tip5/6 APIs**.

use std::fmt;

/// Who participates on the future mesh / connectivity underlay.
///
/// Product C2 control today is separate framed wire; this enum is the
/// **role lock** for underlay dial policy, not a C2 RPC dialect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MeshParticipantRole {
    /// C2 relay — peer; may dial or accept under NAT call-home rules.
    C2Peer,
    /// Station node — peer; same dial/accept rule as C2.
    NodePeer,
    /// Hatchery HQ — **always** client/admin; never underlay accept.
    HqClientAdmin,
}

/// What a participant is allowed to do on the underlay TCP/UDP dial axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MeshDialCapability {
    /// Outbound only (HQ).
    DialOnly,
    /// Peer may dial **or** accept (C2 / node; NAT decides who opens).
    DialOrAccept,
}

/// TCP open direction for one hop (distinct from protocol client/server roles).
///
/// Existing `C2NodeRoute::CallHome` already flips TCP while keeping protocol
/// roles; this type names that axis for future underlay tips.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MeshTcpDialDirection {
    /// This side opens the connection.
    Dial,
    /// This side accepts an inbound connection.
    Accept,
}

impl MeshParticipantRole {
    /// Normative dial capability for this role.
    pub const fn dial_capability(self) -> MeshDialCapability {
        match self {
            Self::C2Peer | Self::NodePeer => MeshDialCapability::DialOrAccept,
            Self::HqClientAdmin => MeshDialCapability::DialOnly,
        }
    }

    /// Whether this role may ever be an underlay accept-peer.
    pub const fn may_accept_underlay(self) -> bool {
        matches!(
            self.dial_capability(),
            MeshDialCapability::DialOrAccept
        )
    }

    /// Fixed HQ role (compile-time constant for adapters).
    pub const HQ: Self = Self::HqClientAdmin;
}

impl MeshDialCapability {
    /// Whether [`MeshTcpDialDirection::Accept`] is allowed under this capability.
    pub const fn allows_accept(self) -> bool {
        matches!(self, Self::DialOrAccept)
    }
}

impl fmt::Display for MeshParticipantRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::C2Peer => "c2-peer",
            Self::NodePeer => "node-peer",
            Self::HqClientAdmin => "hq-client-admin",
        })
    }
}

impl fmt::Display for MeshDialCapability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::DialOnly => "dial-only",
            Self::DialOrAccept => "dial-or-accept",
        })
    }
}

impl fmt::Display for MeshTcpDialDirection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Dial => "dial",
            Self::Accept => "accept",
        })
    }
}

/// g4a tip-6 bridge health flag when `--bridge-underlay-listen` is active.
///
/// Cite-only string (node envelope). Hatchery does **not** host this path on
/// crates.io **0.4.4** — no `mesh_underlay::bridge_reach` in that publish.
pub const TIP6_BRIDGE_REACH_HEALTH: &str = "tip-6-bridge-reach";

/// g4a tip-6 health value when underlay listen is configured but not yet active.
pub const TIP6_BRIDGE_UNDERLAY_OPT_IN: &str = "opt-in-via-bridge-underlay-listen";

/// Whether this role may host tip-6 **bridge-over-underlay** accept.
///
/// Only the **node** envelope runs `--bridge-underlay-listen` (browser bridge
/// TCP relay over tip-5 underlay). HQ is DialOnly (dials C2). C2 is a mesh
/// peer for tip-5 underlay but does **not** host the tip-6 bridge door.
pub const fn may_host_bridge_underlay_accept(role: MeshParticipantRole) -> bool {
    matches!(role, MeshParticipantRole::NodePeer)
}

/// Thin tip-6 cite adapter: who hosts bridge reach vs who only dials C2.
///
/// No I/O. No WireGuard. No dependency on unpublished g4a tip-5/6 crates APIs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BridgeReachCite {
    role: MeshParticipantRole,
}

impl BridgeReachCite {
    pub const fn new(role: MeshParticipantRole) -> Self {
        Self { role }
    }

    pub const fn hq() -> Self {
        Self {
            role: MeshParticipantRole::HQ,
        }
    }

    pub const fn node() -> Self {
        Self {
            role: MeshParticipantRole::NodePeer,
        }
    }

    pub const fn role(self) -> MeshParticipantRole {
        self.role
    }

    /// HQ (and non-node peers) never host tip-6 bridge underlay accept.
    pub const fn hosts_bridge_underlay_accept(self) -> bool {
        may_host_bridge_underlay_accept(self.role)
    }

    /// Normative health flag label when this side would advertise tip-6 reach.
    ///
    /// Node → [`TIP6_BRIDGE_REACH_HEALTH`]; HQ → `None` (DialOnly toward C2).
    pub const fn tip6_health_flag(self) -> Option<&'static str> {
        if self.hosts_bridge_underlay_accept() {
            Some(TIP6_BRIDGE_REACH_HEALTH)
        } else {
            None
        }
    }

    /// Refuse HQ attempting to host bridge underlay (doctrine lock).
    pub fn assert_may_host_bridge_underlay(self) -> Result<(), MeshRoleError> {
        if self.hosts_bridge_underlay_accept() {
            Ok(())
        } else {
            Err(MeshRoleError::HqMustNotAcceptUnderlay)
        }
    }
}

/// Error when a call asks HQ (or DialOnly) to accept underlay peers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MeshRoleError {
    /// HQ / DialOnly must not accept mesh underlay peers.
    HqMustNotAcceptUnderlay,
    /// WireGuard / kernel TUN daemon product is not this tip.
    UnderlayNotImplemented,
    /// Win/mac (non-Linux) underlay refused with a clear operator hint.
    PlatformUnsupported,
    /// Probe / action refused — token barrier failed (crypto ≠ auth).
    Unauthorized,
}

impl fmt::Display for MeshRoleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::HqMustNotAcceptUnderlay => {
                f.write_str("HQ mesh role is dial-only; underlay accept is refused")
            }
            Self::UnderlayNotImplemented => {
                f.write_str(
                    "mesh underlay: no full WireGuard/daemon product in tips 5–6;                      hatchery lab = mesh_underlay; tip-6 bridge-reach = node-side g4a                      (not published on crates.io 0.4.4)",
                )
            }
            Self::PlatformUnsupported => {
                f.write_str("mesh underlay unsupported on this OS: Linux-first tip 5; Win/mac later")
            }
            Self::Unauthorized => {
                f.write_str("underlay probe unauthorized: token barrier failed (crypto ≠ auth)")
            }
        }
    }
}

impl std::error::Error for MeshRoleError {}

/// Refuse underlay accept when the role is dial-only (HQ).
pub fn assert_underlay_direction_allowed(
    role: MeshParticipantRole,
    direction: MeshTcpDialDirection,
) -> Result<(), MeshRoleError> {
    if matches!(direction, MeshTcpDialDirection::Accept) && !role.may_accept_underlay() {
        return Err(MeshRoleError::HqMustNotAcceptUnderlay);
    }
    Ok(())
}

/// Future connectivity underlay slice — **stub only**.
///
/// Implementations for HQ must report [`MeshParticipantRole::HqClientAdmin`]
/// and must not grow an accept-listener API that inverts “HQ dials C2.”
/// No TUN/WireGuard/UDP path is provided here.
pub trait MeshUnderlayDial {
    /// Doctrine role for this participant.
    fn role(&self) -> MeshParticipantRole;

    /// Capability derived from [`Self::role`].
    fn dial_capability(&self) -> MeshDialCapability {
        self.role().dial_capability()
    }

    /// Placeholder dial toward a peer endpoint label (no I/O).
    ///
    /// Real underlay tips will replace this; HQ stays dial-oriented.
    fn dial_stub(&self, _peer_label: &str) -> Result<(), MeshRoleError> {
        let _ = self.role();
        Err(MeshRoleError::UnderlayNotImplemented)
    }

    /// Accept path — default **refuses** for dial-only roles (HQ).
    fn accept_stub(&self) -> Result<(), MeshRoleError> {
        assert_underlay_direction_allowed(self.role(), MeshTcpDialDirection::Accept)?;
        Err(MeshRoleError::UnderlayNotImplemented)
    }

    /// Tip-6 bridge-reach cite for this participant (no I/O).
    ///
    /// Node may host; HQ DialOnly (dials C2). Real relay lives in g4a node
    /// after crates bump past 0.4.4.
    fn bridge_reach_cite(&self) -> BridgeReachCite {
        BridgeReachCite::new(self.role())
    }
}

/// Hatchery HQ underlay view: dial-only, no daemon.
#[derive(Debug, Default, Clone, Copy)]
pub struct HqMeshDialStub;

impl MeshUnderlayDial for HqMeshDialStub {
    fn role(&self) -> MeshParticipantRole {
        MeshParticipantRole::HQ
    }
}

/// C2 mesh peer stub — DialOrAccept (NAT call-home). Real path: `mesh_underlay`.
#[derive(Debug, Default, Clone, Copy)]
pub struct C2MeshDialStub;

impl MeshUnderlayDial for C2MeshDialStub {
    fn role(&self) -> MeshParticipantRole {
        MeshParticipantRole::C2Peer
    }
}

/// Node mesh peer stub — DialOrAccept. Tip-5 lab: `mesh_underlay`; tip-6 bridge-reach = node-side g4a.
#[derive(Debug, Default, Clone, Copy)]
pub struct NodeMeshDialStub;

impl MeshUnderlayDial for NodeMeshDialStub {
    fn role(&self) -> MeshParticipantRole {
        MeshParticipantRole::NodePeer
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hq_is_dial_only_and_must_not_accept() {
        let hq = MeshParticipantRole::HqClientAdmin;
        assert_eq!(hq.dial_capability(), MeshDialCapability::DialOnly);
        assert!(!hq.may_accept_underlay());
        assert_eq!(
            assert_underlay_direction_allowed(hq, MeshTcpDialDirection::Dial),
            Ok(())
        );
        assert_eq!(
            assert_underlay_direction_allowed(hq, MeshTcpDialDirection::Accept),
            Err(MeshRoleError::HqMustNotAcceptUnderlay)
        );
    }

    #[test]
    fn c2_and_node_peers_may_dial_or_accept() {
        for role in [MeshParticipantRole::C2Peer, MeshParticipantRole::NodePeer] {
            assert_eq!(role.dial_capability(), MeshDialCapability::DialOrAccept);
            assert!(role.may_accept_underlay());
            assert_eq!(
                assert_underlay_direction_allowed(role, MeshTcpDialDirection::Accept),
                Ok(())
            );
        }
    }

    #[test]
    fn hq_stub_trait_refuses_accept_and_unimplemented_dial() {
        let stub = HqMeshDialStub;
        assert_eq!(stub.role(), MeshParticipantRole::HqClientAdmin);
        assert_eq!(stub.dial_capability(), MeshDialCapability::DialOnly);
        assert_eq!(
            stub.dial_stub("c2-label"),
            Err(MeshRoleError::UnderlayNotImplemented)
        );
        assert_eq!(
            stub.accept_stub(),
            Err(MeshRoleError::HqMustNotAcceptUnderlay)
        );
    }

    #[test]
    fn display_labels_are_stable() {
        assert_eq!(MeshParticipantRole::HqClientAdmin.to_string(), "hq-client-admin");
        assert_eq!(MeshDialCapability::DialOnly.to_string(), "dial-only");
        assert_eq!(MeshTcpDialDirection::Dial.to_string(), "dial");
    }

    #[test]
    fn peer_stubs_may_accept_direction_but_trait_still_unimpl_until_mesh_underlay() {
        let c2 = C2MeshDialStub;
        let node = NodeMeshDialStub;
        assert_eq!(c2.dial_capability(), MeshDialCapability::DialOrAccept);
        assert_eq!(node.dial_capability(), MeshDialCapability::DialOrAccept);
        // Trait accept path: role allows accept, then UnderlayNotImplemented
        // (tip-5 live path lives in `crate::mesh_underlay`, not this stub trait).
        assert_eq!(c2.accept_stub(), Err(MeshRoleError::UnderlayNotImplemented));
        assert_eq!(node.accept_stub(), Err(MeshRoleError::UnderlayNotImplemented));
        assert_eq!(
            HqMeshDialStub.accept_stub(),
            Err(MeshRoleError::HqMustNotAcceptUnderlay)
        );
    }

    #[test]
    fn tip6_bridge_reach_is_node_side_hq_dials_c2_only() {
        assert!(may_host_bridge_underlay_accept(MeshParticipantRole::NodePeer));
        assert!(!may_host_bridge_underlay_accept(MeshParticipantRole::HqClientAdmin));
        assert!(!may_host_bridge_underlay_accept(MeshParticipantRole::C2Peer));

        let hq = BridgeReachCite::hq();
        assert_eq!(hq.tip6_health_flag(), None);
        assert_eq!(
            hq.assert_may_host_bridge_underlay(),
            Err(MeshRoleError::HqMustNotAcceptUnderlay)
        );
        assert_eq!(
            HqMeshDialStub.bridge_reach_cite().tip6_health_flag(),
            None
        );

        let node = BridgeReachCite::node();
        assert_eq!(node.tip6_health_flag(), Some(TIP6_BRIDGE_REACH_HEALTH));
        assert_eq!(node.assert_may_host_bridge_underlay(), Ok(()));
        assert_eq!(
            NodeMeshDialStub.bridge_reach_cite().tip6_health_flag(),
            Some(TIP6_BRIDGE_REACH_HEALTH)
        );
        assert_eq!(TIP6_BRIDGE_REACH_HEALTH, "tip-6-bridge-reach");
        assert_eq!(
            TIP6_BRIDGE_UNDERLAY_OPT_IN,
            "opt-in-via-bridge-underlay-listen"
        );
    }
}
