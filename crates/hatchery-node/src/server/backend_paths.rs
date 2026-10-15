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
}

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
