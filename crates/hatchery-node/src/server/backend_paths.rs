//! Per-provider backend path table and selector (owner rule 2026-10-16).
//!
//! Every interaction with a provider backend can go through PTY (the vendor CLI in a
//! terminal: launch flags, typed input, slash commands, screen detection) or ACP (the
//! provider's Agent Client Protocol server: JSON-RPC session/new, session/prompt,
//! session/set_config_option, session/load, ...). Neither is an invariant. The node
//! knows, per provider, which path supports which action and picks the more beneficial
//! one for the task; when the requested path cannot do an action, the selector says so
//! (and names the other path) instead of silently dropping it.

use gate4agent_node_protocol::SessionMode;

/// One interaction with a provider backend.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BackendAction {
    Spawn,
    Model,
    Effort,
    Prompt,
    Resume,
    Wake,
    Status,
    Stop,
    /// Interrupt the current turn without ending the session.
    Interrupt,
    /// List the provider's own sessions (for resume pickers).
    ListSessions,
    /// Ready / limit / login state of the session.
    Ready,
    Limit,
    Login,
    /// Answer a tool permission request.
    ToolPermission,
    /// Attach MCP servers to the session.
    McpAttach,
    /// Switch the session mode (approval / sandbox / plan).
    ModeSwitch,
}

/// All actions, for tables and docs.
pub const ALL_ACTIONS: &[BackendAction] = &[
    BackendAction::Spawn,
    BackendAction::Model,
    BackendAction::Effort,
    BackendAction::Prompt,
    BackendAction::Resume,
    BackendAction::Wake,
    BackendAction::Status,
    BackendAction::Stop,
    BackendAction::Interrupt,
    BackendAction::ListSessions,
    BackendAction::Ready,
    BackendAction::Limit,
    BackendAction::Login,
    BackendAction::ToolPermission,
    BackendAction::McpAttach,
    BackendAction::ModeSwitch,
];

/// How well one path serves one action.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Support {
    /// Native, structured (flag at launch, or an ACP method).
    Native,
    /// Works, but through a weaker channel (typed slash command, screen heuristics).
    Degraded,
    /// Not available on this path.
    None,
}

/// Provider -> (PTY support, ACP support) for an action. Sources: g4a adapters 0.5.4
/// session-option catalogs and resume plans, vendor `--help` (claude 2.x, codex, grok
/// 1.0.46, kimi-code 2.1.1), ACP spec (session/set_config_option categories `model` and
/// `thought_level`, session/set_model, session/load).
pub fn support(provider: &str, action: BackendAction) -> (Support, Support) {
    use BackendAction::*;
    use Support::*;
    let p = normalize(provider);
    match (p, action) {
        (_, Spawn) => (Native, Native),
        // Other ACP-first agents (gemini --experimental-acp, opencode acp, ...).
        ("gemini" | "opencode", Model | Resume | McpAttach | ModeSwitch) => (Degraded, Native),
        ("gemini" | "opencode", Effort | ListSessions) => (None, Degraded),
        // cursor-agent: PTY only here (arms first so generic rows do not claim ACP).
        ("cursor", Model | Resume | ModeSwitch | McpAttach) => (Native, None),
        ("cursor", _) => (Degraded, None),
        // PTY: launch flags (`--model`, `-m`). ACP: config option `model` / session/set_model.
        ("claude" | "codex" | "grok" | "kimi", Model) => (Native, Native),
        // PTY: claude `--effort`, codex `-c model_reasoning_effort=`, grok `--reasoning-effort`.
        // kimi has no effort launch flag; ACP thought_level option if the agent advertises it.
        ("claude" | "codex" | "grok", Effort) => (Native, Native),
        ("kimi", Effort) => (None, Native),
        // PTY: typed into the composer (bracketed paste). ACP: session/prompt.
        (_, Prompt) => (Degraded, Native),
        // PTY: claude/grok `--resume <id>`, codex `resume <id>`, kimi `--session <id>`.
        // ACP: session/load with the provider session id.
        ("claude" | "codex" | "grok" | "kimi", Resume) => (Native, Native),
        // Wake = a short line into the PTY vs a session/prompt turn.
        (_, Wake) => (Degraded, Native),
        // PTY: screen detector (limit/login/ready heuristics). ACP: typed errors / usage updates.
        (_, Status) => (Degraded, Native),
        (_, Stop) => (Native, Native),
        // PTY: Esc / Ctrl-C into the composer. ACP: session/cancel.
        (_, Interrupt) => (Degraded, Native),
        // PTY: the node reads the provider's on-disk session store (claude projects, codex
        // sessions) or a picker; ACP: session/list where the agent advertises it.
        ("claude" | "codex", ListSessions) => (Native, Degraded),
        ("grok" | "kimi", ListSessions) => (Degraded, Degraded),
        // Ready: PTY screen detector vs ACP initialize/session/new answer.
        (_, Ready) => (Degraded, Native),
        // Limits: PTY banner heuristics vs typed ACP error (still provider text inside).
        (_, Limit) => (Degraded, Degraded),
        // Login: PTY screen detector vs ACP authMethods + authenticate.
        (_, Login) => (Degraded, Native),
        // PTY: approval prompt on screen answered by keystroke. ACP: session/request_permission.
        (_, ToolPermission) => (Degraded, Native),
        // PTY: launch config (claude --mcp-config, codex -c mcp_servers.*, kimi --mcp-config-file;
        // grok via its config file). ACP: session/new mcpServers.
        ("claude" | "codex" | "kimi", McpAttach) => (Native, Native),
        ("grok", McpAttach) => (Degraded, Native),
        // PTY: launch flag (claude --permission-mode, codex --sandbox/--ask-for-approval,
        // kimi --yolo) or a runtime keystroke/slash command. ACP: session/set_mode.
        ("claude" | "codex" | "kimi", ModeSwitch) => (Native, Native),
        ("grok", ModeSwitch) => (Degraded, Native),
        _ => (None, None),
    }
}

fn normalize(provider: &str) -> &str {
    match provider {
        "claude-code" => "claude",
        other => other,
    }
}

fn on(mode: SessionMode, s: (Support, Support)) -> Support {
    match mode {
        SessionMode::Acp => s.1,
        _ => s.0,
    }
}

/// How the node applies launch options for a spawn on a given transport.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LaunchOptionsPlan {
    /// Nothing to apply.
    None,
    /// Vendor CLI flags at spawn (g4a session-option catalog maps them).
    PtyFlags,
    /// Spawn plain, then apply through ACP config options once the agent advertises them.
    AcpAfterSpawn,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PathRefusal {
    pub message: String,
}

/// Pick how model/effort are applied for `mode`. Refuses (naming the other path) when the
/// chosen transport cannot carry an option, rather than dropping it.
pub fn plan_launch_options(
    provider: &str,
    mode: SessionMode,
    model: bool,
    effort: bool,
) -> Result<LaunchOptionsPlan, PathRefusal> {
    if !model && !effort {
        return Ok(LaunchOptionsPlan::None);
    }
    for (wanted, action, name) in [(model, BackendAction::Model, "model"), (effort, BackendAction::Effort, "effort")] {
        if !wanted {
            continue;
        }
        if on(mode, support(provider, action)) == Support::None {
            let other = match mode {
                SessionMode::Acp => "pty",
                _ => "acp",
            };
            let other_mode = if other == "acp" { SessionMode::Acp } else { SessionMode::Pty };
            let hint = if on(other_mode, support(provider, action)) != Support::None {
                format!("; use the {other} path")
            } else {
                String::new()
            };
            return Err(PathRefusal {
                message: format!("{provider}: {name} is not settable over {}{hint}", match mode {
                    SessionMode::Acp => "acp",
                    _ => "pty",
                }),
            });
        }
    }
    Ok(match mode {
        SessionMode::Acp => LaunchOptionsPlan::AcpAfterSpawn,
        _ => LaunchOptionsPlan::PtyFlags,
    })
}

/// Whether `action` can run over `mode`; refuses (naming the other path when it can) otherwise.
pub fn plan_action(provider: &str, mode: SessionMode, action: BackendAction) -> Result<Support, PathRefusal> {
    let s = support(provider, action);
    let here = on(mode, s);
    if here != Support::None {
        return Ok(here);
    }
    let (name, other, other_s) = match mode {
        SessionMode::Acp => ("acp", "pty", s.0),
        _ => ("pty", "acp", s.1),
    };
    let hint = if other_s != Support::None { format!("; use the {other} path") } else { String::new() };
    Err(PathRefusal { message: format!("{provider}: {action:?} is not available over {name}{hint}") })
}

/// The better path for one action: native beats degraded; ties go to `prefer`.
pub fn best_path(provider: &str, action: BackendAction, prefer: SessionMode) -> Option<SessionMode> {
    let (pty, acp) = support(provider, action);
    let rank = |s: Support| match s {
        Support::Native => 2,
        Support::Degraded => 1,
        Support::None => 0,
    };
    match (rank(pty), rank(acp)) {
        (0, 0) => None,
        (p, a) if p > a => Some(SessionMode::Pty),
        (p, a) if a > p => Some(SessionMode::Acp),
        _ => Some(prefer),
    }
}

/// Prompts and data reach a session only through m4a (owner rule). A transport may carry a
/// prompt only as a wake (a short pointer to the m4a letter), never the content itself.
pub fn prompt_allowed_on_transport(is_wake: bool) -> bool {
    is_wake
}

/// Prefer a transport for a task when the caller has no hard requirement: an interactive
/// terminal needs PTY; headless work (one prompt, structured result, cheap start) prefers ACP.
pub fn preferred_mode(provider: &str, interactive: bool, needs_effort: bool) -> SessionMode {
    if interactive {
        // Fall to ACP only when PTY cannot carry a required option.
        if needs_effort && support(provider, BackendAction::Effort).0 == Support::None {
            return SessionMode::Acp;
        }
        return SessionMode::Pty;
    }
    SessionMode::Acp
}

/// ACP config writes for a pending launch selection, given the options the agent advertised
/// (`ProviderEvent::ConfigOptionsUpdated`, full set). Matches by ACP category (`model`,
/// `thought_level`), then by id. Returns (option_id, value_json) pairs and whether the model
/// still needs `session/set_model` (agent advertised no model config option).
pub fn acp_option_writes(
    advertised: &[(String, Option<String>, Vec<String>)],
    model: Option<&str>,
    effort: Option<&str>,
) -> (Vec<(String, String)>, bool, bool) {
    let find = |category: &str, ids: &[&str]| {
        advertised
            .iter()
            .find(|(_, c, _)| c.as_deref() == Some(category))
            .or_else(|| advertised.iter().find(|(id, _, _)| ids.contains(&id.as_str())))
    };
    let mut writes = Vec::new();
    let mut model_via_set_model = false;
    let mut effort_unapplied = false;
    if let Some(m) = model {
        match find("model", &["model"]) {
            Some((id, _, _)) => writes.push((id.clone(), json_str(m))),
            None => model_via_set_model = true,
        }
    }
    if let Some(e) = effort {
        match find("thought_level", &["effort", "reasoning_effort", "thought_level"]) {
            Some((id, _, choices)) if choices.is_empty() || choices.iter().any(|c| c == &json_str(e)) => {
                writes.push((id.clone(), json_str(e)))
            }
            _ => effort_unapplied = true,
        }
    }
    (writes, model_via_set_model, effort_unapplied)
}

fn json_str(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pty_flags_for_all_fleet_models_and_kimi_effort_routes_to_acp() {
        for p in ["claude", "codex", "grok", "kimi"] {
            assert_eq!(plan_launch_options(p, SessionMode::Pty, true, false), Ok(LaunchOptionsPlan::PtyFlags));
            assert_eq!(plan_launch_options(p, SessionMode::Acp, true, true), Ok(LaunchOptionsPlan::AcpAfterSpawn));
        }
        let refused = plan_launch_options("kimi", SessionMode::Pty, true, true).unwrap_err();
        assert!(refused.message.contains("use the acp path"), "{refused:?}");
        assert_eq!(preferred_mode("kimi", true, true), SessionMode::Acp);
        assert_eq!(preferred_mode("claude", true, true), SessionMode::Pty);
        assert_eq!(preferred_mode("codex", false, false), SessionMode::Acp);
    }

    #[test]
    fn acp_writes_match_by_category_then_id_and_report_gaps() {
        let adv = vec![
            ("model".to_owned(), Some("model".to_owned()), vec![]),
            ("reasoning".to_owned(), Some("thought_level".to_owned()), vec!["\"low\"".to_owned(), "\"high\"".to_owned()]),
        ];
        let (w, set_model, gap) = acp_option_writes(&adv, Some("opus"), Some("high"));
        assert_eq!(w, vec![("model".into(), "\"opus\"".into()), ("reasoning".into(), "\"high\"".into())]);
        assert!(!set_model && !gap);
        let (w, set_model, gap) = acp_option_writes(&[], Some("opus"), Some("max"));
        assert!(w.is_empty() && set_model && gap);
        let (_, _, gap) = acp_option_writes(&adv, None, Some("max"));
        assert!(gap, "unadvertised choice is reported, not forced");
    }
}

#[cfg(test)]
mod action_table_tests {
    use super::*;

    #[test]
    fn every_listed_provider_has_a_path_for_core_actions() {
        for p in ["claude", "codex", "grok", "kimi"] {
            for a in ALL_ACTIONS {
                assert!(best_path(p, *a, SessionMode::Pty).is_some(), "{p} {a:?}");
            }
        }
    }

    #[test]
    fn selector_picks_native_and_refuses_with_hint() {
        assert_eq!(best_path("claude", BackendAction::ToolPermission, SessionMode::Pty), Some(SessionMode::Acp));
        assert_eq!(best_path("codex", BackendAction::ListSessions, SessionMode::Acp), Some(SessionMode::Pty));
        assert_eq!(best_path("claude", BackendAction::Stop, SessionMode::Pty), Some(SessionMode::Pty));
        let e = plan_action("kimi", SessionMode::Pty, BackendAction::Effort).unwrap_err();
        assert!(e.message.contains("use the acp path"));
        assert!(plan_action("cursor", SessionMode::Acp, BackendAction::Interrupt).is_err());
        assert!(!prompt_allowed_on_transport(false));
    }
}
