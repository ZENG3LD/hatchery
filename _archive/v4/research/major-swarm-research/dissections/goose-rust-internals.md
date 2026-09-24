# Goose Rust Internals — Swarm Orchestration Deep Dive

**Source**: Block/Square Goose (Apache 2.0)
**Repository**: `c:\Users\VA PC\CODING\ML_TRADING\nemo\research\revolver-research\_tmp_src\goose`
**Date Analyzed**: 2026-02-08
**Focus**: Rust components relevant to swarm orchestration for Hatchery

---

## Executive Summary

Goose implements a sophisticated multi-agent system in Rust with production-grade prompt management, context compaction, subagent spawning, session persistence, and event-driven architecture. Key strengths include:

- **Builder-pattern prompt composition** with template rendering
- **80% threshold auto-compaction** with dual-visibility metadata
- **Nested subagent execution** with inheritance control
- **SQLite-backed session storage** with atomic updates
- **MCP (Model Context Protocol)** for tool/extension management
- **Progressive tool removal** on context overflow

---

## 1. Prompt Manager (★★★★★)

**Location**: `crates/goose/src/agents/prompt_manager.rs`

### Key Architecture

```rust
pub struct PromptManager {
    system_prompt_override: Option<String>,
    system_prompt_extras: Vec<String>,
    current_date_timestamp: String,  // Hourly granularity for cache hits
}

pub struct SystemPromptBuilder<'a, M> {
    manager: &'a M,
    extensions_info: Vec<ExtensionInfo>,
    frontend_instructions: Option<String>,
    extension_tool_count: Option<(usize, usize)>,
    subagents_enabled: bool,
    hints: Option<String>,  // From .goosehints / AGENTS.md
    code_execution_mode: bool,
}
```

### Copy-Worthy Patterns

**1. Builder Pattern with Template Rendering**
```rust
let system_prompt = manager
    .builder()
    .with_extension(ext_info)
    .with_hints(working_dir)
    .with_enable_subagents(true)
    .build();  // Renders Handlebars template with context
```

**2. Hourly Timestamp for Cache Optimization**
```rust
current_date_timestamp: Utc::now().format("%Y-%m-%d %H:00").to_string()
// Balances accuracy vs. prompt cache hit rate across sessions
```

**3. Unicode Tag Sanitization**
```rust
fn sanitize_unicode_tags(text: &str) -> String {
    // Strips E0000-E007F range (prompt injection attempts)
}
```

**4. Stable Tool Ordering for Caching**
```rust
extensions_info.sort_by(|a, b| a.name.cmp(&b.name));
// Ensures consistent prompt structure = better cache hits
```

### Applicability to Hatchery

✅ **Direct Copy**:
- Builder pattern for `SwarmPromptBuilder`
- Template rendering with context structs
- Unicode sanitization
- Stable ordering for caching

✅ **Adapt**:
- Replace `ExtensionInfo` with `AgentCapabilities`
- Add swarm-specific sections: hierarchy, shared memory, coordination protocol

---

## 2. Context Compaction (★★★★★)

**Location**: `crates/goose/src/context_mgmt/mod.rs`

### The 80% Threshold Pattern

```rust
pub const DEFAULT_COMPACTION_THRESHOLD: f64 = 0.8;

pub async fn check_if_compaction_needed(
    provider: &dyn Provider,
    conversation: &Conversation,
    threshold_override: Option<f64>,
    session: &Session,
) -> Result<bool> {
    let context_limit = provider.get_model_config().context_limit();
    let current_tokens = session.total_tokens;  // From provider metadata

    let usage_ratio = current_tokens as f64 / context_limit as f64;
    let needs_compaction = usage_ratio > threshold;

    Ok(needs_compaction)
}
```

### Dual-Visibility Metadata (★★★★★)

**The killer feature**: Messages have separate `agent_visible` and `user_visible` flags.

```rust
pub struct MessageMetadata {
    pub agent_visible: bool,   // Sent to LLM
    pub user_visible: bool,    // Shown in UI
}

impl MessageMetadata {
    pub fn agent_only() -> Self {
        Self { agent_visible: true, user_visible: false }
    }

    pub fn invisible() -> Self {
        Self { agent_visible: false, user_visible: false }
    }

    pub fn with_agent_invisible(self) -> Self {
        Self { agent_visible: false, ..self }
    }
}
```

### Compaction Algorithm

```rust
pub async fn compact_messages(
    provider: &dyn Provider,
    session_id: &str,
    conversation: &Conversation,
    manual_compact: bool,
) -> Result<(Conversation, ProviderUsage)> {
    // Step 1: Preserve most recent user message (unless manual)
    let preserved_user_message = if !manual_compact {
        messages.iter().rev().find(|m|
            m.is_agent_visible() && m.role == Role::User && has_text_only(m)
        )
    } else { None };

    // Step 2: Summarize with fast model
    let (summary_message, usage) = do_compact(provider, session_id, messages).await?;

    // Step 3: Update visibility metadata
    let mut final_messages = Vec::new();

    // Original messages: user_visible, NOT agent_visible
    for msg in messages {
        final_messages.push(msg.with_metadata(msg.metadata.with_agent_invisible()));
    }

    // Summary message: agent_visible, NOT user_visible
    final_messages.push(summary_message.with_metadata(MessageMetadata::agent_only()));

    // Continuation instruction: agent_visible, NOT user_visible
    final_messages.push(
        Message::assistant()
            .with_text(CONTINUATION_TEXT)
            .with_metadata(MessageMetadata::agent_only())
    );

    // Re-add preserved user message if exists (both visible)
    if let Some(user_msg) = preserved_user_message {
        final_messages.push(Message::user().with_text(&extract_text(&user_msg)));
    }

    Ok((Conversation::new_unvalidated(final_messages), usage))
}
```

### Progressive Tool Response Removal

```rust
async fn do_compact(
    provider: &dyn Provider,
    session_id: &str,
    messages: &[Message],
) -> Result<(Message, ProviderUsage)> {
    // Try removing tool responses progressively until context fits
    let removal_percentages = [0, 10, 20, 50, 100];

    for &remove_percent in &removal_percentages {
        let filtered = filter_tool_responses(messages, remove_percent);

        match provider.complete_fast(session_id, system_prompt, &filtered, &[]).await {
            Ok((response, usage)) => return Ok((response, usage)),
            Err(ProviderError::ContextLengthExceeded(_)) => continue,
            Err(e) => return Err(e),
        }
    }

    Err(anyhow!("Failed to compact even after removing all tool responses"))
}

fn filter_tool_responses(messages: &[Message], remove_percent: u32) -> Vec<&Message> {
    let tool_indices = find_tool_response_indices(messages);

    // Remove from middle outward
    let num_to_remove = (tool_indices.len() * remove_percent / 100).max(1);
    let middle = tool_indices.len() / 2;

    // Alternate removing left and right of middle
    for i in 0..num_to_remove {
        if i % 2 == 0 {
            remove(middle - offset - 1);
        } else {
            remove(middle + offset);
        }
    }
}
```

### Applicability to Hatchery

✅ **Critical Copy**:
- Dual-visibility metadata architecture
- 80% threshold check
- Progressive tool removal on overflow
- Preserve recent user message pattern

✅ **Swarm Adaptation**:
- Add `coordinator_visible` flag (3-way visibility)
- Compact per-agent conversations independently
- Shared memory compaction strategy

---

## 3. Subagent System (★★★★★)

**Location**:
- `crates/goose/src/agents/subagent_tool.rs` — Tool definition
- `crates/goose/src/agents/subagent_handler.rs` — Execution logic
- `crates/goose/src/agents/subagent_task_config.rs` — Config

### Subagent Tool Definition

```rust
pub const SUBAGENT_TOOL_NAME: &str = "subagent";

#[derive(Debug, Deserialize, Clone)]
pub struct SubagentParams {
    pub instructions: Option<String>,         // Ad-hoc task
    pub subrecipe: Option<String>,            // Predefined template
    pub parameters: Option<HashMap<String, Value>>,  // Template params
    pub extensions: Option<Vec<String>>,      // Inherit or override
    pub settings: Option<SubagentSettings>,   // Model overrides
    pub summary: bool,  // Default: true (return only final summary)
}

#[derive(Debug, Deserialize, Clone)]
pub struct SubagentSettings {
    pub provider: Option<String>,
    pub model: Option<String>,
    pub temperature: Option<f32>,
    pub max_turns: Option<usize>,
}
```

### Execution Modes

**1. Ad-hoc Mode**
```rust
// Parent calls:
subagent(instructions: "Research KuCoin API fees")

// Creates fresh agent with instructions
```

**2. Template Mode (SubRecipe)**
```rust
// Parent calls:
subagent(
    subrecipe: "research_api",
    parameters: {exchange: "KuCoin"}
)

// Loads template from recipe.yaml, fills params
```

**3. Augmented Mode**
```rust
// Parent calls:
subagent(
    subrecipe: "research_api",
    instructions: "Focus on spot trading fees",
    parameters: {exchange: "KuCoin"}
)

// Template instructions + extra context
```

### Extension Inheritance

```rust
async fn apply_settings_overrides(
    mut task_config: TaskConfig,
    params: &SubagentParams,
) -> Result<TaskConfig> {
    if let Some(extension_names) = &params.extensions {
        if extension_names.is_empty() {
            // Empty array = disable all extensions
            task_config.extensions = Vec::new();
        } else {
            // Filter to only specified extensions
            task_config.extensions.retain(|ext|
                extension_names.contains(&ext.name())
            );
        }
    }
    // If None: inherit all parent extensions

    Ok(task_config)
}
```

### Summary Mode

```rust
const SUMMARY_INSTRUCTIONS: &str = r#"
Important: Your parent agent will only receive your final message as a summary.
Make sure your last message provides:
- What you were asked to do
- What actions you took
- The results or outcomes
- Any important findings or recommendations
"#;

pub async fn run_complete_subagent_task(
    config: AgentConfig,
    recipe: Recipe,
    task_config: TaskConfig,
    return_last_only: bool,  // From params.summary
    session_id: String,
    cancellation_token: Option<CancellationToken>,
) -> Result<String> {
    let (messages, final_output) = get_agent_messages(...).await?;

    if return_last_only {
        // Return only last assistant message
        messages.last()
            .and_then(|m| extract_text(m))
            .unwrap_or("No text content")
    } else {
        // Return all text content concatenated
        all_text_content.join("\n")
    }
}
```

### Session Type Isolation

```rust
pub enum SessionType {
    User,      // Main UI sessions
    SubAgent,  // Subagent spawned sessions
    Scheduled, // Cron/schedule-triggered
    Hidden,    // Internal tool sessions
    Terminal,  // Terminal mode
}

// When creating subagent:
let session = session_manager.create_session(
    working_dir,
    "Subagent task".to_string(),
    SessionType::SubAgent,  // Hidden from main session list
).await?;
```

### Notification Streaming

```rust
pub struct ToolCallResult {
    pub result: Pin<Box<dyn Future<Output = Result<CallToolResult, ErrorData>> + Send>>,
    pub notification_stream: Option<Box<dyn Stream<Item = ServerNotification> + Send>>,
}

// Subagent execution:
let (notification_tx, notification_rx) = mpsc::unbounded_channel();

ToolCallResult {
    notification_stream: Some(Box::new(UnboundedReceiverStream::new(notification_rx))),
    result: Box::new(execute_subagent_with_notifications(...).boxed()),
}

// Inside subagent:
if let Some(ref tx) = notification_tx {
    for content in &msg.content {
        if let Some(notif) = create_tool_notification(content, session_id) {
            tx.send(notif).ok();
        }
    }
}
```

### Applicability to Hatchery

✅ **Direct Copy**:
- SubagentParams structure (rename to SwarmTaskParams)
- Extension inheritance logic
- Summary mode pattern
- Session type isolation

✅ **Enhance for Swarm**:
- Add `parent_agent_id` for hierarchy tracking
- Add `shared_memory_access: bool` flag
- Add `coordination_protocol: String` (broadcast, request-reply, etc.)
- Multi-level subagent nesting (Brood Lord mode)

---

## 4. Session Management (★★★★★)

**Location**: `crates/goose/src/session/session_manager.rs`

### Session Structure

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,                     // YYYYMMDD_N format
    pub working_dir: PathBuf,
    pub name: String,
    pub user_set_name: bool,            // User-provided vs auto-generated
    pub session_type: SessionType,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub extension_data: ExtensionData,  // Key-value blob for extensions
    pub total_tokens: Option<i32>,
    pub input_tokens: Option<i32>,
    pub output_tokens: Option<i32>,
    pub accumulated_total_tokens: Option<i32>,  // Across compactions
    pub accumulated_input_tokens: Option<i32>,
    pub accumulated_output_tokens: Option<i32>,
    pub schedule_id: Option<String>,
    pub recipe: Option<Recipe>,
    pub user_recipe_values: Option<HashMap<String, String>>,
    pub conversation: Option<Conversation>,  // Lazy-loaded
    pub message_count: usize,
    pub provider_name: Option<String>,
    pub model_config: Option<ModelConfig>,
}
```

### Builder Pattern for Updates

```rust
pub struct SessionUpdateBuilder<'a> {
    session_manager: &'a SessionManager,
    session_id: String,
    name: Option<String>,
    user_set_name: Option<bool>,
    // ... (all optional fields)
}

// Usage:
session_manager
    .update(&session_id)
    .user_provided_name("My Research")
    .total_tokens(Some(1500))
    .extension_data(data)
    .apply()
    .await?;
```

### SQLite Storage with WAL Mode

```rust
pub struct SessionStorage {
    pool: Pool<Sqlite>,
    initialized: tokio::sync::OnceCell<()>,  // Lazy init
    session_dir: PathBuf,
}

fn create_pool(path: &Path) -> Pool<Sqlite> {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .busy_timeout(Duration::from_secs(5))
        .journal_mode(SqliteJournalMode::Wal);  // Write-Ahead Logging

    SqlitePoolOptions::new().connect_lazy_with(options)
}
```

### Schema Migrations

```rust
pub const CURRENT_SCHEMA_VERSION: i32 = 7;

async fn run_migrations(pool: &Pool<Sqlite>) -> Result<()> {
    let current_version = get_schema_version(tx).await?;

    if current_version < CURRENT_SCHEMA_VERSION {
        for version in (current_version + 1)..=CURRENT_SCHEMA_VERSION {
            apply_migration(tx, version).await?;
            update_schema_version(tx, version).await?;
        }
    }

    Ok(())
}
```

### Conversation as Messages Table

```rust
// Schema:
CREATE TABLE messages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    message_id TEXT,
    session_id TEXT NOT NULL,
    role TEXT NOT NULL,
    content_json TEXT NOT NULL,
    created_timestamp INTEGER NOT NULL,
    timestamp TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    tokens INTEGER,
    metadata_json TEXT
);

CREATE INDEX idx_messages_session ON messages(session_id);
CREATE INDEX idx_messages_message_id ON messages(message_id);
```

### Atomic Conversation Replacement

```rust
async fn replace_conversation(
    &self,
    session_id: &str,
    conversation: &Conversation,
) -> Result<()> {
    let mut tx = pool.begin().await?;

    // Delete old messages
    sqlx::query("DELETE FROM messages WHERE session_id = ?")
        .bind(session_id)
        .execute(&mut *tx)
        .await?;

    // Insert new messages
    for message in conversation.messages() {
        sqlx::query(
            "INSERT INTO messages (...) VALUES (?, ?, ?, ?, ?, ?)"
        )
        .bind(message_id)
        .bind(session_id)
        .bind(role_to_string(&message.role))
        .bind(serde_json::to_string(&message.content)?)
        .bind(message.created)
        .bind(serde_json::to_string(&message.metadata)?)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    Ok(())
}
```

### Applicability to Hatchery

✅ **Direct Copy**:
- SQLite with WAL mode
- Builder pattern for updates
- Lazy conversation loading
- Migration system

✅ **Swarm Additions**:
- Add `parent_session_id` field for hierarchy
- Add `swarm_coordinator_id` field
- Store shared memory snapshots as JSONB
- Add session graph queries (descendants, siblings)

---

## 5. Extension/Tool Manager (★★★★★)

**Location**: `crates/goose/src/agents/extension_manager.rs`

### Extension Types

```rust
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type")]
pub enum ExtensionConfig {
    Stdio { cmd: String, args: Vec<String>, envs: Envs, timeout: Option<u64> },
    StreamableHttp { uri: String, headers: HashMap<String, String>, timeout: Option<u64> },
    Builtin { name: String, timeout: Option<u64> },
    Platform { name: String },  // Runs in-process
    InlinePython { code: String, dependencies: Option<Vec<String>> },
    Frontend { tools: Vec<Tool>, instructions: Option<String> },
}
```

### Tool Prefixing for Namespacing

```rust
async fn fetch_all_tools(&self, session_id: &str) -> Result<Vec<Tool>> {
    let clients = self.extensions.lock().await;

    for (name, ext, client) in clients {
        let client_tools = client.list_tools(session_id, None, cancel_token).await?;

        for tool in client_tools.tools {
            if ext.config.is_tool_available(&tool.name) {
                tools.push(Tool {
                    name: format!("{}__{}", name, tool.name).into(),  // Prefix!
                    description: tool.description,
                    input_schema: tool.input_schema,
                    // ...
                });
            }
        }
    }

    Ok(tools)
}
```

### Tool Availability Filtering

```rust
pub fn is_tool_available(&self, tool_name: &str) -> bool {
    match self {
        ExtensionConfig::Stdio { available_tools, .. } => {
            // If empty: all tools available
            // If specified: only listed tools available
            available_tools.is_empty() || available_tools.contains(&tool_name.to_string())
        }
        // ... same for other variants
    }
}
```

### Cached Tool List with Versioning

```rust
pub struct ExtensionManager {
    extensions: Mutex<HashMap<String, Extension>>,
    tools_cache: Mutex<Option<Arc<Vec<Tool>>>>,
    tools_cache_version: AtomicU64,  // Invalidation via bump
}

async fn get_all_tools_cached(&self, session_id: &str) -> Result<Arc<Vec<Tool>>> {
    {
        let cache = self.tools_cache.lock().await;
        if let Some(ref tools) = *cache {
            return Ok(Arc::clone(tools));
        }
    }

    let version_before = self.tools_cache_version.load(Ordering::SeqCst);
    let tools = Arc::new(self.fetch_all_tools(session_id).await?);

    {
        let mut cache = self.tools_cache.lock().await;
        let version_after = self.tools_cache_version.load(Ordering::SeqCst);
        if version_after == version_before && cache.is_none() {
            *cache = Some(Arc::clone(&tools));
        }
    }

    Ok(tools)
}

async fn invalidate_tools_cache_and_bump_version(&self) {
    self.tools_cache_version.fetch_add(1, Ordering::SeqCst);
    *self.tools_cache.lock().await = None;
}
```

### Resource Management (MCP)

```rust
pub async fn list_resources(
    &self,
    session_id: &str,
    params: Value,
    cancellation_token: CancellationToken,
) -> Result<Vec<Content>> {
    let extension = params.get("extension").and_then(|v| v.as_str());

    match extension {
        Some(ext_name) => {
            // Single extension
            self.list_resources_from_extension(session_id, ext_name, token).await
        }
        None => {
            // All extensions with resources capability
            let mut futures = FuturesUnordered::new();

            for name in self.extensions.lock().await.keys() {
                futures.push(async move {
                    self.list_resources_from_extension(session_id, name, token).await
                });
            }

            // Collect all results
            let mut all_resources = Vec::new();
            while let Some(result) = futures.next().await {
                match result {
                    Ok(content) => all_resources.extend(content),
                    Err(e) => tracing::error!("Resource error: {:?}", e),
                }
            }

            Ok(all_resources)
        }
    }
}
```

### Applicability to Hatchery

✅ **Direct Copy**:
- Tool prefixing pattern (`agent_name__tool_name`)
- Cached tool list with atomic versioning
- Tool availability filtering
- FuturesUnordered for parallel operations

✅ **Swarm Context**:
- Replace extensions with agent capabilities
- Add coordination tools (broadcast, request, etc.)
- Add shared memory tools (read, write, lock)

---

## 6. Event System (★★★)

**Location**: `crates/goose/src/agents/subagent_execution_tool/notification_events.rs`

### Event Types

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "subtype")]
pub enum TaskExecutionNotificationEvent {
    LineOutput { task_id: String, output: String },
    TasksUpdate { stats: TaskExecutionStats, tasks: Vec<TaskInfo> },
    TasksComplete { stats: TaskCompletionStats, failed_tasks: Vec<FailedTaskInfo> },
}

impl TaskExecutionNotificationEvent {
    pub fn to_notification_data(&self) -> serde_json::Value {
        let mut event_data = serde_json::to_value(self).unwrap();

        // Add type field at root
        if let Value::Object(ref mut map) = event_data {
            map.insert("type".to_string(), Value::String("task_execution".to_string()));
        }

        event_data
    }
}
```

### Agent Event Stream

```rust
pub enum AgentEvent {
    Message(Message),
    McpNotification(ServerNotification),
    ModelChange { provider_name: String, model_name: String },
    HistoryReplaced(Conversation),
}

// Stream usage:
let mut stream = agent.reply(user_message, session_config, cancel_token).await?;

while let Some(event) = stream.next().await {
    match event {
        Ok(AgentEvent::Message(msg)) => {
            // Forward to notification channel
            if let Some(ref tx) = notification_tx {
                for content in &msg.content {
                    if let Some(notif) = create_tool_notification(content, session_id) {
                        tx.send(notif).ok();
                    }
                }
            }
            conversation.push(msg);
        }
        Ok(AgentEvent::HistoryReplaced(updated_conversation)) => {
            conversation = updated_conversation;
        }
        Err(e) => tracing::error!("Stream error: {}", e),
    }
}
```

### Applicability to Hatchery

✅ **Adapt**:
- Define `SwarmEvent` enum (agent spawned, task complete, memory updated, etc.)
- Use mpsc channels for event distribution
- Add event filtering by subscriber

---

## 7. Key Rust Patterns to Adopt

### 1. Arc + Mutex for Shared State

```rust
pub type SharedProvider = Arc<Mutex<Option<Arc<dyn Provider>>>>;

// Double Arc allows provider swaps while maintaining concurrent access
let provider = self.provider.lock().await;
```

### 2. Builder Pattern with Typestate

```rust
impl PromptManager {
    pub fn builder(&self) -> SystemPromptBuilder<'_, Self> {
        SystemPromptBuilder {
            manager: self,
            extensions_info: vec![],
            // ... initialize all fields
        }
    }
}

// Typestate ensures compile-time validation
impl<'a> SystemPromptBuilder<'a, PromptManager> {
    pub fn build(self) -> String {
        // Can't call build() without manager reference
    }
}
```

### 3. OnceCell for Lazy Initialization

```rust
pub struct SessionStorage {
    pool: Pool<Sqlite>,
    initialized: tokio::sync::OnceCell<()>,
}

async fn pool(&self) -> Result<&Pool<Sqlite>> {
    self.initialized
        .get_or_try_init(|| async {
            // Run migrations, import legacy, etc.
            Ok(())
        })
        .await?;
    Ok(&self.pool)
}
```

### 4. Macro for Dynamic SQL Generation

```rust
macro_rules! add_update {
    ($field:expr, $name:expr) => {
        if $field.is_some() {
            if !updates.is_empty() {
                query.push_str(", ");
            }
            updates.push($name);
            query.push_str($name);
            query.push_str(" = ?");
        }
    };
}

add_update!(builder.name, "name");
add_update!(builder.total_tokens, "total_tokens");
```

### 5. Pin<Box<dyn Future>> for Async Traits

```rust
pub struct ToolCallResult {
    pub result: Pin<Box<dyn Future<Output = Result<CallToolResult, ErrorData>> + Send>>,
    pub notification_stream: Option<Box<dyn Stream<Item = ServerNotification> + Send>>,
}

// Allows returning futures from sync functions
```

---

## 8. Comparison with Hatchery

| Feature | Goose | Hatchery (Current) | Hatchery (Should Add) |
|---------|-------|-------------------|----------------------|
| Prompt Management | Builder + Templates | Manual strings | ✅ Builder + Handlebars |
| Context Compaction | 80% threshold + dual-visibility | None | ✅ Copy entire system |
| Subagent System | Nested + inheritance | Basic spawn | ✅ Add settings override |
| Session Persistence | SQLite + migrations | None | ✅ Add session storage |
| Tool Management | Prefixed + cached | Direct calls | ✅ Add namespacing |
| Event System | Typed enums + streams | None | ✅ Add SwarmEvent |
| Shared Memory | None | Basic HashMap | Keep + add snapshots |

---

## 9. Implementation Priorities for Hatchery

### Phase 1: Foundation (Week 1)
1. **Prompt Manager** — Port builder pattern, add swarm-specific templates
2. **Dual-Visibility Metadata** — Add `coordinator_visible` flag to messages
3. **Context Compaction** — Port 80% threshold + progressive tool removal

### Phase 2: Swarm Core (Week 2)
4. **Session Manager** — SQLite storage with hierarchy support
5. **Tool Prefixing** — `agent_name__tool_name` pattern
6. **Extension Inheritance** — Port subagent settings override logic

### Phase 3: Production Features (Week 3)
7. **Event System** — Typed `SwarmEvent` with mpsc channels
8. **Cached Tool Lists** — Atomic versioning pattern
9. **Migration System** — Schema versioning for long-term stability

---

## 10. Code Snippets for Direct Copy

### Prompt Builder Skeleton

```rust
pub struct SwarmPromptManager {
    base_prompt_override: Option<String>,
    swarm_extras: Vec<String>,
    timestamp: String,
}

pub struct SwarmPromptBuilder<'a> {
    manager: &'a SwarmPromptManager,
    agent_capabilities: Vec<AgentCapability>,
    shared_memory_info: Option<SharedMemoryInfo>,
    coordination_protocol: CoordinationProtocol,
    hierarchy_level: u32,
}

impl<'a> SwarmPromptBuilder<'a> {
    pub fn with_agent(mut self, cap: AgentCapability) -> Self {
        self.agent_capabilities.push(cap);
        self
    }

    pub fn with_shared_memory(mut self, info: SharedMemoryInfo) -> Self {
        self.shared_memory_info = Some(info);
        self
    }

    pub fn build(self) -> String {
        let context = SwarmPromptContext {
            agents: self.agent_capabilities,
            shared_memory: self.shared_memory_info,
            coordination: self.coordination_protocol,
            timestamp: self.manager.timestamp.clone(),
        };

        render_template("swarm_system.hbs", &context).unwrap()
    }
}
```

### Dual-Visibility Metadata

```rust
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MessageMetadata {
    pub agent_visible: bool,        // Sent to agent's LLM
    pub coordinator_visible: bool,  // Sent to coordinator's LLM
    pub user_visible: bool,         // Shown in UI
}

impl MessageMetadata {
    pub fn agent_only() -> Self {
        Self { agent_visible: true, coordinator_visible: false, user_visible: false }
    }

    pub fn coordinator_only() -> Self {
        Self { agent_visible: false, coordinator_visible: true, user_visible: false }
    }

    pub fn all_visible() -> Self {
        Self { agent_visible: true, coordinator_visible: true, user_visible: true }
    }
}
```

### Compaction Threshold Check

```rust
pub async fn check_swarm_compaction_needed(
    agent: &SwarmAgent,
    conversation: &Conversation,
) -> Result<bool> {
    const THRESHOLD: f64 = 0.8;

    let context_limit = agent.model_config.context_limit;
    let current_tokens = conversation.total_tokens();

    let usage_ratio = current_tokens as f64 / context_limit as f64;

    Ok(usage_ratio > THRESHOLD)
}
```

---

## 11. Files to Reference When Implementing

| Feature | Goose File | Start Line | Key Struct/Function |
|---------|-----------|-----------|-------------------|
| Prompt Builder | `agents/prompt_manager.rs` | 20-244 | `SystemPromptBuilder::build()` |
| Context Compaction | `context_mgmt/mod.rs` | 55-172 | `compact_messages()` |
| Dual Visibility | `conversation/message.rs` | (metadata struct) | `MessageMetadata` |
| Subagent Execution | `agents/subagent_handler.rs` | 55-213 | `run_complete_subagent_task()` |
| Session Storage | `session/session_manager.rs` | 380-1449 | `SessionStorage` |
| Tool Prefixing | `agents/extension_manager.rs` | 834-905 | `fetch_all_tools()` |
| Progressive Removal | `context_mgmt/mod.rs` | 218-266 | `filter_tool_responses()` |

---

## 12. Conclusion

Goose provides a **production-ready blueprint** for multi-agent systems in Rust. Key takeaways:

✅ **Must Copy**:
- Dual-visibility metadata (game-changer for UX)
- Prompt builder pattern
- 80% compaction threshold
- Progressive tool removal

✅ **Adapt for Swarm**:
- Add coordinator visibility layer
- Enhance subagent params with swarm-specific fields
- Add shared memory access controls
- Implement hierarchy tracking in sessions

✅ **Skip/Simplify**:
- Full MCP protocol (use simpler tool registration)
- Legacy migration system (not needed for new project)
- Recipe template system (unless adding swarm templates)

**Next Steps**:
1. Port `PromptManager` to `hatchery/prompt/`
2. Add dual-visibility to `Message` struct
3. Implement compaction in `hatchery/memory/context.rs`
4. Enhance `SwarmAgent::spawn_agent()` with Goose's subagent params
