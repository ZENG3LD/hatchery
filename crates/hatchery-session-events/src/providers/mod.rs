//! Per-provider live PTY parsers behind one trait.
//!
//! A parser looks at the current emulated screen (rows of text) and reports the *items* visible on it
//! (answers, thoughts, actions, diffs, prompts) with stable keys. The engine turns appearance and change
//! of items into [`crate::SessionEvent`]s. Parsers hold no clock and no I/O, so they are tested on
//! recorded captures.

mod claude;
mod codex;
pub(crate) mod common;
mod gemini;
mod generic;
mod grok;
mod kimi;

use crate::event::EventKind;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    /// Stable identity of the item across screens (kind plus a prefix of its text).
    pub key: String,
    pub kind: EventKind,
}

#[derive(Clone, Debug, Default)]
pub struct Parsed {
    pub items: Vec<Item>,
    /// The provider shows a working marker ("esc to interrupt", a device-code wait, ...): its text.
    pub working: Option<String>,
    /// A blocking prompt is on screen and the agent waits for the operator.
    pub needs_input: bool,
    pub prompt_hint: String,
}

pub trait ProviderParser: Send {
    fn provider(&self) -> &'static str;
    fn parse(&mut self, rows: &[String]) -> Parsed;
}

/// Parser for a provider id (`claude`, `codex`, `kimi`, `grok`, `gemini`); unknown ids get the generic fallback.
pub fn parser_for(provider: &str) -> Box<dyn ProviderParser> {
    match provider {
        "claude" => Box::new(claude::ClaudeParser),
        "codex" => Box::new(codex::CodexParser),
        "kimi" => Box::new(kimi::KimiParser),
        "grok" => Box::new(grok::GrokParser),
        "gemini" => Box::new(gemini::GeminiParser),
        _ => Box::new(generic::GenericParser),
    }
}
