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

/// Oldest courier version the node treats as current (the linked mail4agent).
pub const COURIER_MIN_VERSION: &str = "0.4.17";

/// Decide [`CourierPlan`] for `addr` via [`crate::local_infra::probe`].
pub fn plan_courier(addr: std::net::SocketAddr, probe_timeout: std::time::Duration) -> CourierPlan {
    match crate::local_infra::probe(addr, "mail4agent", probe_timeout) {
        crate::local_infra::Probe::Free => CourierPlan::Own(addr),
        crate::local_infra::Probe::Ours(info) => CourierPlan::Attach { addr, version: info.version },
        crate::local_infra::Probe::Foreign(detail) => CourierPlan::Foreign { addr, detail },
    }
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
            let _ = version;
            let crate::local_infra::Probe::Ours(info) = crate::local_infra::probe(addr, "mail4agent", std::time::Duration::from_secs(2)) else {
                return None;
            };
            match crate::local_infra::upgrade_plan(&info, COURIER_MIN_VERSION, false) {
                crate::local_infra::Upgrade::NotNeeded => None,
                crate::local_infra::Upgrade::ManualOnly => {
                    tracing::warn!(%addr, minimum = COURIER_MIN_VERSION, "running courier is older and has no control channel; kept as is");
                    None
                }
                crate::local_infra::Upgrade::Handoff if !crate::local_infra::auto_upgrade_allowed() => {
                    tracing::warn!(%addr, "running courier is older; HATCHERY_LOCAL_UPGRADE=off, kept as is");
                    None
                }
                crate::local_infra::Upgrade::Handoff => {
                    match crate::local_infra::handoff(addr, &info, 300, std::time::Duration::from_secs(15)) {
                        Ok(()) => {
                            tracing::info!(%addr, "older courier handed over; starting ours");
                            start_own_mailbox(addr)
                        }
                        Err(error) => {
                            tracing::warn!(%addr, %error, "courier handoff failed; keeping the running one");
                            None
                        }
                    }
                }
            }
        }
        CourierPlan::Foreign { addr, detail } => {
            tracing::warn!(%addr, %detail, "courier port held by something that is not mail4agent; running without a mailbox (set --m4a-bind)");
            None
        }
        CourierPlan::Own(addr) => start_own_mailbox(addr),
    }
}

#[cfg(feature = "m4a")]
fn start_own_mailbox(addr: std::net::SocketAddr) -> Option<MailboxHandle> {
    {
        {
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
            supervise_mailbox(addr, mailbox);
            None
        }
    }
}

/// The node supervises its own courier: if the mailbox thread ends (crash,
/// or a handoff to a successor), restart it with backoff once the port is
/// free again; if someone else now serves a newer courier there, attach.
#[cfg(feature = "m4a")]
fn supervise_mailbox(addr: std::net::SocketAddr, first: MailboxHandle) {
    let _ = std::thread::Builder::new().name("mail4agent-supervisor".to_owned()).spawn(move || {
        let mut current = first;
        let mut backoff = std::time::Duration::from_secs(1);
        loop {
            let result = current.join();
            tracing::warn!(%addr, ?result, "node-owned mailbox ended; supervising");
            loop {
                std::thread::sleep(backoff);
                backoff = (backoff * 2).min(std::time::Duration::from_secs(30));
                match plan_courier(addr, std::time::Duration::from_secs(2)) {
                    CourierPlan::Attach { version, .. } => {
                        tracing::info!(%addr, version = version.as_deref().unwrap_or("?"), "a courier took over the port; attached, supervisor stops");
                        return;
                    }
                    CourierPlan::Foreign { detail, .. } => {
                        tracing::warn!(%addr, %detail, "courier port held by something else; retrying");
                    }
                    CourierPlan::Own(_) => match start_mailbox() {
                        Ok(next) => {
                            std::thread::sleep(std::time::Duration::from_millis(750));
                            if next.is_finished() {
                                let _ = next.join();
                                continue;
                            }
                            tracing::info!(%addr, "node-owned mailbox restarted");
                            backoff = std::time::Duration::from_secs(1);
                            current = next;
                            break;
                        }
                        Err(error) => tracing::error!(%error, "mailbox restart failed"),
                    },
                }
            }
        }
    });
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
