//! Isolation defaults per node kind and the egress kill-switch ruleset.
//!
//! The ruleset is rendered from data and only text leaves this module: the node (or an
//! operator following the text instructions) feeds it to `nft -f -` or `iptables-restore`
//! inside the isolated space. Nothing here touches the host it is compiled on.

use crate::NodeKind;
use serde::{Deserialize, Serialize};
use std::net::{IpAddr, SocketAddr};

/// Where a node keeps its secrets.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyStore {
    /// A 0600 state directory owned by the node's own user (the default everywhere).
    StateDir,
    /// Local kind only: the operator keeps the admin key in the OS keyring or on a hardware key.
    OsKeyringOrHardware,
}

/// How the node's tunnel to C2 is managed.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TunnelMode {
    /// The node creates and owns kernel WireGuard (needs CAP_NET_ADMIN).
    NodeManaged,
    /// An OS-level WireGuard client already exists; the node runs with `--no-wg`.
    External,
}

/// What a kind runs with unless the operator overrides it.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct IsolationProfile {
    pub kind: NodeKind,
    /// Egress is dropped unless it goes through the tunnel (or is the tunnel's own UDP).
    pub kill_switch: bool,
    /// Provider sessions run as an unprivileged user; the node keeps the keys.
    pub agents_unprivileged: bool,
    /// The provisioning share (keys, env file) is unmounted before any agent starts.
    pub drop_provisioning_share: bool,
    pub tunnel: TunnelMode,
    pub key_store: KeyStore,
}

impl IsolationProfile {
    pub fn for_kind(kind: NodeKind) -> Self {
        match kind {
            // Light by default: a person's own machine. Everything below is opt-in.
            NodeKind::Local => Self {
                kind,
                kill_switch: false,
                agents_unprivileged: false,
                drop_provisioning_share: false,
                tunnel: TunnelMode::External,
                key_store: KeyStore::StateDir,
            },
            NodeKind::Docker => Self {
                kind,
                kill_switch: true,
                agents_unprivileged: true,
                drop_provisioning_share: false,
                tunnel: TunnelMode::NodeManaged,
                key_store: KeyStore::StateDir,
            },
            // The primary isolated space: identity lives inside, nothing leaks around the tunnel.
            NodeKind::Qemu => Self {
                kind,
                kill_switch: true,
                agents_unprivileged: true,
                drop_provisioning_share: true,
                tunnel: TunnelMode::NodeManaged,
                key_store: KeyStore::StateDir,
            },
            // Somebody else's machine / VPS: unprivileged agents, network policy is the host's.
            NodeKind::Remote => Self {
                kind,
                kill_switch: false,
                agents_unprivileged: true,
                drop_provisioning_share: false,
                tunnel: TunnelMode::NodeManaged,
                key_store: KeyStore::StateDir,
            },
        }
    }

    /// Operator overrides from the environment-style flags: `Some` replaces the default.
    pub fn with_overrides(
        mut self,
        kill_switch: Option<bool>,
        agents_unprivileged: Option<bool>,
    ) -> Self {
        if let Some(value) = kill_switch {
            self.kill_switch = value;
        }
        if let Some(value) = agents_unprivileged {
            self.agents_unprivileged = value;
        }
        self
    }
}

#[derive(Debug, thiserror::Error, Eq, PartialEq)]
pub enum KillSwitchError {
    #[error("interface name must be 1-15 characters of [A-Za-z0-9_.-]")]
    InvalidInterface,
    #[error("tunnel endpoint must be an IP:PORT literal (a hostname cannot be allowed without a resolver leak)")]
    EndpointNotLiteral,
}

/// Egress kill-switch: everything is dropped except loopback, the tunnel interface, and the
/// tunnel's own UDP to the C2 endpoint on the underlay. Rules match interface *names*, so
/// deleting the tunnel interface (VPN down) leaves traffic dropped instead of rerouted.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KillSwitch {
    interface: String,
    endpoint: SocketAddr,
}

impl KillSwitch {
    pub fn new(interface: &str, endpoint: &str) -> Result<Self, KillSwitchError> {
        let valid = !interface.is_empty()
            && interface.len() <= 15
            && interface
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'));
        if !valid {
            return Err(KillSwitchError::InvalidInterface);
        }
        let endpoint: SocketAddr = endpoint
            .parse()
            .map_err(|_| KillSwitchError::EndpointNotLiteral)?;
        Ok(Self {
            interface: interface.to_owned(),
            endpoint,
        })
    }

    pub fn interface(&self) -> &str {
        &self.interface
    }

    pub fn endpoint(&self) -> SocketAddr {
        self.endpoint
    }

    pub const TABLE: &'static str = "hatchery_killswitch";

    /// nftables ruleset (`nft -f -`). The final `counter drop` rules make leaks measurable.
    pub fn nft_ruleset(&self) -> String {
        let (family, ip) = match self.endpoint.ip() {
            IpAddr::V4(ip) => ("ip", ip.to_string()),
            IpAddr::V6(ip) => ("ip6", ip.to_string()),
        };
        let port = self.endpoint.port();
        let wg = &self.interface;
        let table = Self::TABLE;
        format!(
            "table inet {table} {{\n\
             \x20 chain output {{\n\
             \x20   type filter hook output priority 0; policy drop;\n\
             \x20   oif \"lo\" accept\n\
             \x20   oifname \"{wg}\" accept\n\
             \x20   {family} daddr {ip} udp dport {port} accept\n\
             \x20   counter drop\n\
             \x20 }}\n\
             \x20 chain input {{\n\
             \x20   type filter hook input priority 0; policy drop;\n\
             \x20   iif \"lo\" accept\n\
             \x20   iifname \"{wg}\" accept\n\
             \x20   {family} saddr {ip} udp sport {port} accept\n\
             \x20   counter drop\n\
             \x20 }}\n\
             \x20 chain forward {{\n\
             \x20   type filter hook forward priority 0; policy drop;\n\
             \x20 }}\n\
             }}\n"
        )
    }

    /// The same policy for hosts that only have iptables (`iptables-restore`, IPv4 endpoint only).
    pub fn iptables_restore(&self) -> Option<String> {
        let IpAddr::V4(ip) = self.endpoint.ip() else {
            return None;
        };
        let port = self.endpoint.port();
        let wg = &self.interface;
        Some(format!(
            "*filter\n:INPUT DROP [0:0]\n:FORWARD DROP [0:0]\n:OUTPUT DROP [0:0]\n\
             -A OUTPUT -o lo -j ACCEPT\n-A OUTPUT -o {wg} -j ACCEPT\n\
             -A OUTPUT -d {ip}/32 -p udp --dport {port} -j ACCEPT\n\
             -A INPUT -i lo -j ACCEPT\n-A INPUT -i {wg} -j ACCEPT\n\
             -A INPUT -s {ip}/32 -p udp --sport {port} -j ACCEPT\nCOMMIT\n"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_default_to_the_isolation_the_plan_names() {
        let local = IsolationProfile::for_kind(NodeKind::Local);
        assert!(
            !local.kill_switch
                && !local.agents_unprivileged
                && local.tunnel == TunnelMode::External
        );
        let qemu = IsolationProfile::for_kind(NodeKind::Qemu);
        assert!(qemu.kill_switch && qemu.agents_unprivileged && qemu.drop_provisioning_share);
        let docker = IsolationProfile::for_kind(NodeKind::Docker);
        assert!(
            docker.kill_switch && docker.agents_unprivileged && !docker.drop_provisioning_share
        );
        let remote = IsolationProfile::for_kind(NodeKind::Remote);
        assert!(!remote.kill_switch && remote.agents_unprivileged);
        let relaxed = qemu.with_overrides(Some(false), None);
        assert!(!relaxed.kill_switch && relaxed.agents_unprivileged);
    }

    #[test]
    fn ruleset_drops_by_default_and_allows_only_loopback_tunnel_and_its_udp() {
        let ks = KillSwitch::new("hatch0", "10.0.2.2:51820").unwrap();
        let text = ks.nft_ruleset();
        assert!(text.contains("policy drop;"));
        assert_eq!(text.matches("policy drop;").count(), 3);
        assert!(text.contains("ip daddr 10.0.2.2 udp dport 51820 accept"));
        assert!(text.contains("oifname \"hatch0\" accept"));
        assert!(text.contains("counter drop"));
        // no catch-all accept
        assert!(!text.lines().any(|l| l.trim() == "accept"));
    }

    #[test]
    fn hostnames_and_odd_interface_names_are_refused() {
        assert_eq!(
            KillSwitch::new("hatch0", "c2.example:51820"),
            Err(KillSwitchError::EndpointNotLiteral)
        );
        assert_eq!(
            KillSwitch::new("a b", "10.0.0.1:1"),
            Err(KillSwitchError::InvalidInterface)
        );
        assert_eq!(
            KillSwitch::new("wg0\"; accept; \"", "10.0.0.1:1"),
            Err(KillSwitchError::InvalidInterface)
        );
        assert_eq!(
            KillSwitch::new("", "10.0.0.1:1"),
            Err(KillSwitchError::InvalidInterface)
        );
        assert_eq!(
            KillSwitch::new("0123456789abcdef", "10.0.0.1:1"),
            Err(KillSwitchError::InvalidInterface)
        );
    }

    #[test]
    fn ipv6_endpoint_uses_ip6_and_iptables_variant_is_v4_only() {
        let v6 = KillSwitch::new("wg0", "[fd00::1]:51820").unwrap();
        assert!(v6
            .nft_ruleset()
            .contains("ip6 daddr fd00::1 udp dport 51820 accept"));
        assert!(v6.iptables_restore().is_none());
        let v4 = KillSwitch::new("wg0", "192.0.2.1:51820").unwrap();
        let ipt = v4.iptables_restore().unwrap();
        assert!(
            ipt.contains(":OUTPUT DROP")
                && ipt.contains("-d 192.0.2.1/32 -p udp --dport 51820 -j ACCEPT")
        );
    }
}
