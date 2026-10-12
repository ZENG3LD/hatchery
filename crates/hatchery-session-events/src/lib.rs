//! Session event layer.
//!
//! Turns what a provider CLI draws in its terminal (or what an ACP agent reports) into typed
//! [`SessionEvent`]s. Web HQ (and later the TUI) read only this layer: chat answers, thought and
//! action widgets, diffs, prompt-needed alerts and avatar states are all derived from events.
//!
//! Design (see docs/hatchery/research/session-ui-competitors-2026-10.md):
//! * live PTY parsing is the quality base: [`ProviderParser`] modules, one per provider, plus a generic fallback;
//! * every event carries its `source` and a [`Confidence`]: screen parsing is always `Heuristic`;
//! * ACP is an additional source behind the same event type ([`acp::AcpAdapter`]), `Structured`.

pub mod acp;
mod engine;
mod event;
pub mod providers;

pub use engine::EventStream;
pub use event::{ActionStatus, Confidence, EventKind, PromptKind, SessionEvent, SessionState};
pub use providers::{parser_for, Item, Parsed, ProviderParser};
