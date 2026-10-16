//! Standard node kit cores: dig2browser (`d2b`), mail4agent (`m4a`), the four
//! session-restore crates (`session-restore`) and kernel WireGuard
//! (`wireguard`). Each core is compiled only with its own feature; the default
//! build (`kit`) enables all of them.

use std::sync::OnceLock;

/// Names of the cores linked into this build, in owner order.
pub fn linked_core_names() -> &'static [&'static str] {
    static NAMES: OnceLock<Vec<&'static str>> = OnceLock::new();
    NAMES.get_or_init(|| {
        let mut names: Vec<&'static str> = Vec::new();
        #[cfg(feature = "d2b")]
        names.push("dig2browser");
        #[cfg(feature = "m4a")]
        names.push("mail4agent");
        #[cfg(feature = "session-restore")]
        names.push("session-restore");
        #[cfg(feature = "wireguard")]
        names.push("wireguard");
        names
    })
}

/// Touches a real symbol in each enabled core so the node binary links them.
/// Does not start a browser, a provider CLI, or a tunnel, and does not read
/// a secret. The mailbox is started separately by [`start_mailbox`].
pub fn force_link() -> usize {
    let mut acc: usize = 0;
    #[cfg(feature = "d2b")]
    {
        acc = acc.wrapping_add(dig2browser::browser_stream::LOCAL_PAGE_HTML.len());
    }
    #[cfg(feature = "m4a")]
    {
        let mail: fn() -> Result<(), mail4agent::MainError> = mail4agent::run;
        acc = acc.wrapping_add(mail as usize);
    }
    #[cfg(feature = "session-restore")]
    {
        let claude = std::mem::size_of::<claude_session_restore::transcript::SessionEvent>();
        let codex = codex_session_restore::MAX_HEAD_BYTES;
        let grok: fn() = grok_session_restore::main;
        let kimi: fn() = kimi_session_restore::main;
        acc = acc
            .wrapping_add(claude)
            .wrapping_add(codex)
            .wrapping_add(grok as usize)
            .wrapping_add(kimi as usize);
    }
    #[cfg(feature = "wireguard")]
    {
        let wireguard: fn(
            &crate::kit_wireguard::NodeWgPeerConfig,
        ) -> Result<std::net::IpAddr, crate::kit_wireguard::NodeWgPeerError> =
            crate::kit_wireguard::bring_up_node_wireguard;
        acc = acc
            .wrapping_add(wireguard as usize)
            .wrapping_add(crate::kit_wireguard::LINK_TAG.len());
    }
    acc
}

/// Start the mail4agent mailbox on a background thread (`m4a` only).
/// Same daemon the standalone `mail4agent` binary runs.
#[cfg(feature = "m4a")]
///
/// The daemon's own failure (for example a bootstrap refusal) is logged and
/// visible through [`MailboxHandle::is_finished`]; it is never swallowed.
pub fn start_mailbox() -> Result<MailboxHandle, mail4agent::MainError> {
    let join = std::thread::Builder::new()
        .name("mail4agent".to_owned())
        .spawn(|| {
            let result = mail4agent::run();
            if let Err(error) = &result {
                tracing::error!(%error, "mail4agent mailbox stopped");
            }
            result
        })
        .map_err(mail4agent::MainError::Runtime)?;
    Ok(MailboxHandle { join })
}

/// Env var mail4agent reads for its bind address (also set by `--m4a-bind`).
pub const COURIER_BIND_ENV: &str = "MAIL4AGENT_BIND";
/// mail4agent's default bind.
pub const COURIER_DEFAULT_BIND: &str = "127.0.0.1:18301";

/// What the node does about the courier port at start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CourierPlan {
    /// The port is free (or ephemeral): run our own mailbox there.
    Own(std::net::SocketAddr),
    /// A mail4agent courier already answers there: use it, start none.
    Attach { addr: std::net::SocketAddr, version: Option<String> },
    /// Something else holds the port: run without a mailbox.
    Foreign { addr: std::net::SocketAddr, detail: String },
}

/// The configured courier address (`MAIL4AGENT_BIND`, else the default).
pub fn courier_bind() -> Result<std::net::SocketAddr, String> {
    let raw = std::env::var(COURIER_BIND_ENV).unwrap_or_else(|_| COURIER_DEFAULT_BIND.to_owned());
    raw.parse().map_err(|error| format!("{COURIER_BIND_ENV}={raw:?} is invalid: {error}"))
}

/// Decide [`CourierPlan`] for `addr`: free port -> own; held -> `GET /health`
/// must answer `"service":"mail4agent"` to attach, anything else is foreign.
pub fn plan_courier(addr: std::net::SocketAddr, probe_timeout: std::time::Duration) -> CourierPlan {
    if addr.port() == 0 {
        return CourierPlan::Own(addr);
    }
    match std::net::TcpListener::bind(addr) {
        Ok(listener) => {
            drop(listener);
            return CourierPlan::Own(addr);
        }
        Err(error) if error.kind() != std::io::ErrorKind::AddrInUse => {
            return CourierPlan::Foreign { addr, detail: format!("cannot bind: {error}") };
        }
        Err(_) => {}
    }
    match probe_courier_health(addr, probe_timeout) {
        Ok(version) => CourierPlan::Attach { addr, version },
        Err(detail) => CourierPlan::Foreign { addr, detail },
    }
}

/// Minimal HTTP/1.1 `GET /health` against a loopback courier.
fn probe_courier_health(
    addr: std::net::SocketAddr,
    timeout: std::time::Duration,
) -> Result<Option<String>, String> {
    use std::io::{Read, Write};
    let mut stream = std::net::TcpStream::connect_timeout(&addr, timeout).map_err(|e| format!("connect: {e}"))?;
    let _ = stream.set_read_timeout(Some(timeout));
    let _ = stream.set_write_timeout(Some(timeout));
    stream
        .write_all(format!("GET /health HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n").as_bytes())
        .map_err(|e| format!("write: {e}"))?;
    let mut raw = Vec::new();
    let _ = stream.take(64 * 1024).read_to_end(&mut raw);
    let text = String::from_utf8_lossy(&raw);
    let (head, body) = text.split_once("\r\n\r\n").ok_or_else(|| "no HTTP response".to_owned())?;
    if !head.starts_with("HTTP/1.1 200") && !head.starts_with("HTTP/1.0 200") {
        return Err(format!("health status: {}", head.lines().next().unwrap_or("")));
    }
    let start = body.find('{').ok_or_else(|| "health body is not JSON".to_owned())?;
    let json: serde_json::Value = serde_json::from_str(body[start..].trim_end_matches(|c: char| c != '}'))
        .map_err(|e| format!("health JSON: {e}"))?;
    if json.get("service").and_then(|v| v.as_str()) != Some("mail4agent") {
        return Err("health does not name mail4agent".to_owned());
    }
    Ok(json.get("version").and_then(|v| v.as_str()).map(str::to_owned))
}

#[cfg(feature = "m4a")]
/// Start our mailbox, attach to a running courier, or run without one.
/// Never exits the process. Returns the handle when our own mailbox runs.
pub fn start_or_attach_mailbox() -> Option<MailboxHandle> {
    let addr = match courier_bind() {
        Ok(addr) => addr,
        Err(error) => {
            tracing::error!(%error, "mail4agent courier address; running without a mailbox");
            return None;
        }
    };
    match plan_courier(addr, std::time::Duration::from_secs(2)) {
        CourierPlan::Attach { addr, version } => {
            tracing::info!(%addr, version = version.as_deref().unwrap_or("?"), "mail4agent courier already running; attached, not starting our own");
            None
        }
        CourierPlan::Foreign { addr, detail } => {
            tracing::warn!(%addr, %detail, "courier port held by something that is not mail4agent; running without a mailbox (set --m4a-bind)");
            None
        }
        CourierPlan::Own(addr) => {
            let mailbox = match start_mailbox() {
                Ok(mailbox) => mailbox,
                Err(error) => {
                    tracing::error!(%error, "mail4agent mailbox did not start; node continues without it");
                    return None;
                }
            };
            std::thread::sleep(std::time::Duration::from_millis(750));
            if mailbox.is_finished() {
                let error = mailbox.join().err().map_or_else(|| "mailbox exited".to_owned(), |e| e.to_string());
                tracing::error!(%addr, %error, "mail4agent mailbox stopped at start; node continues without it");
                return None;
            }
            tracing::info!(%addr, "node kit mailbox started");
            Some(mailbox)
        }
    }
}

/// Handle of the background mailbox thread.
#[cfg(feature = "m4a")]
pub struct MailboxHandle {
    join: std::thread::JoinHandle<Result<(), mail4agent::MainError>>,
}

#[cfg(feature = "m4a")]
impl MailboxHandle {
    /// True once the mailbox thread has returned (it only returns on failure).
    pub fn is_finished(&self) -> bool {
        self.join.is_finished()
    }

    /// Block until the mailbox thread ends and return its result.
    pub fn join(self) -> Result<(), mail4agent::MainError> {
        self.join.join().unwrap_or_else(|_| {
            Err(mail4agent::MainError::Runtime(std::io::Error::other("mail4agent thread panicked")))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(all(feature = "d2b", feature = "m4a", feature = "session-restore", feature = "wireguard"))]
    #[test]
    fn kit_names_the_four_cores() {
        assert_eq!(
            linked_core_names(),
            ["dig2browser", "mail4agent", "session-restore", "wireguard"]
        );
        assert!(force_link() > 0);
    }

    #[test]
    fn linked_names_match_enabled_features() {
        let expected = cfg!(feature = "d2b") as usize
            + cfg!(feature = "m4a") as usize
            + cfg!(feature = "session-restore") as usize
            + cfg!(feature = "wireguard") as usize;
        assert_eq!(linked_core_names().len(), expected);
    }

    #[cfg(feature = "m4a")]
    #[test]
    fn kit_exposes_mailbox_automaton_entry() {
        let start: fn() -> Result<MailboxHandle, mail4agent::MainError> = start_mailbox;
        assert!(start as usize != 0);
    }

    fn serve_once(body: &'static str, status: &'static str) -> std::net::SocketAddr {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
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

    #[test]
    fn courier_free_port_is_own() {
        let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = probe.local_addr().unwrap();
        drop(probe);
        assert_eq!(plan_courier(addr, std::time::Duration::from_secs(1)), CourierPlan::Own(addr));
    }

    #[test]
    fn courier_port_zero_is_own() {
        let addr: std::net::SocketAddr = "127.0.0.1:0".parse().unwrap();
        assert_eq!(plan_courier(addr, std::time::Duration::from_secs(1)), CourierPlan::Own(addr));
    }

    #[test]
    fn courier_held_by_mail4agent_attaches() {
        let addr = serve_once(r#"{"ok":true,"service":"mail4agent","version":"9.9.9"}"#, "200 OK");
        assert_eq!(
            plan_courier(addr, std::time::Duration::from_secs(2)),
            CourierPlan::Attach { addr, version: Some("9.9.9".to_owned()) }
        );
    }

    #[test]
    fn courier_held_by_other_service_is_foreign() {
        let addr = serve_once(r#"{"ok":true,"service":"something-else"}"#, "200 OK");
        assert!(matches!(plan_courier(addr, std::time::Duration::from_secs(2)), CourierPlan::Foreign { .. }));
    }

    #[test]
    fn courier_held_by_silent_socket_is_foreign() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        assert!(matches!(plan_courier(addr, std::time::Duration::from_millis(300)), CourierPlan::Foreign { .. }));
        drop(listener);
    }
}
