use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    /// Typed source (ACP, stream-json): the fields are what the agent said.
    Structured,
    /// Recovered from rendered terminal text: may be wrong after a provider UI change.
    Heuristic,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionState {
    Starting,
    Working,
    Idle,
    NeedsInput,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptKind {
    Login,
    Trust,
    Permission,
    ApiKey,
    Menu,
    Confirm,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionStatus {
    Running,
    Done,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EventKind {
    Banner { text: String },
    UserMessage { text: String },
    Answer { text: String },
    Thought { text: String },
    Action { name: String, args: String, result: Option<String>, status: ActionStatus },
    Diff { files: Vec<String>, patch: String },
    PromptNeeded { prompt: PromptKind, text: String, options: Vec<String> },
    State { state: SessionState, detail: String },
}

/// One typed event. `id` is stable for an item; `rev` grows when the same item changes
/// (a tool call getting its result, an answer growing). Consumers upsert by `id`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionEvent {
    pub id: u64,
    pub rev: u32,
    pub provider: String,
    /// `pty:<provider>` or `acp`.
    pub source: String,
    pub confidence: Confidence,
    pub ts_ms: u64,
    #[serde(flatten)]
    pub event: EventKind,
}
