//! Status -> capability policy, shared by C2 (enforcement) and hatchery-server (grants).
//!
//! The defaults are the built-in table; an operator can replace the three configurable rows
//! (neutral, member, trusted) with a JSON file. `suspended` and `revoked` are always empty and
//! cannot be configured. Unknown fields and unknown capability names are rejected so a typo
//! never silently widens or narrows access.

use serde::{Deserialize, Serialize};

use crate::NodeStatus;

/// Capabilities the policy can grant. `heartbeat` = may call home and be seen alive,
/// `attach` = operator may attach sessions and see inventory, `tasks`/`tools` = task and tool
/// access (consumers check them through the C2 operator API), `mail`/`m4a` = messaging.
pub const KNOWN_CAPABILITIES: &[&str] = &["heartbeat", "attach", "mail", "m4a", "tasks", "tools"];

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Grants {
    pub mailbox: bool,
    pub rooms: Vec<String>,
    pub capabilities: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Policy {
    pub neutral: Grants,
    pub member: Grants,
    pub trusted: Grants,
}

#[derive(Debug, thiserror::Error, Eq, PartialEq)]
pub enum PolicyError {
    #[error("policy file is not valid: {0}")]
    Parse(String),
    #[error("unknown capability '{0}' (known: heartbeat, attach, mail, m4a, tasks, tools)")]
    UnknownCapability(String),
}

fn list(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            neutral: Grants { mailbox: false, rooms: list(&["lobby"]), capabilities: list(&["heartbeat"]) },
            member: Grants {
                mailbox: true,
                rooms: list(&["lobby", "general"]),
                capabilities: list(&["heartbeat", "attach", "mail", "m4a"]),
            },
            trusted: Grants {
                mailbox: true,
                rooms: list(&["lobby", "general", "ops"]),
                capabilities: list(&["heartbeat", "attach", "mail", "m4a", "tasks", "tools"]),
            },
        }
    }
}

impl Policy {
    pub fn from_json(text: &str) -> Result<Self, PolicyError> {
        let policy: Self = serde_json::from_str(text).map_err(|e| PolicyError::Parse(e.to_string()))?;
        policy.validate()?;
        Ok(policy)
    }

    pub fn load(path: &std::path::Path) -> Result<Self, PolicyError> {
        let text = std::fs::read_to_string(path).map_err(|e| PolicyError::Parse(e.to_string()))?;
        Self::from_json(&text)
    }

    pub fn validate(&self) -> Result<(), PolicyError> {
        for grants in [&self.neutral, &self.member, &self.trusted] {
            for cap in &grants.capabilities {
                if !KNOWN_CAPABILITIES.contains(&cap.as_str()) {
                    return Err(PolicyError::UnknownCapability(cap.clone()));
                }
            }
        }
        Ok(())
    }

    pub fn grants(&self, status: NodeStatus) -> Grants {
        match status {
            NodeStatus::Neutral => self.neutral.clone(),
            NodeStatus::Member => self.member.clone(),
            NodeStatus::Trusted => self.trusted.clone(),
            NodeStatus::Suspended | NodeStatus::Revoked => Grants::default(),
        }
    }

    pub fn allows(&self, status: NodeStatus, capability: &str) -> bool {
        self.grants(status).capabilities.iter().any(|c| c == capability)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_grow_with_status_and_out_statuses_are_empty() {
        let p = Policy::default();
        assert!(p.allows(NodeStatus::Neutral, "heartbeat") && !p.allows(NodeStatus::Neutral, "attach"));
        assert!(p.allows(NodeStatus::Member, "attach") && !p.allows(NodeStatus::Member, "tasks"));
        assert!(p.allows(NodeStatus::Trusted, "tasks") && p.allows(NodeStatus::Trusted, "tools"));
        for s in [NodeStatus::Suspended, NodeStatus::Revoked] {
            assert_eq!(p.grants(s), Grants::default());
            assert!(!p.allows(s, "heartbeat"));
        }
    }

    #[test]
    fn file_overrides_rows_and_keeps_defaults_for_the_rest() {
        let p = Policy::from_json(r#"{"member":{"mailbox":true,"rooms":["general"],"capabilities":["heartbeat","mail"]}}"#).unwrap();
        assert!(!p.allows(NodeStatus::Member, "attach"));
        assert_eq!(p.trusted, Policy::default().trusted);
    }

    #[test]
    fn typos_and_unknown_capabilities_are_rejected() {
        assert!(matches!(Policy::from_json(r#"{"membr":{}}"#), Err(PolicyError::Parse(_))));
        assert_eq!(
            Policy::from_json(r#"{"neutral":{"capabilities":["hearbeat"]}}"#),
            Err(PolicyError::UnknownCapability("hearbeat".into()))
        );
        // suspended and revoked cannot be configured
        assert!(matches!(Policy::from_json(r#"{"revoked":{"capabilities":["attach"]}}"#), Err(PolicyError::Parse(_))));
    }
}
