//! Mesh dial-role types for Hatchery HQ (types + stub only).
//!
//! Locks owner doctrine in code **without** implementing a WireGuard / mesh
//! daemon and **without** making HQ a mesh server:
//!
//! - HQ = always client / admin → [`MeshDialCapability::DialOnly`]
//! - C2 + node = mesh peers → [`MeshDialCapability::DialOrAccept`] (NAT call-home)
//! - Transport crypto ≠ authorization (token barriers stay inside any future path)
//!
//! Cite:
//! - `hq-dials-c2-mesh-role-inventory-2026-10-02.md`
//! - `mesh-connectivity-daemon-design-2026-10-02.md` §1.5
//! - `oss-perimeter-mesh-roles-and-versioning-2026-10-02.md` §1

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

/// Error when a call asks HQ (or DialOnly) to accept underlay peers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeshRoleError {
    /// HQ / DialOnly must not accept mesh underlay peers.
    HqMustNotAcceptUnderlay,
    /// Stub underlay has no real tunnel yet.
    UnderlayNotImplemented,
}

impl fmt::Display for MeshRoleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::HqMustNotAcceptUnderlay => {
                f.write_str("HQ mesh role is dial-only; underlay accept is refused")
            }
            Self::UnderlayNotImplemented => {
                f.write_str("mesh underlay stub: no WireGuard/daemon in this tip")
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
}

/// Hatchery HQ underlay view: dial-only, no daemon.
#[derive(Debug, Default, Clone, Copy)]
pub struct HqMeshDialStub;

impl MeshUnderlayDial for HqMeshDialStub {
    fn role(&self) -> MeshParticipantRole {
        MeshParticipantRole::HQ
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
}
