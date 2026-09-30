//! Projection of a gate4agent node's own event stream onto the observation
//! vocabulary.
//!
//! The node publishes `NodeEvent::Control` (session lifecycle and provider
//! events) and the agent stream; it names no observation type. This module is
//! the harness side of that boundary: it derives `ObservationV1` telemetry from
//! those events, deterministically, so the same node cursor observed twice --
//! directly and through C2, or replayed on resync -- projects to equal
//! observations.
//!
//! Three inputs feed it:
//! - a control event ([`control_event_observations`]) -- lifecycle, turns,
//!   tools, usage, plans, subagents, interactions, host-request audit;
//! - an agent-stream `Blocked` chunk ([`blocked_chunk_observation`]) -- the
//!   blocked-action fact, computed once by the node and only mapped here;
//! - a record history summary ([`history_summary_observations`]) -- the counts
//!   the node publishes when a client previews a session record.

use gate4agent_node_protocol::correlation::{
    interaction_correlation, process_correlation, subagent_correlation, tool_class_label,
    tool_correlation,
};
use gate4agent_node_protocol::{
    AgentStreamChunkKindV1, AgentStreamChunkV1, BlockAuthorityV1 as NodeBlockAuthorityV1,
    SessionHistorySummaryV1,
};
use gate4agent_types::{
    AdapterFamily, ControlEvent, ControlEventKind,
    HostDecisionAuthority as ProviderHostDecisionAuthority,
    HostRequestDecision as ProviderHostRequestDecision,
    HostRequestOutcome as ProviderHostRequestOutcome, ProviderEvent, ProviderInteractionKind,
    ProviderPlanStatus,
};
use hatchery_observation_protocol::{
    truncate_observation_text, BlockAuthorityV1, HostDecisionAuthorityV1, HostRequestDecisionV1,
    HostRequestOutcomeV1, ObservationCapabilitiesV1, ObservationEvidenceV1,
    ObservationInteractionOutcomeV1, ObservationKindV1, ObservationSourceFamilyV1,
    ObservationTodoItemV1, ObservationTodoStateV1, ObservationV1,
    OBSERVATION_ACTION_BLOCKED_REASON_MAX_BYTES,
};

/// The category `ObservationKindV1::Error` carries for a provider error.
///
/// `detail` is a CATEGORY, not a message: `validate_error_category`
/// (`hatchery-observation-api`) accepts only hyphen-separated segments of
/// lowercase letters and digits, and rejects anything else -- and that
/// rejection propagates out of the harness runtime loop, taking the operator
/// wire down with it. Passing a provider's sentence through here did exactly
/// that, live. A refusal still names itself, in this field's own vocabulary: a
/// distinct slug rather than a distinct sentence.
fn observation_error_detail(message: &str) -> String {
    let message = message.trim().to_ascii_lowercase();
    if message.starts_with("provider events rejected") {
        return "provider-events-rejected".to_owned();
    }
    "provider-error".to_owned()
}

fn observation_evidence(family: AdapterFamily) -> Option<ObservationEvidenceV1> {
    match family {
        // Lifecycle hooks are retired (owner ruling 2026-09-25): nothing
        // produces an `AdapterFamily::Hook`/`ManagedHook`-sourced event
        // anymore. Both fall through to the same evidence a PTY-transport
        // session otherwise reports; `ObservationEvidenceV1::ManagedHook`
        // stays defined on the observation wire (schema/wire-compat, not a
        // code-removal question) but nothing here produces it.
        AdapterFamily::PtySemantic | AdapterFamily::Hook | AdapterFamily::ManagedHook => {
            Some(ObservationEvidenceV1::PtyHint)
        }
        AdapterFamily::Pipe | AdapterFamily::OneShot | AdapterFamily::Acp => {
            Some(ObservationEvidenceV1::StructuredProvider)
        }
        AdapterFamily::History
        | AdapterFamily::Resume
        | AdapterFamily::SessionOptions
        | AdapterFamily::CapabilityProbe => None,
    }
}

fn observation_source_capabilities(
    source: &gate4agent_types::ProviderSource,
) -> Option<(ObservationSourceFamilyV1, ObservationCapabilitiesV1)> {
    let adapter = source.binding.id.as_str();
    Some(match source.family {
        AdapterFamily::PtySemantic => (
            ObservationSourceFamilyV1::PtySemantic,
            ObservationCapabilitiesV1::default(),
        ),
        AdapterFamily::Pipe => (
            ObservationSourceFamilyV1::Pipe,
            ObservationCapabilitiesV1 {
                tools: matches!(adapter, "claude-code" | "codex" | "kimi"),
                usage: matches!(adapter, "claude-code" | "codex"),
                ..ObservationCapabilitiesV1::default()
            },
        ),
        // Hooks are retired, so no normalizer backs a capability claim for
        // these families: they claim none.
        AdapterFamily::Hook => (
            ObservationSourceFamilyV1::Hook,
            ObservationCapabilitiesV1::default(),
        ),
        AdapterFamily::ManagedHook => (
            ObservationSourceFamilyV1::ManagedHook,
            ObservationCapabilitiesV1::default(),
        ),
        AdapterFamily::OneShot => (
            ObservationSourceFamilyV1::OneShot,
            ObservationCapabilitiesV1::default(),
        ),
        AdapterFamily::Acp => (
            ObservationSourceFamilyV1::Acp,
            ObservationCapabilitiesV1 {
                tools: true,
                usage: true,
                ..ObservationCapabilitiesV1::default()
            },
        ),
        AdapterFamily::History
        | AdapterFamily::Resume
        | AdapterFamily::SessionOptions
        | AdapterFamily::CapabilityProbe => return None,
    })
}

fn token_usage_is_observed(usage: &gate4agent_types::TokenUsage) -> bool {
    usage.input_tokens != 0
        || usage.output_tokens != 0
        || usage.cache_read_tokens != 0
        || usage.cache_write_tokens != 0
        || usage.reasoning_tokens != 0
        || usage.context_window.is_some()
}

fn source_capabilities_observation(
    source: &gate4agent_types::ProviderSource,
    source_sequence: u64,
    evidence: ObservationEvidenceV1,
) -> Option<ObservationV1> {
    let (source_family, capabilities) = observation_source_capabilities(source)?;
    Some(ObservationV1 {
        source_sequence,
        observed_at_unix_ms: None,
        evidence,
        kind: ObservationKindV1::SourceCapabilities {
            source_family,
            source_adapter: source.binding.id.as_str().to_owned(),
            capabilities,
        },
        truncated: false,
    })
}

/// Map `gate4agent-types`' `ProviderHostRequestDecision`/
/// `ProviderHostDecisionAuthority` onto this crate's observation-wire
/// `HostRequestDecisionV1`/`HostDecisionAuthorityV1` -- `gate4agent-
/// observation-protocol` cannot depend on `gate4agent-types` (dependency-
/// light wire crate), so the conversion lives here, the shell that already
/// depends on both. A plain match, not `format!("{:?}", ..)`: the wire
/// carries the typed value itself.
fn observation_host_request_decision(decision: &ProviderHostRequestDecision) -> HostRequestDecisionV1 {
    match decision {
        ProviderHostRequestDecision::Granted { by } => {
            HostRequestDecisionV1::Granted { by: observation_host_decision_authority(*by) }
        }
        ProviderHostRequestDecision::Denied { by } => {
            HostRequestDecisionV1::Denied { by: observation_host_decision_authority(*by) }
        }
        ProviderHostRequestDecision::Deferred => HostRequestDecisionV1::Deferred,
    }
}

/// Map `gate4agent-types`' `ProviderHostRequestOutcome` onto this crate's
/// observation-wire `HostRequestOutcomeV1` -- same reason
/// `observation_host_request_decision` exists rather than a `From` impl on
/// either side (`gate4agent-observation-protocol` cannot depend on
/// `gate4agent-types`). A `Failed` error is cut to the wire bound BEFORE the
/// observation is constructed, at a safe UTF-8 boundary, since
/// `ObservationKindV1::validate` rejects an over-long value outright rather
/// than truncating.
fn observation_host_request_outcome(outcome: &ProviderHostRequestOutcome) -> HostRequestOutcomeV1 {
    match outcome {
        ProviderHostRequestOutcome::Executed => HostRequestOutcomeV1::Executed,
        ProviderHostRequestOutcome::Failed { error } => {
            let (error, _) =
                truncate_observation_text(error, OBSERVATION_ACTION_BLOCKED_REASON_MAX_BYTES);
            HostRequestOutcomeV1::Failed { error }
        }
    }
}

fn observation_host_decision_authority(by: ProviderHostDecisionAuthority) -> HostDecisionAuthorityV1 {
    match by {
        ProviderHostDecisionAuthority::Gate => HostDecisionAuthorityV1::Gate,
        ProviderHostDecisionAuthority::Policy => HostDecisionAuthorityV1::Policy,
        ProviderHostDecisionAuthority::Operator => HostDecisionAuthorityV1::Operator,
        ProviderHostDecisionAuthority::DeadlinePolicy => HostDecisionAuthorityV1::DeadlinePolicy,
    }
}

fn observation_interaction_outcome(
    outcome: gate4agent_types::ProviderInteractionOutcome,
) -> ObservationInteractionOutcomeV1 {
    match outcome {
        gate4agent_types::ProviderInteractionOutcome::Approved => {
            ObservationInteractionOutcomeV1::Approved
        }
        gate4agent_types::ProviderInteractionOutcome::Answered => {
            ObservationInteractionOutcomeV1::Answered
        }
        gate4agent_types::ProviderInteractionOutcome::Denied => {
            ObservationInteractionOutcomeV1::Denied
        }
        gate4agent_types::ProviderInteractionOutcome::Interrupted => {
            ObservationInteractionOutcomeV1::Interrupted
        }
        gate4agent_types::ProviderInteractionOutcome::TurnEnded => {
            ObservationInteractionOutcomeV1::TurnEnded
        }
        gate4agent_types::ProviderInteractionOutcome::Superseded => {
            ObservationInteractionOutcomeV1::Superseded
        }
    }
}

/// Maps ACP's per-step plan status onto the observation wire's own todo
/// state -- a plain match, not `format!("{:?}", ..)`, for the same reason
/// `observation_host_decision_authority` above is: the source carries the
/// typed value itself, not a string to reparse.
fn observation_todo_state(status: ProviderPlanStatus) -> ObservationTodoStateV1 {
    match status {
        ProviderPlanStatus::Pending => ObservationTodoStateV1::Pending,
        ProviderPlanStatus::InProgress => ObservationTodoStateV1::InProgress,
        ProviderPlanStatus::Completed => ObservationTodoStateV1::Completed,
    }
}

/// The observations one node control event projects to, in the order they
/// were minted. Deterministic: the same event always yields the same
/// observations (`observed_at_unix_ms` is left unset -- the receive time lives
/// on the ingress envelope), so the same node cursor observed twice compares
/// canonically equal.
///
/// A blocked action (`ObservationKindV1::ActionBlocked`) is NOT minted here: the
/// node computes the block once, on its agent stream, and
/// [`blocked_chunk_observation`] projects that chunk.
pub fn control_event_observations(event: &ControlEvent) -> Vec<ObservationV1> {
    let (source, source_sequence, provider_sequence, provider_event) = match &event.event {
        ControlEventKind::ProviderEvent {
            sequence,
            source,
            source_sequence,
            event,
        } => (source, *source_sequence, *sequence, event),
        ControlEventKind::ProviderGap {
            source,
            source_sequence,
            missed,
            ..
        } => {
            let Some(evidence) = observation_evidence(source.family) else {
                return Vec::new();
            };
            let mut observations = Vec::with_capacity(2);
            observations.extend(source_capabilities_observation(
                source,
                *source_sequence,
                evidence,
            ));
            observations.push(ObservationV1 {
                source_sequence: *source_sequence,
                observed_at_unix_ms: None,
                evidence,
                kind: ObservationKindV1::Gap { missed: *missed },
                truncated: false,
            });
            return observations;
        }
        ControlEventKind::InteractionResolved {
            interaction_id,
            outcome,
        } => {
            return vec![ObservationV1 {
                source_sequence: event.sequence,
                observed_at_unix_ms: None,
                evidence: ObservationEvidenceV1::NodeLifecycle,
                kind: ObservationKindV1::InteractionResolved {
                    correlation_id: interaction_correlation(
                        event.instance_id,
                        event.generation,
                        interaction_id.0,
                    ),
                    outcome: observation_interaction_outcome(*outcome),
                },
                truncated: false,
            }];
        }
        _ => return node_lifecycle_observations(event),
    };
    let Some(evidence) = observation_evidence(source.family) else {
        return Vec::new();
    };
    let is_pty_hint = evidence == ObservationEvidenceV1::PtyHint;
    let mut kinds = Vec::with_capacity(2);
    match provider_event {
        ProviderEvent::SessionStarted { .. } => kinds.push(ObservationKindV1::SessionStarted),
        ProviderEvent::TurnStarted { .. } => kinds.push(ObservationKindV1::TurnStarted),
        ProviderEvent::WorkingObserved => kinds.push(ObservationKindV1::Working),
        ProviderEvent::Thinking { .. } => kinds.push(ObservationKindV1::Working),
        ProviderEvent::ToolStarted { id, name, .. } => kinds.push(ObservationKindV1::ToolStarted {
            correlation_id: tool_correlation(event, source, id),
            class: tool_class_label(name),
        }),
        ProviderEvent::ToolCompleted { id, is_error, duration_ms, .. } if !is_pty_hint => {
            kinds.push(ObservationKindV1::ToolCompleted {
                correlation_id: tool_correlation(event, source, id),
                class: "Tool".to_owned(),
                success: !is_error,
                duration_ms: *duration_ms,
            });
        }
        ProviderEvent::TurnCompleted { usage, is_cumulative } if !is_pty_hint => {
            kinds.push(ObservationKindV1::TurnCompleted);
            if token_usage_is_observed(usage) {
                kinds.push(ObservationKindV1::Usage {
                    input_tokens: usage.input_tokens,
                    output_tokens: usage.output_tokens,
                    cache_read_tokens: usage.cache_read_tokens,
                    cache_write_tokens: usage.cache_write_tokens,
                    reasoning_tokens: usage.reasoning_tokens,
                    context_window: usage.context_window,
                    is_cumulative: *is_cumulative,
                });
            }
        }
        ProviderEvent::ContextWindowUsage { usage }
            if evidence == ObservationEvidenceV1::StructuredProvider =>
        {
            kinds.push(ObservationKindV1::ContextWindowUsage {
                uncached_input_tokens: Some(usage.uncached_input_tokens),
                cache_read_tokens: Some(usage.cache_read_tokens),
                cache_write_tokens: Some(usage.cache_write_tokens),
                output_tokens: Some(usage.output_tokens),
                unattributed_tokens: Some(usage.unattributed_tokens),
                used_tokens: usage.used_tokens,
                capacity_tokens: usage.capacity_tokens,
            });
        }
        // ACP's `usage_update` only ever reports `used_tokens`/
        // `context_window`, never the cache/segment breakdown the
        // `ProviderEvent::ContextWindowUsage` arm above maps -- the five
        // breakdown fields go in as `None` rather than a fabricated zero.
        // `cost_amount`/`cost_currency` have no `ObservationKindV1` of
        // their own; carrying provider turn cost onto this wire is a
        // separate decision this arm deliberately does not make, so both
        // are dropped here.
        ProviderEvent::UsageUpdated {
            used_tokens,
            context_window,
            ..
        } if evidence == ObservationEvidenceV1::StructuredProvider => {
            if let (Some(used_tokens), Some(capacity_tokens)) = (*used_tokens, *context_window) {
                kinds.push(ObservationKindV1::ContextWindowUsage {
                    uncached_input_tokens: None,
                    cache_read_tokens: None,
                    cache_write_tokens: None,
                    output_tokens: None,
                    unattributed_tokens: None,
                    used_tokens,
                    capacity_tokens,
                });
            }
        }
        // ACP always sends the FULL plan on `session/update`, never a
        // delta, so each snapshot wholesale-replaces the previous one --
        // `source_sequence` is already strictly increasing per (instance,
        // generation, provider source) (`Gate4AgentEngine::ingest_provider`
        // rejects any `source_sequence` that is not, `gate4agent-engine`),
        // so it doubles as the per-session `revision` a `TodoSnapshot`
        // needs without this function tracking any counter of its own.
        ProviderEvent::Plan { steps } if !is_pty_hint => {
            kinds.push(ObservationKindV1::TodoSnapshot {
                revision: source_sequence,
                items: steps
                    .iter()
                    .map(|step| ObservationTodoItemV1 {
                        id: None,
                        text: step.content.clone(),
                        state: observation_todo_state(step.status),
                    })
                    .collect(),
                // An empty plan is a CLEARED plan, not a finished one. `all`
                // is vacuously true on an empty list, and an agent that sent
                // `plan` with no entries would therefore have its timeline
                // entry rendered as "done" -- a completion nobody reported.
                // Emptiness has to be excluded explicitly.
                complete: !steps.is_empty()
                    && steps
                        .iter()
                        .all(|step| step.status == ProviderPlanStatus::Completed),
            });
        }
        ProviderEvent::TurnInterrupted => kinds.push(ObservationKindV1::TurnInterrupted),
        ProviderEvent::SessionEnded { is_error, .. } => {
            kinds.push(ObservationKindV1::Exited { success: Some(!is_error) });
        }
        // The message travels verbatim, bounded, rather than being replaced
        // by a constant. It was `"provider-error"` for every error alike,
        // which is a category where a reason belongs: the engine mints a
        // refusal here naming which capability it withheld and how many
        // events it dropped, and flattening that to a fixed word turns a
        // named refusal back into the silence it was written to end. An
        // empty message keeps the old constant, because a blank detail is
        // rejected by `ObservationKindV1::validate`.
        ProviderEvent::Error { message } => kinds.push(ObservationKindV1::Error {
            detail: observation_error_detail(message),
        }),
        ProviderEvent::Ready => kinds.push(ObservationKindV1::Ready),
        ProviderEvent::InteractionRequested {
            interaction_kind,
            tool_name,
            ..
        } => kinds.push(match interaction_kind {
            ProviderInteractionKind::Approval => ObservationKindV1::ApprovalRequested {
                correlation_id: interaction_correlation(
                    event.instance_id,
                    event.generation,
                    provider_sequence,
                ),
                tool_class: tool_class_label(tool_name),
            },
            ProviderInteractionKind::Question => ObservationKindV1::QuestionRequested {
                correlation_id: interaction_correlation(
                    event.instance_id,
                    event.generation,
                    provider_sequence,
                ),
                tool_class: tool_class_label(tool_name),
            },
        }),
        ProviderEvent::SubagentStarted { agent_id, agent_type, .. } => {
            kinds.push(ObservationKindV1::SubagentStarted {
                correlation_id: subagent_correlation(
                    event.instance_id,
                    event.generation,
                    source,
                    agent_id,
                ),
                class: agent_type
                    .as_deref()
                    .map(tool_class_label)
                    .unwrap_or_else(|| "Task".to_owned()),
            });
        }
        ProviderEvent::SubagentStopped { agent_id } if !is_pty_hint => {
            kinds.push(ObservationKindV1::SubagentCompleted {
                correlation_id: subagent_correlation(
                    event.instance_id,
                    event.generation,
                    source,
                    agent_id,
                ),
                success: None,
            });
        }
        ProviderEvent::RateLimited { .. } => kinds.push(ObservationKindV1::RateLimited),
        ProviderEvent::HostRequestObserved { method, decision, outcome, .. } => {
            kinds.push(ObservationKindV1::HostRequestObserved {
                class: tool_class_label(method),
                decision: observation_host_request_decision(decision),
                outcome: observation_host_request_outcome(outcome),
            });
        }
        ProviderEvent::UnrecognizedNotification { method, .. } => {
            kinds.push(ObservationKindV1::UnrecognizedNotification {
                method: method.clone(),
            });
        }
        ProviderEvent::SessionIdentityObserved { .. }
        | ProviderEvent::Text { .. }
        | ProviderEvent::InteractionResolved { .. }
        | ProviderEvent::ToolCompleted { .. }
        | ProviderEvent::TurnCompleted { .. }
        | ProviderEvent::ContextWindowUsage { .. }
        | ProviderEvent::SubagentStopped { .. }
        | ProviderEvent::UsageUpdated { .. }
        | ProviderEvent::Plan { .. }
        // ACP session/update coverage beyond text/tool/turn/plan/usage
        // streaming has no dedicated `ObservationKindV1` yet -- see the
        // matching comment on `agent_progress_event_kind` above for why
        // minting one is left to whoever owns this versioned wire contract.
        | ProviderEvent::UserMessage { .. }
        | ProviderEvent::AvailableCommandsUpdated { .. }
        | ProviderEvent::ModeChanged { .. }
        | ProviderEvent::SessionInfoUpdated { .. }
        | ProviderEvent::ConfigOptionsUpdated { .. } => {}
    }
    let reports_capabilities = source_sequence == 1
        || matches!(provider_event, ProviderEvent::SessionStarted { .. });
    let mut observations = kinds.into_iter().map(|kind| ObservationV1 {
            source_sequence,
            observed_at_unix_ms: None,
            evidence,
            kind,
            truncated: false,
        })
        .filter(|observation| observation.validate().is_ok())
        .collect::<Vec<_>>();
    // The observation engine coalesces this declaration by source and excludes it
    // from timeline rows. Emit it with the first source receipt (or an explicit
    // session restart), while gaps carry their own declaration for repair.
    if reports_capabilities {
        if let Some(capabilities) =
            source_capabilities_observation(source, source_sequence, evidence)
        {
            observations.insert(0, capabilities);
        }
    }
    observations
}

fn node_lifecycle_observations(event: &ControlEvent) -> Vec<ObservationV1> {
    let correlation_id = process_correlation(event.instance_id, event.generation);
    let kinds = match &event.event {
        ControlEventKind::Running { .. } => vec![
            ObservationKindV1::SessionStarted,
            ObservationKindV1::OwnedProcessStarted {
                correlation_id,
                class: "provider-session".to_owned(),
            },
        ],
        ControlEventKind::Exited { exit_code, .. } => vec![
            ObservationKindV1::OwnedProcessExited {
                correlation_id,
                success: exit_code.map(|code| code == 0),
                exit_code: *exit_code,
            },
            ObservationKindV1::Exited {
                success: exit_code.map(|code| code == 0),
            },
        ],
        ControlEventKind::Failed { .. } => vec![
            ObservationKindV1::OwnedProcessExited {
                correlation_id,
                success: Some(false),
                exit_code: None,
            },
            ObservationKindV1::Error {
                detail: "session-failed".to_owned(),
            },
        ],
        ControlEventKind::Removed => vec![ObservationKindV1::Stopped],
        _ => return Vec::new(),
    };
    std::iter::once(ObservationV1 {
        source_sequence: event.sequence,
        observed_at_unix_ms: None,
        evidence: ObservationEvidenceV1::NodeLifecycle,
        kind: ObservationKindV1::SourceCapabilities {
            source_family: ObservationSourceFamilyV1::NodeLifecycle,
            source_adapter: "node".to_owned(),
            capabilities: ObservationCapabilitiesV1 {
                owned_processes: true,
                ..ObservationCapabilitiesV1::default()
            },
        },
        truncated: false,
    })
        .chain(kinds.into_iter().map(|kind| ObservationV1 {
            source_sequence: event.sequence,
            observed_at_unix_ms: None,
            evidence: ObservationEvidenceV1::NodeLifecycle,
            kind,
            truncated: false,
        }))
        .filter(|observation| observation.validate().is_ok())
        .collect()
}

/// The observations one session record history summary projects to: the
/// declaration that this record reports a history summary, and the aggregate
/// snapshot itself. Both are `HistoryProjection` evidence and deterministic;
/// `source_sequence` is the record modification time (1 when the history has
/// none), so a newer history orders after an older one.
pub fn history_summary_observations(summary: &SessionHistorySummaryV1) -> [ObservationV1; 2] {
    let source_sequence = summary.modified_at_unix_ms.unwrap_or(1).max(1);
    [
        ObservationV1 {
            source_sequence,
            observed_at_unix_ms: None,
            evidence: ObservationEvidenceV1::HistoryProjection,
            kind: ObservationKindV1::SourceCapabilities {
                source_family: ObservationSourceFamilyV1::History,
                source_adapter: "native-history".to_owned(),
                capabilities: ObservationCapabilitiesV1 {
                    history_summary: true,
                    ..ObservationCapabilitiesV1::default()
                },
            },
            truncated: false,
        },
        ObservationV1 {
            source_sequence,
            observed_at_unix_ms: None,
            evidence: ObservationEvidenceV1::HistoryProjection,
            kind: ObservationKindV1::HistorySnapshot {
                message_count: summary.message_count,
                message_count_exact: summary.message_count_exact,
                completed_turn_count: summary.completed_turn_count,
                total_tokens: summary.total_tokens,
            },
            // The observation intentionally carries aggregate metrics only.
            // Omitted preview messages therefore do not make this aggregate
            // partial.
            truncated: false,
        },
    ]
}

/// The `ActionBlocked` observation for one agent-stream `Blocked` chunk, or
/// `None` for every other chunk.
///
/// The node decides what is a block (a denied host request, ACP's
/// `nonExecutionKind`, a refusal or quota stop) once, when it builds the chunk;
/// the observation carries exactly that fact and never re-derives it. The
/// evidence is `StructuredProvider`: a block is only ever minted from a typed
/// provider or host field, never from PTY text.
pub fn blocked_chunk_observation(chunk: &AgentStreamChunkV1) -> Option<ObservationV1> {
    let AgentStreamChunkKindV1::Blocked {
        correlation_id,
        tool_class,
        authority,
        reason_kind,
        reason,
        help,
    } = &chunk.kind
    else {
        return None;
    };
    let observation = ObservationV1 {
        source_sequence: chunk.source_sequence,
        observed_at_unix_ms: None,
        evidence: ObservationEvidenceV1::StructuredProvider,
        kind: ObservationKindV1::ActionBlocked {
            correlation_id: correlation_id.clone(),
            tool_class: tool_class.clone(),
            authority: block_authority(*authority),
            reason_kind: reason_kind.clone(),
            reason: reason.clone(),
            help: help.clone(),
        },
        truncated: false,
    };
    observation.validate().is_ok().then_some(observation)
}

/// Maps the node's block authority onto the observation wire's own: the node
/// names the host side `Host*`, this vocabulary names it `Harness*`.
fn block_authority(authority: NodeBlockAuthorityV1) -> BlockAuthorityV1 {
    match authority {
        NodeBlockAuthorityV1::HostGate => BlockAuthorityV1::HarnessGate,
        NodeBlockAuthorityV1::HostPolicy => BlockAuthorityV1::HarnessPolicy,
        NodeBlockAuthorityV1::HostDeadline => BlockAuthorityV1::HarnessDeadline,
        NodeBlockAuthorityV1::Operator => BlockAuthorityV1::Operator,
        NodeBlockAuthorityV1::ProviderClassifier => BlockAuthorityV1::ProviderClassifier,
        NodeBlockAuthorityV1::ProviderPermissionRule => BlockAuthorityV1::ProviderPermissionRule,
        NodeBlockAuthorityV1::ProviderSandbox => BlockAuthorityV1::ProviderSandbox,
        NodeBlockAuthorityV1::ProviderRefusal => BlockAuthorityV1::ProviderRefusal,
        NodeBlockAuthorityV1::UserRejected => BlockAuthorityV1::UserRejected,
        NodeBlockAuthorityV1::ProviderQuota => BlockAuthorityV1::ProviderQuota,
        NodeBlockAuthorityV1::Unknown => BlockAuthorityV1::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gate4agent_types::{
        AdapterBinding, AdapterId, AdapterVerification, AgentInstanceId, ContextWindowUsage,
        ProviderPlanPriority, ProviderPlanStep, ProviderSessionIdentity, ProviderSessionKey,
        ProviderSource, SessionGeneration, TokenUsage,
    };

    fn provider_control_event(
        family: AdapterFamily,
        provider_event: ProviderEvent,
    ) -> ControlEvent {
        provider_control_event_at(family, "fixture", 9, provider_event)
    }

    fn provider_control_event_at(
        family: AdapterFamily,
        adapter: &str,
        source_sequence: u64,
        provider_event: ProviderEvent,
    ) -> ControlEvent {
        ControlEvent {
            sequence: 41,
            command_id: None,
            instance_id: AgentInstanceId(7),
            generation: SessionGeneration(3),
            event: ControlEventKind::ProviderEvent {
                sequence: 8,
                source: provider_source(family, adapter),
                source_sequence,
                event: provider_event,
            },
        }
    }

    fn provider_source(family: AdapterFamily, adapter: &str) -> ProviderSource {
        ProviderSource {
            family,
            binding: AdapterBinding::new(
                AdapterId::new(adapter).unwrap(),
                "fixture/v1",
                AdapterVerification::SyntheticFixture,
            )
            .unwrap(),
        }
    }

    fn timeline_observations(observations: &[ObservationV1]) -> Vec<&ObservationV1> {
        observations
            .iter()
            .filter(|observation| {
                !matches!(observation.kind, ObservationKindV1::SourceCapabilities { .. })
            })
            .collect()
    }

    #[test]
    fn source_capabilities_match_what_each_family_can_report() {
        let claims = |family, adapter| {
            observation_source_capabilities(&provider_source(family, adapter)).unwrap()
        };
        assert_eq!(
            claims(AdapterFamily::PtySemantic, "codex"),
            (ObservationSourceFamilyV1::PtySemantic, ObservationCapabilitiesV1::default()),
        );
        let (family, pipe) = claims(AdapterFamily::Pipe, "codex");
        assert_eq!(family, ObservationSourceFamilyV1::Pipe);
        assert!(pipe.tools && pipe.usage);
        assert!(!pipe.attention && !pipe.subagents && !pipe.todo && !pipe.file_changes);
        let (_, kimi_pipe) = claims(AdapterFamily::Pipe, "kimi");
        assert!(kimi_pipe.tools);
        assert!(!kimi_pipe.usage);
        let (family, acp) = claims(AdapterFamily::Acp, "codex");
        assert_eq!(family, ObservationSourceFamilyV1::Acp);
        assert!(acp.tools && acp.usage);
        assert!(!acp.attention && !acp.subagents && !acp.todo);
        assert!(!acp.owned_processes && !acp.file_changes && !acp.history_summary);
        assert_eq!(
            claims(AdapterFamily::OneShot, "codex"),
            (ObservationSourceFamilyV1::OneShot, ObservationCapabilitiesV1::default()),
        );
        // Lifecycle hooks are retired: their families claim nothing.
        for (family, expected) in [
            (AdapterFamily::Hook, ObservationSourceFamilyV1::Hook),
            (AdapterFamily::ManagedHook, ObservationSourceFamilyV1::ManagedHook),
        ] {
            assert_eq!(
                claims(family, "claude-code"),
                (expected, ObservationCapabilitiesV1::default()),
            );
        }
        for family in [
            AdapterFamily::History,
            AdapterFamily::Resume,
            AdapterFamily::SessionOptions,
            AdapterFamily::CapabilityProbe,
        ] {
            assert!(observation_evidence(family).is_none());
            assert!(observation_source_capabilities(&provider_source(family, "codex")).is_none());
        }
    }

    #[test]
    fn a_source_declares_its_capabilities_with_its_first_event_only() {
        let first = control_event_observations(&provider_control_event_at(
            AdapterFamily::Acp,
            "claude-code",
            1,
            ProviderEvent::WorkingObserved,
        ));
        assert_eq!(first.len(), 2);
        assert_eq!(first[0].source_sequence, 1);
        assert!(matches!(
            &first[0].kind,
            ObservationKindV1::SourceCapabilities {
                source_family: ObservationSourceFamilyV1::Acp,
                source_adapter,
                capabilities,
            } if source_adapter == "claude-code" && capabilities.tools && capabilities.usage
        ));
        assert_eq!(first[1].kind, ObservationKindV1::Working);

        let text = provider_control_event_at(
            AdapterFamily::Acp,
            "claude-code",
            2,
            ProviderEvent::Text { text: String::new(), is_delta: true },
        );
        assert!(control_event_observations(&text).is_empty());
        let working = control_event_observations(&provider_control_event_at(
            AdapterFamily::Acp,
            "claude-code",
            3,
            ProviderEvent::WorkingObserved,
        ));
        assert_eq!(working.len(), 1);
        assert_eq!(working[0].kind, ObservationKindV1::Working);
    }

    #[test]
    fn a_first_event_that_is_only_an_identity_still_declares_capabilities() {
        let identity = provider_control_event_at(
            AdapterFamily::Acp,
            "claude-code",
            1,
            ProviderEvent::SessionIdentityObserved {
                identity: ProviderSessionIdentity {
                    key: ProviderSessionKey::SessionId,
                    id: String::new(),
                    transcript_path: None,
                },
            },
        );
        let projected = control_event_observations(&identity);
        assert_eq!(projected.len(), 1);
        assert!(matches!(
            &projected[0].kind,
            ObservationKindV1::SourceCapabilities {
                source_family: ObservationSourceFamilyV1::Acp,
                source_adapter,
                ..
            } if source_adapter == "claude-code"
        ));
    }

    #[test]
    fn provider_gap_receives_capabilities_and_preserves_source_sequence() {
        let control = ControlEvent {
            sequence: 99,
            command_id: None,
            instance_id: AgentInstanceId(7),
            generation: SessionGeneration(3),
            event: ControlEventKind::ProviderGap {
                sequence: 44,
                source: provider_source(AdapterFamily::Acp, "grok"),
                source_sequence: 11,
                missed: 2,
            },
        };
        let projected = control_event_observations(&control);
        assert_eq!(projected.len(), 2);
        assert_eq!(projected[0].source_sequence, 11);
        assert!(matches!(
            &projected[0].kind,
            ObservationKindV1::SourceCapabilities {
                source_family: ObservationSourceFamilyV1::Acp,
                source_adapter,
                ..
            } if source_adapter == "grok"
        ));
        assert_eq!(projected[1].source_sequence, 11);
        assert_eq!(projected[1].kind, ObservationKindV1::Gap { missed: 2 });
    }

    #[test]
    fn projection_is_deterministic_and_carries_no_wall_clock() {
        let event = provider_control_event_at(
            AdapterFamily::Acp,
            "claude-code",
            1,
            ProviderEvent::WorkingObserved,
        );
        let first = control_event_observations(&event);
        assert_eq!(first, control_event_observations(&event));
        assert!(first.iter().all(|observation| observation.observed_at_unix_ms.is_none()));
        let lifecycle = ControlEvent {
            sequence: 5,
            command_id: None,
            instance_id: AgentInstanceId(7),
            generation: SessionGeneration(3),
            event: ControlEventKind::Running { process_id: None },
        };
        let projected = control_event_observations(&lifecycle);
        assert_eq!(projected, control_event_observations(&lifecycle));
        assert!(projected.iter().all(|observation| observation.observed_at_unix_ms.is_none()));
    }

    #[test]
    fn node_lifecycle_events_project_to_owned_process_observations() {
        let project = |event| {
            control_event_observations(&ControlEvent {
                sequence: 6,
                command_id: None,
                instance_id: AgentInstanceId(7),
                generation: SessionGeneration(3),
                event,
            })
        };
        let running = project(ControlEventKind::Running { process_id: None });
        assert!(matches!(
            &running[0].kind,
            ObservationKindV1::SourceCapabilities {
                source_family: ObservationSourceFamilyV1::NodeLifecycle,
                capabilities: ObservationCapabilitiesV1 { owned_processes: true, .. },
                ..
            }
        ));
        assert_eq!(running[1].kind, ObservationKindV1::SessionStarted);
        let ObservationKindV1::OwnedProcessStarted { correlation_id, .. } = &running[2].kind
        else {
            panic!("expected the owned process to start");
        };
        assert_eq!(
            correlation_id,
            &process_correlation(AgentInstanceId(7), SessionGeneration(3)),
        );
        let exited = project(ControlEventKind::Exited { exit_code: Some(0), forced: false });
        assert!(matches!(
            exited[1].kind,
            ObservationKindV1::OwnedProcessExited { success: Some(true), exit_code: Some(0), .. }
        ));
        assert_eq!(exited[2].kind, ObservationKindV1::Exited { success: Some(true) });
        let failed = project(ControlEventKind::Failed { message: String::new() });
        assert_eq!(
            failed[2].kind,
            ObservationKindV1::Error { detail: "session-failed".to_owned() },
        );
        assert_eq!(
            project(ControlEventKind::Removed)[1].kind,
            ObservationKindV1::Stopped,
        );
        assert!(project(ControlEventKind::Registered).is_empty());
    }

    /// A denied host request projects the audit-only `HostRequestObserved`; the
    /// blocked action itself arrives as the node's `Blocked` chunk.
    #[test]
    fn denied_host_request_projects_the_audit_observation_and_no_block() {
        let projected = control_event_observations(&provider_control_event_at(
            AdapterFamily::Acp,
            "claude-code",
            2,
            ProviderEvent::HostRequestObserved {
                method: "terminal/create".to_owned(),
                params_json: String::new(),
                decision: ProviderHostRequestDecision::Denied {
                    by: ProviderHostDecisionAuthority::Gate,
                },
                outcome: ProviderHostRequestOutcome::Executed,
                reason: None,
            },
        ));
        let timeline = timeline_observations(&projected);
        assert_eq!(timeline.len(), 1);
        assert!(matches!(
            &timeline[0].kind,
            ObservationKindV1::HostRequestObserved {
                decision: HostRequestDecisionV1::Denied { by: HostDecisionAuthorityV1::Gate },
                ..
            }
        ));
    }

    fn blocked_chunk(authority: NodeBlockAuthorityV1) -> AgentStreamChunkV1 {
        AgentStreamChunkV1 {
            source_sequence: 7,
            kind: AgentStreamChunkKindV1::Blocked {
                correlation_id: Some("tool-deadbeefcafebabe".to_owned()),
                tool_class: "Shell".to_owned(),
                authority,
                reason_kind: Some("gate-rule".to_owned()),
                reason: "blocked by dangerous-command gate".to_owned(),
                help: Some("add a Bash permission rule".to_owned()),
            },
        }
    }

    #[test]
    fn a_blocked_chunk_projects_to_exactly_its_action_blocked_fact() {
        let observation = blocked_chunk_observation(&blocked_chunk(NodeBlockAuthorityV1::HostGate))
            .expect("a blocked chunk projects");
        assert_eq!(observation.source_sequence, 7);
        assert_eq!(observation.evidence, ObservationEvidenceV1::StructuredProvider);
        assert_eq!(observation.observed_at_unix_ms, None);
        assert_eq!(
            observation.kind,
            ObservationKindV1::ActionBlocked {
                correlation_id: Some("tool-deadbeefcafebabe".to_owned()),
                tool_class: "Shell".to_owned(),
                authority: BlockAuthorityV1::HarnessGate,
                reason_kind: Some("gate-rule".to_owned()),
                reason: "blocked by dangerous-command gate".to_owned(),
                help: Some("add a Bash permission rule".to_owned()),
            },
        );
        observation.validate().unwrap();
    }

    #[test]
    fn every_node_block_authority_maps_to_its_observation_authority() {
        for (node, expected) in [
            (NodeBlockAuthorityV1::HostGate, BlockAuthorityV1::HarnessGate),
            (NodeBlockAuthorityV1::HostPolicy, BlockAuthorityV1::HarnessPolicy),
            (NodeBlockAuthorityV1::HostDeadline, BlockAuthorityV1::HarnessDeadline),
            (NodeBlockAuthorityV1::Operator, BlockAuthorityV1::Operator),
            (NodeBlockAuthorityV1::ProviderClassifier, BlockAuthorityV1::ProviderClassifier),
            (
                NodeBlockAuthorityV1::ProviderPermissionRule,
                BlockAuthorityV1::ProviderPermissionRule,
            ),
            (NodeBlockAuthorityV1::ProviderSandbox, BlockAuthorityV1::ProviderSandbox),
            (NodeBlockAuthorityV1::ProviderRefusal, BlockAuthorityV1::ProviderRefusal),
            (NodeBlockAuthorityV1::UserRejected, BlockAuthorityV1::UserRejected),
            (NodeBlockAuthorityV1::ProviderQuota, BlockAuthorityV1::ProviderQuota),
            (NodeBlockAuthorityV1::Unknown, BlockAuthorityV1::Unknown),
        ] {
            let observation = blocked_chunk_observation(&blocked_chunk(node)).unwrap();
            let ObservationKindV1::ActionBlocked { authority, .. } = observation.kind else {
                panic!("expected ActionBlocked");
            };
            assert_eq!(authority, expected);
        }
    }

    #[test]
    fn a_history_summary_projects_a_declaration_and_an_aggregate_snapshot() {
        let summary = SessionHistorySummaryV1 {
            message_count: 12,
            message_count_exact: true,
            completed_turn_count: Some(5),
            total_tokens: Some(4_000),
            modified_at_unix_ms: Some(1_786_000_000_000),
        };
        let projected = history_summary_observations(&summary);
        assert!(matches!(
            projected[0].kind,
            ObservationKindV1::SourceCapabilities {
                source_family: ObservationSourceFamilyV1::History,
                capabilities: ObservationCapabilitiesV1 { history_summary: true, .. },
                ..
            }
        ));
        assert_eq!(
            projected[1].kind,
            ObservationKindV1::HistorySnapshot {
                message_count: 12,
                message_count_exact: true,
                completed_turn_count: Some(5),
                total_tokens: Some(4_000),
            },
        );
        for observation in &projected {
            assert_eq!(observation.source_sequence, 1_786_000_000_000);
            assert_eq!(observation.evidence, ObservationEvidenceV1::HistoryProjection);
            assert_eq!(observation.observed_at_unix_ms, None);
            observation.validate().unwrap();
        }
        let undated = history_summary_observations(&SessionHistorySummaryV1 {
            modified_at_unix_ms: None,
            ..summary
        });
        assert!(undated.iter().all(|observation| observation.source_sequence == 1));
        assert_eq!(history_summary_observations(&summary), projected);
    }

    #[test]
    fn chunks_that_are_not_blocks_project_nothing() {
        let text = AgentStreamChunkV1 {
            source_sequence: 3,
            kind: AgentStreamChunkKindV1::Text { text: "hello".to_owned(), is_delta: false },
        };
        assert_eq!(blocked_chunk_observation(&text), None);
    }

    #[test]
    fn exact_context_projects_only_from_structured_non_pty_provider_events() {
        let usage = ContextWindowUsage {
            uncached_input_tokens: 70,
            cache_read_tokens: 20,
            cache_write_tokens: 0,
            output_tokens: 10,
            unattributed_tokens: 5,
            used_tokens: 105,
            capacity_tokens: 100,
        };
        let structured = control_event_observations(&provider_control_event_at(
            AdapterFamily::Pipe,
            "codex",
            2,
            ProviderEvent::ContextWindowUsage { usage },
        ));
        let timeline = timeline_observations(&structured);
        assert_eq!(timeline.len(), 1);
        assert_eq!(timeline[0].evidence, ObservationEvidenceV1::StructuredProvider);
        assert_eq!(
            timeline[0].kind,
            ObservationKindV1::ContextWindowUsage {
                uncached_input_tokens: Some(70),
                cache_read_tokens: Some(20),
                cache_write_tokens: Some(0),
                output_tokens: Some(10),
                unattributed_tokens: Some(5),
                used_tokens: 105,
                capacity_tokens: 100,
            }
        );

        for family in [AdapterFamily::PtySemantic, AdapterFamily::ManagedHook] {
            let projected = control_event_observations(&provider_control_event_at(
                family,
                "codex",
                2,
                ProviderEvent::ContextWindowUsage { usage },
            ));
            assert!(
                timeline_observations(&projected).is_empty(),
                "{family:?} must not project an authoritative context fact"
            );
        }
    }

    /// A fleet Pipe adapter's structured stream projects private categorical
    /// observations end to end: source capabilities, a redacted tool start,
    /// an approval request/resolution round-trip, and usage.
    #[test]
    fn pipe_events_project_private_categorical_tools_and_usage() {
        let ready = provider_control_event_at(AdapterFamily::Pipe, "codex", 1, ProviderEvent::Ready);
        let projected = control_event_observations(&ready);
        let ObservationKindV1::SourceCapabilities { capabilities, .. } = &projected[0].kind else {
            panic!("expected source capabilities");
        };
        assert!(capabilities.tools && capabilities.usage);
        assert!(!capabilities.attention);
        assert!(!capabilities.subagents && !capabilities.todo && !capabilities.file_changes);

        let tool = control_event_observations(&provider_control_event_at(
            AdapterFamily::Pipe,
            "codex",
            2,
            ProviderEvent::ToolStarted {
                id: "private-provider-tool-id".to_owned(),
                name: "run_shell_command".to_owned(),
                input_json: String::new(),
                agent_id: None,
            },
        ));
        assert!(matches!(
            timeline_observations(&tool)[0].kind,
            ObservationKindV1::ToolStarted { ref class, .. } if class == "Shell"
        ));
        let attention = control_event_observations(&provider_control_event_at(
            AdapterFamily::Pipe,
            "codex",
            3,
            ProviderEvent::InteractionRequested {
                request_id: Some("private-request-id".to_owned()),
                interaction_kind: ProviderInteractionKind::Approval,
                tool_name: "Shell".to_owned(),
                title: None,
                prompt: String::new(),
                options: Vec::new(),
                agent_id: None,
            },
        ));
        let ObservationKindV1::ApprovalRequested {
            correlation_id: requested_correlation,
            tool_class,
        } = &timeline_observations(&attention)[0].kind else {
            panic!("expected approval observation");
        };
        assert_eq!(tool_class, "Shell");
        let requested_correlation = requested_correlation.clone();
        let raw_resolution = control_event_observations(&provider_control_event_at(
            AdapterFamily::Pipe,
            "codex",
            4,
            ProviderEvent::InteractionResolved {
                request_id: "private-request-id".to_owned(),
                outcome: gate4agent_types::ProviderInteractionOutcome::Approved,
            },
        ));
        assert!(raw_resolution.is_empty());
        let resolved = control_event_observations(&ControlEvent {
            sequence: 42,
            command_id: None,
            instance_id: AgentInstanceId(7),
            generation: SessionGeneration(3),
            event: ControlEventKind::InteractionResolved {
                interaction_id: gate4agent_types::ProviderInteractionId(8),
                outcome: gate4agent_types::ProviderInteractionOutcome::Approved,
            },
        });
        let ObservationKindV1::InteractionResolved {
            correlation_id,
            outcome,
        } = &resolved[0].kind else {
            panic!("expected interaction resolution observation");
        };
        assert_eq!(correlation_id, &requested_correlation);
        assert_eq!(*outcome, ObservationInteractionOutcomeV1::Approved);
        let usage = control_event_observations(&provider_control_event_at(
            AdapterFamily::Pipe,
            "codex",
            5,
            ProviderEvent::TurnCompleted {
                usage: TokenUsage {
                    input_tokens: 5,
                    output_tokens: 8,
                    ..TokenUsage::default()
                },
                is_cumulative: false,
            },
        ));
        assert!(timeline_observations(&usage).iter().any(|observation| matches!(
            observation.kind,
            ObservationKindV1::Usage { input_tokens: 5, output_tokens: 8, .. }
        )));
        let encoded = serde_json::to_string(&(
            projected,
            tool,
            attention,
            raw_resolution,
            resolved,
            usage,
        ))
        .unwrap();
        assert!(!encoded.contains("private-provider-tool-id"));
        assert!(!encoded.contains("private-request-id"));
    }

    #[test]
    fn pty_hint_never_projects_authoritative_completion() {
        let completed = provider_control_event(
            AdapterFamily::PtySemantic,
            ProviderEvent::TurnCompleted {
                usage: TokenUsage {
                    input_tokens: 10,
                    output_tokens: 20,
                    cache_read_tokens: 30,
                    cache_write_tokens: 40,
                    reasoning_tokens: 50,
                    context_window: Some(128_000),
                },
                is_cumulative: true,
            },
        );
        let projected = control_event_observations(&completed);
        assert!(projected.is_empty());
        assert!(timeline_observations(&projected).is_empty());
        let tool_completed = provider_control_event(
            AdapterFamily::PtySemantic,
            ProviderEvent::ToolCompleted {
                id: "private-tool-id".to_owned(),
                output: "private output".to_owned(),
                is_error: false,
                duration_ms: Some(5),
                agent_id: None,
                non_execution_kind: None,
            },
        );
        let projected = control_event_observations(&tool_completed);
        assert!(projected.is_empty());
        assert!(timeline_observations(&projected).is_empty());
        let subagent_completed = provider_control_event(
            AdapterFamily::PtySemantic,
            ProviderEvent::SubagentStopped {
                agent_id: "private-subagent-id".to_owned(),
            },
        );
        let projected = control_event_observations(&subagent_completed);
        assert!(projected.is_empty());
        assert!(timeline_observations(&projected).is_empty());

        let working = provider_control_event(
            AdapterFamily::PtySemantic,
            ProviderEvent::WorkingObserved,
        );
        let projected = control_event_observations(&working);
        assert_eq!(projected.len(), 1);
        let timeline = timeline_observations(&projected);
        assert_eq!(timeline[0].evidence, ObservationEvidenceV1::PtyHint);
        assert_eq!(timeline[0].kind, ObservationKindV1::Working);
    }

    #[test]
    fn unproven_codex_workflow_tool_payloads_do_not_project_detail() {
        for (name, input_json, private_value) in [
            (
                "plan_update",
                r#"{"summary":"private plan summary","status":"in_progress"}"#,
                "private plan summary",
            ),
            (
                "FileChange",
                r#"{"path":"C:\\private\\host.rs","status":"in_progress"}"#,
                r#"C:\\private\\host.rs"#,
            ),
        ] {
            let event = provider_control_event(
                AdapterFamily::Pipe,
                ProviderEvent::ToolStarted {
                    id: "private-workflow-id".to_owned(),
                    name: name.to_owned(),
                    input_json: input_json.to_owned(),
                    agent_id: None,
                },
            );
            let projected = control_event_observations(&event);
            assert_eq!(projected.len(), 1);
            let timeline = timeline_observations(&projected);
            assert_eq!(
                timeline[0].evidence,
                ObservationEvidenceV1::StructuredProvider,
            );
            assert!(matches!(
                &timeline[0].kind,
                ObservationKindV1::ToolStarted { .. }
            ));
            assert!(!matches!(
                &timeline[0].kind,
                ObservationKindV1::TodoSnapshot { .. }
                    | ObservationKindV1::FileChanged { .. }
            ));
            let wire = serde_json::to_string(&projected).unwrap();
            assert!(!wire.contains("private-workflow-id"));
            assert!(!wire.contains(private_value));
        }

        for name in ["plan_update", "FileChange"] {
            let pty = provider_control_event(
                AdapterFamily::PtySemantic,
                ProviderEvent::ToolStarted {
                    id: "private-workflow-id".to_owned(),
                    name: name.to_owned(),
                    input_json: r#"{"path":"C:\\private\\host.rs"}"#.to_owned(),
                    agent_id: None,
                },
            );
            let projected = control_event_observations(&pty);
            assert_eq!(projected.len(), 1);
            let timeline = timeline_observations(&projected);
            assert_eq!(timeline[0].evidence, ObservationEvidenceV1::PtyHint);
            assert!(matches!(
                &timeline[0].kind,
                ObservationKindV1::ToolStarted { .. }
            ));
        }
    }

    #[test]
    fn acp_plan_produces_a_todo_snapshot_carrying_source_sequence_as_revision() {
        let event = provider_control_event(
            AdapterFamily::Acp,
            ProviderEvent::Plan {
                steps: vec![
                    ProviderPlanStep {
                        content: "read the file".to_owned(),
                        priority: ProviderPlanPriority::High,
                        status: ProviderPlanStatus::Completed,
                    },
                    ProviderPlanStep {
                        content: "write the fix".to_owned(),
                        priority: ProviderPlanPriority::Medium,
                        status: ProviderPlanStatus::InProgress,
                    },
                ],
            },
        );
        let projected = control_event_observations(&event);
        let timeline = timeline_observations(&projected);
        assert_eq!(timeline.len(), 1);
        assert_eq!(
            timeline[0].evidence,
            ObservationEvidenceV1::StructuredProvider
        );
        let ObservationKindV1::TodoSnapshot {
            revision,
            items,
            complete,
        } = &timeline[0].kind
        else {
            panic!("expected a todo snapshot observation");
        };
        // `provider_control_event` fixes `source_sequence` at 9 (see its own
        // definition); that value doubles as the snapshot's revision -- see
        // the doc comment on the `ProviderEvent::Plan` match arm.
        assert_eq!(*revision, 9);
        assert_eq!(
            items,
            &vec![
                ObservationTodoItemV1 {
                    id: None,
                    text: "read the file".to_owned(),
                    state: ObservationTodoStateV1::Completed,
                },
                ObservationTodoItemV1 {
                    id: None,
                    text: "write the fix".to_owned(),
                    state: ObservationTodoStateV1::InProgress,
                },
            ]
        );
        assert!(!complete, "one step is still in progress");

        let all_done = provider_control_event(
            AdapterFamily::Acp,
            ProviderEvent::Plan {
                steps: vec![ProviderPlanStep {
                    content: "read the file".to_owned(),
                    priority: ProviderPlanPriority::High,
                    status: ProviderPlanStatus::Completed,
                }],
            },
        );
        let projected = control_event_observations(&all_done);
        let timeline = timeline_observations(&projected);
        let ObservationKindV1::TodoSnapshot { complete, .. } = &timeline[0].kind else {
            panic!("expected a todo snapshot observation");
        };
        assert!(*complete, "every step is completed");

        // A pty-hint source cannot claim the authoritative plan snapshot --
        // same rule as `ToolCompleted`/`SubagentCompleted` above.
        let pty = provider_control_event(
            AdapterFamily::PtySemantic,
            ProviderEvent::Plan {
                steps: vec![ProviderPlanStep {
                    content: "read the file".to_owned(),
                    priority: ProviderPlanPriority::High,
                    status: ProviderPlanStatus::Pending,
                }],
            },
        );
        let projected = control_event_observations(&pty);
        assert!(timeline_observations(&projected).is_empty());
    }

    #[test]
    fn acp_usage_updated_produces_context_window_usage_with_unobserved_breakdown() {
        let event = provider_control_event(
            AdapterFamily::Acp,
            ProviderEvent::UsageUpdated {
                used_tokens: Some(4_200),
                context_window: Some(200_000),
                cost_amount: Some("0.42".to_owned()),
                cost_currency: Some("USD".to_owned()),
            },
        );
        let projected = control_event_observations(&event);
        let timeline = timeline_observations(&projected);
        assert_eq!(timeline.len(), 1);
        assert_eq!(
            timeline[0].evidence,
            ObservationEvidenceV1::StructuredProvider
        );
        assert_eq!(
            timeline[0].kind,
            ObservationKindV1::ContextWindowUsage {
                uncached_input_tokens: None,
                cache_read_tokens: None,
                cache_write_tokens: None,
                output_tokens: None,
                unattributed_tokens: None,
                used_tokens: 4_200,
                capacity_tokens: 200_000,
            }
        );
        let wire = serde_json::to_string(&projected).unwrap();
        assert!(!wire.contains("0.42"));
        assert!(!wire.contains("USD"));

        // ACP requires both `used`/`size` -- a notification missing either
        // is malformed, not a measurement, and mints nothing.
        for incomplete in [
            ProviderEvent::UsageUpdated {
                used_tokens: None,
                context_window: Some(200_000),
                cost_amount: None,
                cost_currency: None,
            },
            ProviderEvent::UsageUpdated {
                used_tokens: Some(4_200),
                context_window: None,
                cost_amount: None,
                cost_currency: None,
            },
        ] {
            let event = provider_control_event(AdapterFamily::Acp, incomplete);
            let projected = control_event_observations(&event);
            assert!(timeline_observations(&projected).is_empty());
        }
    }

    #[test]
    fn token_bearing_pipe_turn_completed_projects_usage() {
        let completed = provider_control_event_at(
            AdapterFamily::Pipe,
            "codex",
            1,
            ProviderEvent::TurnCompleted {
                usage: TokenUsage {
                    input_tokens: 10,
                    output_tokens: 20,
                    cache_read_tokens: 30,
                    cache_write_tokens: 40,
                    reasoning_tokens: 50,
                    context_window: Some(128_000),
                },
                is_cumulative: true,
            },
        );
        let projected = control_event_observations(&completed);
        let ObservationKindV1::SourceCapabilities { capabilities, .. } = &projected[0].kind else {
            panic!("expected source capabilities");
        };
        assert!(capabilities.usage);
        assert_eq!(projected[1].kind, ObservationKindV1::TurnCompleted);
        assert!(matches!(
            projected[2].kind,
            ObservationKindV1::Usage {
                input_tokens: 10,
                output_tokens: 20,
                is_cumulative: true,
                ..
            }
        ));
    }

    /// K1c's split: an authorized (`Granted`) host request that failed
    /// WHILE EXECUTING (a `terminal/create` spawn error) carries
    /// `HostRequestOutcomeV1::Failed` on the SAME audit-only
    /// `HostRequestObserved` observation `decision: Granted` already mints.
    #[test]
    fn granted_host_request_execution_failure_carries_a_failed_outcome_never_a_block() {
        let spawn_error = "terminal/create spawn failed: os error 3";
        let event = provider_control_event_at(
            AdapterFamily::Acp,
            "claude-code",
            6,
            ProviderEvent::HostRequestObserved {
                method: "terminal/create".to_owned(),
                params_json: String::new(),
                decision: ProviderHostRequestDecision::Granted {
                    by: ProviderHostDecisionAuthority::Policy,
                },
                outcome: ProviderHostRequestOutcome::Failed { error: spawn_error.to_owned() },
                reason: None,
            },
        );
        let projected = control_event_observations(&event);
        let timeline = timeline_observations(&projected);
        let (observed_decision, observed_outcome) = timeline
            .iter()
            .find_map(|observation| match &observation.kind {
                ObservationKindV1::HostRequestObserved { decision, outcome, .. } => {
                    Some((*decision, outcome.clone()))
                }
                _ => None,
            })
            .expect("expected a HostRequestObserved observation");
        assert_eq!(
            observed_decision,
            HostRequestDecisionV1::Granted { by: HostDecisionAuthorityV1::Policy }
        );
        assert_eq!(
            observed_outcome,
            HostRequestOutcomeV1::Failed { error: spawn_error.to_owned() }
        );
        assert!(
            !timeline
                .iter()
                .any(|observation| matches!(observation.kind, ObservationKindV1::ActionBlocked { .. })),
            "a Granted decision must never mint ActionBlocked, regardless of outcome: {timeline:?}"
        );
        for observation in &projected {
            observation.validate().expect("minted observation must validate");
        }
    }
}
