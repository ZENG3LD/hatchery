//! Discovery of local infrastructure the node can reuse instead of starting
//! its own copy: a mail4agent courier, another gate4agent node.
//!
//! Rule: an existing process is never killed or relaunched just because it
//! exists. The node probes the loopback address it would bind, reads the
//! service's `GET /health` (service name, version, optional capabilities)
//! and then owns, attaches to, or steps around it. Upgrading an older
//! instance is a graceful handoff through that service's control endpoint
//! (see the `local-infra-reuse` spec); this module only reports the need.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::time::Duration;

/// What answers on a loopback address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Probe {
    /// Nothing holds the port (or it is ephemeral): we may bind it.
    Free,
    /// The expected service answers.
    Ours(ServiceInfo),
    /// Something else holds the port.
    Foreign(String),
}

/// What a reused service reported about itself.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ServiceInfo {
    pub service: String,
    pub version: Option<String>,
    pub capabilities: Vec<String>,
    pub pid: Option<u64>,
}

impl ServiceInfo {
    /// True when the reported version is known and older than `minimum`.
    pub fn older_than(&self, minimum: &str) -> bool {
        match (self.version.as_deref().and_then(parse_version), parse_version(minimum)) {
            (Some(found), Some(min)) => found < min,
            _ => false,
        }
    }

    /// Graceful control verbs the service advertises (`control.*` capabilities).
    pub fn supports(&self, capability: &str) -> bool {
        self.capabilities.iter().any(|c| c == capability)
    }
}

/// `MAJOR.MINOR.PATCH` (pre-release / build suffix ignored).
pub fn parse_version(raw: &str) -> Option<(u64, u64, u64)> {
    let core = raw.trim().trim_start_matches('v').split(['-', '+']).next()?;
    let mut parts = core.split('.').map(|p| p.parse::<u64>().ok());
    Some((parts.next()??, parts.next().flatten().unwrap_or(0), parts.next().flatten().unwrap_or(0)))
}

/// Probe `addr` for `service` (the `service` field its `/health` returns).
pub fn probe(addr: SocketAddr, service: &str, timeout: Duration) -> Probe {
    if addr.port() == 0 {
        return Probe::Free;
    }
    match TcpListener::bind(addr) {
        Ok(listener) => {
            drop(listener);
            return Probe::Free;
        }
        Err(error) if error.kind() != std::io::ErrorKind::AddrInUse => {
            return Probe::Foreign(format!("cannot bind: {error}"));
        }
        Err(_) => {}
    }
    match read_health(addr, timeout) {
        Ok(json) => {
            let name = json.get("service").and_then(|v| v.as_str()).unwrap_or("");
            if name != service {
                return Probe::Foreign(format!("health names {name:?}, not {service:?}"));
            }
            Probe::Ours(ServiceInfo {
                service: name.to_owned(),
                version: json.get("version").and_then(|v| v.as_str()).map(str::to_owned),
                capabilities: json
                    .get("capabilities")
                    .and_then(|v| v.as_array())
                    .map(|a| a.iter().filter_map(|c| c.as_str().map(str::to_owned)).collect())
                    .unwrap_or_default(),
                pid: json.get("pid").and_then(|v| v.as_u64()),
            })
        }
        Err(detail) => Probe::Foreign(detail),
    }
}

/// Minimal HTTP/1.1 `GET /health` (unauthenticated in both services).
fn read_health(addr: SocketAddr, timeout: Duration) -> Result<serde_json::Value, String> {
    let mut stream = TcpStream::connect_timeout(&addr, timeout).map_err(|e| format!("connect: {e}"))?;
    let _ = stream.set_read_timeout(Some(timeout));
    let _ = stream.set_write_timeout(Some(timeout));
    stream
        .write_all(format!("GET /health HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n").as_bytes())
        .map_err(|e| format!("write: {e}"))?;
    let mut raw = Vec::new();
    let _ = (&mut stream).take(64 * 1024).read_to_end(&mut raw);
    let text = String::from_utf8_lossy(&raw);
    let (head, body) = text.split_once("\r\n\r\n").ok_or_else(|| "no HTTP response".to_owned())?;
    let status = head.lines().next().unwrap_or("");
    if !(status.starts_with("HTTP/1.1 200") || status.starts_with("HTTP/1.0 200")) {
        return Err(format!("health status: {status}"));
    }
    let start = body.find('{').ok_or_else(|| "health body is not JSON".to_owned())?;
    let end = body.rfind('}').ok_or_else(|| "health body is not JSON".to_owned())?;
    serde_json::from_str(&body[start..=end]).map_err(|e| format!("health JSON: {e}"))
}

/// Control verbs a reusable service may advertise in `capabilities`
/// (spec `local-infra-reuse`). Not implemented by the services yet.
pub mod control {
    pub const DRAIN: &str = "control.drain";
    pub const RELOAD: &str = "control.reload";
    pub const HANDOFF: &str = "control.handoff";
}

/// Upgrade decision for a reused service. Stub: the handoff itself is
/// spec-only until the service exposes `control.handoff`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Upgrade {
    NotNeeded,
    /// Older, and it offers graceful handoff: the node may run it.
    Handoff,
    /// Older, no control endpoint: keep using it, tell the operator.
    ManualOnly,
}

pub fn upgrade_plan(info: &ServiceInfo, minimum: &str) -> Upgrade {
    if !info.older_than(minimum) {
        Upgrade::NotNeeded
    } else if info.supports(control::HANDOFF) {
        Upgrade::Handoff
    } else {
        Upgrade::ManualOnly
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn serve_once(body: &'static str, status: &'static str) -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            if let Ok((mut s, _)) = listener.accept() {
                let mut buf = [0u8; 1024];
                let _ = s.read(&mut buf);
                let _ = write!(s, "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}", body.len());
            }
        });
        addr
    }

    const T: Duration = Duration::from_secs(2);

    #[test]
    fn free_and_zero_ports() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        drop(l);
        assert_eq!(probe(addr, "svc", T), Probe::Free);
        assert_eq!(probe("127.0.0.1:0".parse().unwrap(), "svc", T), Probe::Free);
    }

    #[test]
    fn ours_reports_version_and_capabilities() {
        let addr = serve_once(r#"{"ok":true,"service":"svc","version":"1.2.3","pid":7,"capabilities":["control.drain"]}"#, "200 OK");
        let Probe::Ours(info) = probe(addr, "svc", T) else { panic!("not ours") };
        assert_eq!(info.version.as_deref(), Some("1.2.3"));
        assert_eq!(info.pid, Some(7));
        assert!(info.supports(control::DRAIN));
        assert!(!info.supports(control::HANDOFF));
    }

    #[test]
    fn other_service_or_status_is_foreign() {
        let addr = serve_once(r#"{"service":"other"}"#, "200 OK");
        assert!(matches!(probe(addr, "svc", T), Probe::Foreign(_)));
        let addr = serve_once(r#"{"service":"svc"}"#, "500 Internal Server Error");
        assert!(matches!(probe(addr, "svc", T), Probe::Foreign(_)));
    }

    #[test]
    fn silent_socket_is_foreign() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        assert!(matches!(probe(addr, "svc", Duration::from_millis(300)), Probe::Foreign(_)));
    }

    #[test]
    fn version_and_upgrade_plan() {
        assert_eq!(parse_version("0.4.14"), Some((0, 4, 14)));
        assert_eq!(parse_version("v1.2-rc1"), Some((1, 2, 0)));
        let old = ServiceInfo { version: Some("0.4.9".into()), ..Default::default() };
        assert_eq!(upgrade_plan(&old, "0.4.14"), Upgrade::ManualOnly);
        let old_ctl = ServiceInfo { version: Some("0.4.9".into()), capabilities: vec![control::HANDOFF.into()], ..Default::default() };
        assert_eq!(upgrade_plan(&old_ctl, "0.4.14"), Upgrade::Handoff);
        let new = ServiceInfo { version: Some("0.5.0".into()), ..Default::default() };
        assert_eq!(upgrade_plan(&new, "0.4.14"), Upgrade::NotNeeded);
        let unknown = ServiceInfo::default();
        assert_eq!(upgrade_plan(&unknown, "0.4.14"), Upgrade::NotNeeded);
    }
}
