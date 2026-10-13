//! WireGuard binding rules for the restreamer (A2).
//!
//! Production: both the node uplink and the core drain listeners bind to the
//! C2 tunnel address (never `0.0.0.0`). Kernel WireGuard provides the outer
//! transport layer; the multiplex and nested crypto sit inside.
//!
//! Loopback binds remain allowed for local tests. [`RestreamerWgBind::production`]
//! rejects loopback and unspecified addresses.

use std::net::{IpAddr, SocketAddr};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RestreamerWgBind {
    /// C2 address inside the WireGuard tunnel.
    pub c2_tunnel_ip: IpAddr,
    /// Node multiplex uplink (`c2_tunnel_ip:port`).
    pub node_listen: SocketAddr,
    /// Core drain listen (`c2_tunnel_ip:port`), distinct from node_listen.
    pub drain_listen: SocketAddr,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WgBindError {
    Unspecified,
    PortZero,
    NotOnTunnel { got: IpAddr, expected: IpAddr },
    SamePort,
    LoopbackInProduction,
}

impl std::fmt::Display for WgBindError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unspecified => write!(f, "restreamer must not bind 0.0.0.0"),
            Self::PortZero => write!(f, "restreamer ports must be nonzero"),
            Self::NotOnTunnel { got, expected } => {
                write!(f, "listen ip {got} is not the C2 tunnel ip {expected}")
            }
            Self::SamePort => write!(f, "node and drain listen ports must differ"),
            Self::LoopbackInProduction => {
                write!(f, "production restreamer binds must use the WG tunnel address")
            }
        }
    }
}

impl std::error::Error for WgBindError {}

impl RestreamerWgBind {
    /// Validate a production bind: both listeners on `c2_tunnel_ip`, nonzero
    /// distinct ports, not loopback, not unspecified.
    pub fn production(
        c2_tunnel_ip: IpAddr,
        node_port: u16,
        drain_port: u16,
    ) -> Result<Self, WgBindError> {
        if c2_tunnel_ip.is_unspecified() {
            return Err(WgBindError::Unspecified);
        }
        if c2_tunnel_ip.is_loopback() {
            return Err(WgBindError::LoopbackInProduction);
        }
        if node_port == 0 || drain_port == 0 {
            return Err(WgBindError::PortZero);
        }
        if node_port == drain_port {
            return Err(WgBindError::SamePort);
        }
        let node_listen = SocketAddr::new(c2_tunnel_ip, node_port);
        let drain_listen = SocketAddr::new(c2_tunnel_ip, drain_port);
        Self::validate(c2_tunnel_ip, node_listen, drain_listen, true)
    }

    /// Validate listeners share the tunnel IP (loopback allowed when `require_tunnel` is false).
    pub fn validate(
        c2_tunnel_ip: IpAddr,
        node_listen: SocketAddr,
        drain_listen: SocketAddr,
        require_tunnel: bool,
    ) -> Result<Self, WgBindError> {
        if node_listen.ip().is_unspecified() || drain_listen.ip().is_unspecified() {
            return Err(WgBindError::Unspecified);
        }
        if node_listen.port() == 0 || drain_listen.port() == 0 {
            return Err(WgBindError::PortZero);
        }
        if node_listen.port() == drain_listen.port() && node_listen.ip() == drain_listen.ip() {
            return Err(WgBindError::SamePort);
        }
        if require_tunnel {
            if c2_tunnel_ip.is_loopback() {
                return Err(WgBindError::LoopbackInProduction);
            }
            if node_listen.ip() != c2_tunnel_ip {
                return Err(WgBindError::NotOnTunnel {
                    got: node_listen.ip(),
                    expected: c2_tunnel_ip,
                });
            }
            if drain_listen.ip() != c2_tunnel_ip {
                return Err(WgBindError::NotOnTunnel {
                    got: drain_listen.ip(),
                    expected: c2_tunnel_ip,
                });
            }
        }
        Ok(Self {
            c2_tunnel_ip,
            node_listen,
            drain_listen,
        })
    }

    /// Address core dials for the drain stream (over WG).
    pub fn drain_addr(&self) -> SocketAddr {
        self.drain_listen
    }

    /// Address nodes dial for the multiplex uplink (over WG).
    pub fn node_addr(&self) -> SocketAddr {
        self.node_listen
    }
}

/// Core-side dial target once HQ's WireGuard tunnel is up.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CoreDrainTarget {
    pub c2_tunnel_ip: IpAddr,
    pub drain_port: u16,
}

impl CoreDrainTarget {
    pub fn new(c2_tunnel_ip: IpAddr, drain_port: u16) -> Result<Self, WgBindError> {
        if c2_tunnel_ip.is_unspecified() || drain_port == 0 {
            return Err(WgBindError::PortZero);
        }
        if c2_tunnel_ip.is_loopback() {
            // Allowed for tests; production HQ uses the tunnel IP from enroll.
        }
        Ok(Self {
            c2_tunnel_ip,
            drain_port,
        })
    }

    pub fn socket_addr(&self) -> SocketAddr {
        SocketAddr::new(self.c2_tunnel_ip, self.drain_port)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[test]
    fn production_bind_on_tunnel_ip() {
        let ip = IpAddr::V4(Ipv4Addr::new(10, 88, 1, 1));
        let bind = RestreamerWgBind::production(ip, 19001, 19002).unwrap();
        assert_eq!(bind.node_addr().ip(), ip);
        assert_eq!(bind.drain_addr().port(), 19002);
    }

    #[test]
    fn production_rejects_loopback_and_same_port() {
        let lo = IpAddr::V4(Ipv4Addr::LOCALHOST);
        assert!(matches!(
            RestreamerWgBind::production(lo, 1, 2),
            Err(WgBindError::LoopbackInProduction)
        ));
        let ip = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1));
        assert!(matches!(
            RestreamerWgBind::production(ip, 9, 9),
            Err(WgBindError::SamePort)
        ));
    }

    #[test]
    fn listeners_must_use_tunnel_ip() {
        let tunnel = IpAddr::V4(Ipv4Addr::new(10, 88, 1, 1));
        let other = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(10, 88, 1, 2)), 19001);
        let drain = SocketAddr::new(tunnel, 19002);
        assert!(matches!(
            RestreamerWgBind::validate(tunnel, other, drain, true),
            Err(WgBindError::NotOnTunnel { .. })
        ));
    }

    #[test]
    fn core_drain_target_addr() {
        let t = CoreDrainTarget::new(IpAddr::V4(Ipv4Addr::new(10, 88, 1, 1)), 19002).unwrap();
        assert_eq!(t.socket_addr().to_string(), "10.88.1.1:19002");
    }
}
