//! HQ messenger dial: hatchery consumes [`mail4agent`](https://github.com/ZENG3LD/mail4agent)
//! as a **client dependency** — the harness holds no mailbox of its own.
//!
//! Cite: Claude extraction plan
//! `docs/gate4agent/plans/mailbox-service-extraction-and-signed-session-identity-2026-09-16.md`
//! (libraries publishable; daemon is the service; consumers link `mail4agent-client`).
//! §11 / CLAUDE.md: agent mail is `mail4agent` on `:18301`.
//!
//! This module is a thin adapter over [`mail4agent_client::MailClient`] for
//! health + send/list smoke from the harness side. Full messenger product
//! surface (TUI, grant wiring) is later; S2 only needs the dial proven.

use std::fmt;
use std::path::Path;
use std::time::Duration;

use mail4agent_api::{
    Address, Directory, InboxPage, InboxRequest, ParticipantId, SendRequest, SendResponse,
    INBOX_LIMIT_DEFAULT,
};
use mail4agent_client::{
    ClientError, HealthReport, MailClient, RegisterParticipantRequest, SecretResponse,
    WhoAmIResponse, DEFAULT_BASE_URL,
};

/// Default local mail4agent bind (daemon default / hatchery CLAUDE.md).
pub const HATCHERY_MAIL_DEFAULT_URL: &str = DEFAULT_BASE_URL;

/// How hatchery dials a local mail4agent for HQ messenger use.
#[derive(Clone)]
pub struct MailDial {
    client: MailClient,
}

impl fmt::Debug for MailDial {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MailDial")
            .field("base", self.client.base_url())
            .finish()
    }
}

impl MailDial {
    /// Dial `base_url` (loopback) with the given bearer. Refuses non-loopback
    /// URLs — same discipline as [`MailClient::new`].
    pub fn connect(base_url: impl AsRef<str>, bearer: impl Into<String>) -> Result<Self, MailDialError> {
        let client = MailClient::builder(base_url.as_ref())
            .map_err(MailDialError::Client)?
            .bearer(bearer)
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(MailDialError::Client)?;
        Ok(Self { client })
    }

    /// Read a bootstrap / operator secret from `path` (one line, trimmed) and
    /// dial. Never logs the secret.
    pub fn connect_with_key_file(
        base_url: impl AsRef<str>,
        key_path: impl AsRef<Path>,
    ) -> Result<Self, MailDialError> {
        let raw = std::fs::read_to_string(key_path.as_ref()).map_err(|e| MailDialError::KeyFile {
            path: key_path.as_ref().display().to_string(),
            source: e,
        })?;
        let bearer = raw.trim();
        if bearer.is_empty() {
            return Err(MailDialError::EmptyKeyFile {
                path: key_path.as_ref().display().to_string(),
            });
        }
        Self::connect(base_url, bearer)
    }

    pub fn client(&self) -> &MailClient {
        &self.client
    }

    pub async fn health(&self) -> Result<HealthReport, MailDialError> {
        self.client.health().await.map_err(MailDialError::Client)
    }

    /// Unauthenticated health against an arbitrary loopback base (no dial needed).
    pub async fn health_at(base_url: impl AsRef<str>) -> Result<HealthReport, MailDialError> {
        MailClient::health_at(base_url.as_ref())
            .await
            .map_err(MailDialError::Client)
    }

    pub async fn whoami(&self) -> Result<WhoAmIResponse, MailDialError> {
        self.client.whoami().await.map_err(MailDialError::Client)
    }

    pub async fn directory(&self) -> Result<Directory, MailDialError> {
        self.client.directory().await.map_err(MailDialError::Client)
    }

    pub async fn list_inbox(&self) -> Result<InboxPage, MailDialError> {
        self.client
            .inbox(InboxRequest {
                since_unix_ms: None,
                limit: INBOX_LIMIT_DEFAULT,
                wait_secs: None,
            })
            .await
            .map_err(MailDialError::Client)
    }

    pub async fn send_direct(
        &self,
        to: ParticipantId,
        subject: impl Into<String>,
        body: impl Into<String>,
    ) -> Result<SendResponse, MailDialError> {
        self.client
            .send(SendRequest {
                to: Address::Direct { participant: to },
                subject: subject.into(),
                body: body.into(),
                reply_to: None,
                correlation: None,
                refs: Vec::new(),
                idempotency_key: None,
            })
            .await
            .map_err(MailDialError::Client)
    }

    /// Operator helper: register a send/read participant for smoke peers.
    pub async fn register_peer(
        &self,
        id: ParticipantId,
        label: Option<String>,
    ) -> Result<SecretResponse, MailDialError> {
        self.client
            .admin_register_participant(RegisterParticipantRequest {
                id,
                label,
                may_send: true,
                may_read: true,
                operator: false,
            })
            .await
            .map_err(MailDialError::Client)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum MailDialError {
    #[error(transparent)]
    Client(ClientError),
    #[error("read mail key file {path}: {source}")]
    KeyFile {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("mail key file {path} is empty")]
    EmptyKeyFile { path: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_url_is_loopback_18301() {
        assert!(HATCHERY_MAIL_DEFAULT_URL.contains("127.0.0.1:18301"));
    }

    #[test]
    fn connect_refuses_empty_bearer() {
        let err = MailDial::connect(HATCHERY_MAIL_DEFAULT_URL, "").expect_err("empty");
        assert!(matches!(err, MailDialError::Client(_)));
    }
}
