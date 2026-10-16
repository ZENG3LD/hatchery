//! Letter -> node -> g4a wake -> session turn.
//!
//! m4a only hands a letter over (its `M4A_LETTER_HOOK`); the node knows which of its sessions
//! owns the addressed nick (it assigned that account at spawn) and asks g4a
//! (`gate4agent_wake::hosted`) what to do. A live session gets the framed prompt as its next
//! turn; the node's policy ([`crate::wake_pipeline`]) gates everything else: typing into a raw
//! terminal needs an explicit terminal grant, a nick bound to a routine gets one routine POST via
//! g4a, and anything else is refused (m4a keeps the letter in its durable inbox).
//! Callers present the node's wake token (a 0600 file next to the accounts root), so another
//! local user cannot type into a session through the door.

use std::collections::BTreeMap;
use std::sync::Arc;

use gate4agent_wake::hosted::{plan_hosted_wake, HostedSessions, HostedTarget, HostedWake};
use gate4agent_wake::{ProviderKind, WakeLetter};

use crate::wake_pipeline::{deliver_routine, launch_spawn_profile, route, Route, RoutineLetter, SessionShape};

use super::*;

/// One letter for a session of this node.
pub struct NodeLetter {
    pub nick: String,
    pub from: String,
    pub event_id: String,
    pub body: String,
}

#[derive(Clone)]
pub struct NodeWake {
    shared: Arc<NodeShared>,
    token: Arc<str>,
    policy: Arc<crate::wake_pipeline::WakePolicy>,
}

struct Snapshot(BTreeMap<String, HostedTarget>);

impl HostedSessions for Snapshot {
    fn lookup(&self, nick: &str) -> Option<HostedTarget> {
        self.0.get(nick).cloned()
    }
}

fn kind_of(agent: &AgentId) -> Option<ProviderKind> {
    ProviderKind::parse(crate::m4a_accounts::m4a_provider(agent.as_str())?)
}

impl NodeWake {
    pub(super) fn new(shared: Arc<NodeShared>, token: String, policy: Arc<crate::wake_pipeline::WakePolicy>) -> Self {
        Self { shared, token: token.into(), policy }
    }

    /// Constant-time check of the caller's token.
    pub fn authorized(&self, token: &str) -> bool {
        let (a, b) = (self.token.as_bytes(), token.as_bytes());
        a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
    }

    fn snapshot(&self) -> (Snapshot, BTreeMap<String, SessionAddress>) {
        let live = self.shared.m4a_live.lock().unwrap_or_else(|p| p.into_inner()).clone();
        let bindings = self.shared.session_bindings.lock().unwrap_or_else(|p| p.into_inner());
        let mut targets = BTreeMap::new();
        let mut addresses = BTreeMap::new();
        for (nick, (instance_id, agent)) in live {
            let Some(provider) = kind_of(&agent) else { continue };
            let binding = bindings.get(&instance_id);
            if let Some(b) = binding {
                addresses.insert(
                    nick.clone(),
                    SessionAddress { workspace_id: b.workspace_id.clone(), session: SessionKey { instance_id, generation: b.generation } },
                );
            }
            let spawn_allowed = self.policy.spawn_profile(&nick).is_some();
            targets.insert(nick.clone(), HostedTarget { nick, provider, live: binding.is_some(), spawn_allowed });
        }
        (Snapshot(targets), addresses)
    }

    /// Wakes the session that owns `letter.nick`. `Ok` names what happened; `Err` is safe to
    /// log (it never carries the letter text).
    pub async fn wake(&self, letter: NodeLetter) -> Result<&'static str, String> {
        let (sessions, addresses) = self.snapshot();
        let wake = WakeLetter { body: &letter.body, from_nick: &letter.from, event_id: &letter.event_id, room: None, reaction: None };
        match plan_hosted_wake(&sessions, &letter.nick, &wake) {
            HostedWake::Submit { nick, prompt } => {
                let prompt = if self.policy.deliver_body { prompt } else { crate::wake_pipeline::wake_line(&letter.from, &letter.event_id) };
                let address = addresses.get(&nick).cloned().ok_or("session vanished")?;
                let agent = self.shared.validate_address(&address).map_err(|_| "session is not live".to_owned())?;
                let refused = |e: NodeFailure| {
                    tracing::warn!(nick = %nick, code = ?e.code, "letter wake input refused");
                    "session did not take the turn".to_owned()
                };
                let shape = if self.shared.require_session_runtime_policy(&address, ProviderRuntimeRequirement::SemanticPrompt).is_ok() {
                    SessionShape::Semantic
                } else {
                    SessionShape::RawTerminal
                };
                match route(&self.policy, &nick, shape) {
                    Route::Deliver => {
                        let action = InputAction::SubmitPrompt(PromptPayload { text: prompt, framing: prompt_framing(&agent) });
                        self.shared.dispatch_input_bounded(&address, action).await.map_err(refused)?;
                    }
                    Route::TerminalTyping => {
                        // Terminal capability, granted explicitly for this nick: type the prompt
                        // as one line, then Enter.
                        let text = prompt.replace(['\n', '\r'], " ");
                        self.shared.dispatch_input_bounded(&address, InputAction::TerminalText(TerminalText { text })).await.map_err(refused)?;
                        self.shared.dispatch_input_bounded(&address, InputAction::TerminalControl(TerminalControl::ControlM)).await.map_err(refused)?;
                    }
                    Route::Refuse(why) => {
                        tracing::info!(nick = %nick, "letter not typed: no terminal grant");
                        return Err(why);
                    }
                    Route::Routine(_) => return Err("unexpected routine route for a live session".into()),
                    Route::Spawn(_) => return Err("unexpected spawn route for a live session".into()),
                }
                tracing::info!(nick = %nick, "letter woke a live session");
                Ok("submitted")
            }
            HostedWake::Spawn { nick, .. } => match self.policy.spawn_profile(&nick).cloned() {
                Some(profile) => {
                    tokio::task::spawn_blocking(move || launch_spawn_profile(&profile))
                        .await
                        .map_err(|_| "spawn profile task failed".to_owned())?
                        .map_err(|e| e)?;
                    tracing::info!(nick = %nick, "letter woke an offline session via spawn profile");
                    Ok("spawned")
                }
                None => Err("spawn on letter is not enabled on this node".into()),
            },
            HostedWake::Refuse(why) => match route(&self.policy, &letter.nick, SessionShape::None) {
                Route::Spawn(profile) => {
                    let nick = letter.nick.clone();
                    tokio::task::spawn_blocking(move || launch_spawn_profile(&profile))
                        .await
                        .map_err(|_| "spawn profile task failed".to_owned())?
                        .map_err(|e| e)?;
                    tracing::info!(nick = %nick, "letter woke an offline session via spawn profile");
                    Ok("spawned")
                }
                Route::Routine(target) => {
                    let routine_letter = RoutineLetter {
                        to: letter.nick.clone(),
                        from: letter.from.clone(),
                        event_id: letter.event_id.clone(),
                        subject: None,
                        room: None,
                        body: letter.body.clone(),
                        reply: None,
                        note: None,
                    };
                    let nick = letter.nick.clone();
                    match tokio::task::spawn_blocking(move || deliver_routine(&target, &routine_letter)).await {
                        Ok(Ok(_)) => {
                            tracing::info!(nick = %nick, "letter delivered to a routine");
                            Ok("routine")
                        }
                        Ok(Err(e)) => Err(e),
                        Err(_) => Err("routine delivery task failed".into()),
                    }
                }
                _ => Err(why),
            },
        }
    }
}
