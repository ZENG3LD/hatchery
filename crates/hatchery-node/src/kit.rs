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
        let mail: fn() -> Result<mail4agent::MailboxJoinHandle, mail4agent::MainError> =
            mail4agent::spawn;
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
pub fn start_mailbox() -> Result<mail4agent::MailboxJoinHandle, mail4agent::MainError> {
    mail4agent::spawn()
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
        let start: fn() -> Result<mail4agent::MailboxJoinHandle, mail4agent::MainError> =
            start_mailbox;
        assert!(start as usize != 0);
    }
}
