# Compression/Compaction Strategies Evaluation for Swarm Orchestration

## Executive Summary

Analysis of 18 CLI coding agent compression strategies reveals distinct approaches to context management. This evaluation identifies patterns applicable to swarm orchestration systems, rating each on suitability for multi-agent coordination.

**Key Finding**: Most agents use inline runtime compaction with token-budget triggers. The best patterns for swarms combine:
1. Non-destructive projection (Roo Code, Goose)
2. Dual visibility layers (user/agent, Goose)
3. Pluggable context strategies (AutoGen, LangGraph)
4. Async summarization (Plandex, AutoGPT)

---

## 1. Trigger Mechanisms

### Token Budget Threshold (EXCELLENT IDEA)

**Systems**: Continue CLI, Crush, Goose, Cline, OpenCode, Roo Code, Open Interpreter

**Pattern**: Auto-compaction triggers when `current_tokens >= context_window * threshold_ratio`

**Why Excellent for Swarms**:
- Predictable resource management across multiple agents
- Prevents cascading OOM failures in swarm
- Enables budget allocation per agent role
- Threshold tunable per agent type (coordinator vs worker)

**Implementation Notes**:
- Continue CLI: `0.8 * contextWindow`, configurable per profile
- Crush: Large windows (>200k) use 20k buffer, small use 20% ratio
- Goose: `GOOSE_AUTO_COMPACT_THRESHOLD=0.8`, env-configurable
- Open Interpreter: `context_window - max_tokens - 25` safety margin

**Swarm Application**:
```rust
// Per-agent budget allocation
struct AgentBudget {
    context_window: usize,
    threshold: f32,  // 0.8 for workers, 0.9 for coordinator
    reserved_tokens: usize,
}
```

### Reactive Overflow Exception (GOOD IDEA)

**Systems**: CrewAI, OpenClaw, OpenHands

**Pattern**: Catch context-limit exceptions from LLM provider, trigger recovery compaction

**Why Good**:
- Last-resort safety net when predictions fail
- Useful for heterogeneous swarms with unknown budgets
- Automatic degradation vs hard crash

**Why Not Excellent**:
- Wastes API calls on failed attempts
- Increases latency (retry overhead)
- Can cascade in swarm if multiple agents hit simultaneously

**Swarm Application**: Use as fallback, not primary trigger. Implement circuit breaker to prevent storm.

### Event-Driven Per-Step (GOOD IDEA for specific roles)

**Systems**: AutoGPT

**Pattern**: After every action execution, summarize episode into `episode.summary`

**Why Good for Coordinators**:
- Clean episodic memory for orchestration
- Progressive summarization prevents sudden compaction
- Maintains action causality chain

**Why Not Universal**:
- Overhead for high-frequency agents (data fetchers)
- Over-summarization risk (summary-of-summary)

**Swarm Application**: Use for coordinator agents only. Workers use token-budget triggers.

### Manual Command (IRRELEVANT for autonomous swarms)

**Systems**: Goose, Continue, OpenCode

**Pattern**: User-triggered `/compact` or `/summarize` command

**Why Irrelevant**: Swarms must be autonomous. Manual triggers require operator intervention.

**Exception**: Useful for debugging/admin tools to force compaction during testing.

---

## 2. Compaction Algorithms

### LLM-Based Semantic Summarization (EXCELLENT IDEA)

**Systems**: Aider, CAMEL, Cline, Goose, AutoGPT, OpenClaw, OpenCode

**Pattern**: Use LLM to generate semantic summary of old context, insert as anchor

**Why Excellent**:
- Preserves causal relationships better than naive truncation
- Compresses information density (100k tokens → 2k summary)
- Maintains task continuity across long sessions

**Best Practices from Analysis**:

1. **Dedicated Summarizer Model** (Goose):
   - Use fast/cheap model for summarization (GPT-4o-mini vs GPT-4)
   - Prevents recursive context blow-up

2. **Structured Summary Prompt** (AutoGPT):
   - Current State / Files Changed / Technical Context / Next Steps
   - Not just "what happened" but "what matters going forward"

3. **Progressive Fallback** (Goose):
   - If summary itself overflows context, progressively remove tool outputs
   - Fallback percentages: 10%, 20%, 50%, 100% tool-result removal

4. **Summary Anchoring** (Crush, OpenClaw):
   - `SummaryMessageID` marks boundary
   - Future loads fetch `messages[summaryIndex:]` only
   - Atomic cutover, no gradual drift

**Swarm Application**:
```rust
// Shared summarizer service for swarm
struct SwarmSummarizer {
    model: FastLLM,  // Separate from main agents
    prompt_templates: HashMap<AgentRole, PromptTemplate>,
}

// Per-agent summary storage
struct AgentMemory {
    raw_history: Vec<Message>,
    summary_anchor: Option<SummaryAnchor>,
    messages_after_anchor: Vec<Message>,
}
```

### Head + Tail with Placeholder (GOOD IDEA)

**Systems**: AutoGen, SWE-agent

**Pattern**: Keep first N and last M messages, insert `"Skipped X messages"` placeholder

**Why Good**:
- Simple, deterministic
- No LLM cost for compaction
- Useful for short-lived agents

**Why Not Excellent**:
- Loses middle context (may contain critical info)
- Placeholder adds no semantic value
- Not suitable for long-running coordinators

**Swarm Application**: Use for ephemeral worker agents with <50 turns. Coordinators need semantic summarization.

### Token-Limited Pruning (GOOD IDEA)

**Systems**: AutoGen, MetaGPT

**Pattern**: Remove middle messages until under budget, keep system + recent

**MetaGPT Variants**:
- `POST_CUT_BY_MSG`: Keep newest messages
- `PRE_CUT_BY_MSG`: Keep oldest messages
- `POST_CUT_BY_TOKEN`: Truncate boundary message content to fit
- `PRE_CUT_BY_TOKEN`: Similar, from start

**Why Good**:
- Zero LLM cost
- Instant compaction
- Configurable policy (keep recent vs keep early)

**Why Not Excellent**:
- Hard cutoff loses information
- No semantic compression
- Can break tool-call chains

**Swarm Application**: Use for data-fetching agents where history is less critical. Combine with tool-output truncation.

### Tool Output Truncation (EXCELLENT IDEA)

**Systems**: Open Interpreter, OpenCode, Goose

**Pattern**: Truncate large tool outputs (stdout, API responses) before storing in history

**Open Interpreter**:
- `max_observation_length = 100_000` chars
- Head + tail + `"... (middle truncated) ..."` guidance
- `MAX_RESPONSE_LEN = 16_000` for shell output

**OpenCode**:
- After compaction, `prune(...)` marks old tool outputs
- `PRUNE_PROTECT = 40_000`, `PRUNE_MINIMUM = 20_000`
- Materialization replaces with `"[Old tool result content cleared]"`

**Goose**:
- Tool-pair summarization: summarize `tool_request + tool_response` into agent-only message
- Original marked `agent_invisible`, summary is `agent_only`

**Why Excellent for Swarms**:
- Prevents single bloated API response from killing agent
- Most tool outputs are low-semantic-value (JSON dumps, log spam)
- Orthogonal to conversation compaction (can combine)

**Swarm Application**:
```rust
enum ToolOutputHandling {
    Truncate { max_chars: usize },
    Summarize { llm: FastLLM },
    Hash { show_preview: usize },  // "SHA256: abc... (500 lines omitted)"
}
```

### Sliding Window (BAD IDEA for stateful agents)

**Systems**: Buffered contexts (AutoGen), Roo Code fallback

**Pattern**: Keep only last N messages/tokens, discard rest

**Why Bad for Swarms**:
- Complete amnesia of early context
- Breaks long-running task continuity
- Coordinators lose delegation history

**Exception**: Acceptable for stateless worker agents (e.g., "fetch price from Binance" agent).

---

## 3. Visibility & Hiding Mechanisms

### Dual Visibility Flags (EXCELLENT IDEA)

**Systems**: Goose

**Pattern**: Each message has `user_visible` and `agent_visible` flags

**Why Excellent for Swarms**:
- Agent sees internal reasoning, user sees clean output
- Coordinator can hide inter-agent messages from workers
- Enables "private channel" between orchestrator and specific agents
- Supports selective replay for debugging

**Goose Implementation**:
- Compaction marks old messages `agent_invisible=true`
- Tool-pair summaries are `agent_only=true` (invisible to user)
- Continuation messages restore last user message for context

**Swarm Application**:
```rust
struct SwarmMessage {
    content: String,
    visibility: MessageVisibility,
}

enum MessageVisibility {
    Public,                    // User + all agents
    AgentOnly,                 // All agents, hidden from user
    Coordinator,               // Only coordinator
    Private(Vec<AgentId>),    // Specific agents
}
```

### Tag-Based Projection (EXCELLENT IDEA)

**Systems**: Roo Code, Cline

**Pattern**: Messages stay in storage, marked with `condenseParent` or `truncationParent` ID; effective view filters them out

**Roo Code**:
- Summary has unique `condenseId`
- Old messages tagged `condenseParent = condenseId`
- `getEffectiveApiHistory()` filters by tag
- Rewind/delete summary → old messages reappear (non-destructive)

**Cline**:
- `conversationHistoryDeletedRange` tracks hidden span
- `contextHistoryUpdates` persisted rewrites log
- Structural repair after hiding (fix tool_use/tool_result chains)

**Why Excellent for Swarms**:
- Full auditability (nothing truly deleted)
- Reversible compaction for debugging
- Time-travel debugging (restore to any summary point)
- Compliance/observability (all raw data retained)

**Swarm Application**:
```rust
struct MessageTag {
    compaction_id: Option<Uuid>,  // Which compaction hid this
    visible_to: VisibilitySet,
    timestamp: Timestamp,
}

fn effective_history(
    raw: &[Message],
    active_compactions: &[Uuid]
) -> Vec<Message> {
    raw.iter()
        .filter(|m| !m.tag.compaction_id.map_or(false, |id| active_compactions.contains(&id)))
        .cloned()
        .collect()
}
```

### Range Masking (GOOD IDEA)

**Systems**: Cline, LangGraph

**Pattern**: Track deleted message ID ranges, skip when building prompt

**Cline**:
- `conversationHistoryDeletedRange: {start, end}`
- `getAndAlterTruncatedMessages()` skips range
- Updates persisted to disk

**LangGraph**:
- `RemoveMessage(id=...)` or `RemoveMessage(id=REMOVE_ALL_MESSAGES)`
- `add_messages` reducer filters by ID

**Why Good**:
- Explicit boundary tracking
- Works with state-machine models (LangGraph)

**Why Not Excellent vs Tag-Based**:
- Range can fragment (multiple compactions)
- Less flexible than tag filtering
- Harder to implement partial unhiding

**Swarm Application**: Use tag-based for swarms (more flexible). Range masking for simple single-agent cases.

### In-Place Mutation (BAD IDEA)

**Systems**: CrewAI

**Pattern**: `messages.clear()` then append summary, overwrite original list

**Why Bad**:
- Irreversible data loss
- No audit trail
- Impossible to debug "what did the agent see before compaction?"
- Breaks time-travel debugging

**Swarm Application**: AVOID. Always use non-destructive projection.

---

## 4. Initiator & Execution Model

### Inline Runtime Loop (EXCELLENT IDEA)

**Systems**: Most systems (Cline, Goose, Crush, Continue, etc.)

**Pattern**: Same agent runtime loop detects threshold → triggers compaction → continues

**Why Excellent for Swarms**:
- No external orchestration dependency
- Agent self-manages context budget
- Continues task after compaction (no restart)

**Cline**:
1. Loop calculates `shouldCompact`
2. Adds `summarize_task` to user content
3. Model returns `summarize_task` tool-call
4. `SummarizeTaskHandler` executes compaction
5. Loop resumes with compacted context

**Goose**:
1. `check_if_compaction_needed()` at step end
2. If true → call `compact_messages(..., manual_compact=false)`
3. Store summary, mark old messages `agent_invisible`
4. Emit `AgentEvent::HistoryReplaced`
5. Continue turn

**Swarm Application**: Each agent runs inline compaction. Coordinator observes compaction events for budget tracking.

### Background Async Summarization (EXCELLENT IDEA for high-throughput)

**Systems**: Plandex, AutoGPT

**Pattern**: Current turn uses previous summary; new summary generated in background for next turn

**Plandex**:
1. Prompt assembly: check if existing summaries fit budget
2. Select summary, use it in current request
3. After reply stored → `summarizeConvo(...)` in goroutine
4. Next turn has fresh summary available

**AutoGPT**:
1. After action execution, `handle_compression(...)`
2. Parallel `asyncio.gather` for unsummarized episodes
3. Store summaries in `episode.summary`
4. Next cycle uses summaries for older episodes

**Why Excellent for Swarms**:
- Doesn't block critical path (agent continues immediately)
- Amortizes summarization cost across turns
- High-throughput agents aren't throttled by compaction

**Trade-off**: One-turn lag (turn N uses summary of turn N-2). Acceptable for most use cases.

**Swarm Application**:
```rust
// Coordinator spawns background summarizer tasks
async fn maybe_summarize_background(&self, agent_id: AgentId) {
    if self.should_compact(agent_id) {
        let history = self.get_raw_history(agent_id);
        tokio::spawn(async move {
            let summary = summarize_llm(history).await;
            self.store_summary(agent_id, summary).await;
        });
    }
}
```

### Pluggable Context Strategy (EXCELLENT IDEA)

**Systems**: AutoGen, LangGraph, SWE-agent

**Pattern**: Context management is abstract interface, runtime selects implementation

**AutoGen**:
```python
UnboundedChatCompletionContext  # No limits
BufferedChatCompletionContext   # Last N
HeadAndTailChatCompletionContext  # First M + Last N
TokenLimitedChatCompletionContext  # Budget-based pruning
```

**SWE-agent**:
```python
DefaultHistoryProcessor         # No-op
LastNObservations              # Keep N recent outputs
ClosedWindowHistoryProcessor   # Compress repeated file windows
RemoveRegex                     # Regex-based filtering
CacheControlHistoryProcessor   # Prompt caching optimization
```

**Why Excellent for Swarms**:
- Different agent roles need different strategies
- Coordinator: semantic summarization
- Data fetcher: sliding window
- Code analyzer: tool-output truncation
- Easy to A/B test strategies

**Swarm Application**:
```rust
trait ContextStrategy {
    fn compact(&self, history: &[Message]) -> CompactionResult;
    fn effective_history(&self, history: &[Message]) -> Vec<Message>;
}

struct AgentConfig {
    role: AgentRole,
    strategy: Box<dyn ContextStrategy>,
}

// Factory pattern
fn strategy_for_role(role: AgentRole) -> Box<dyn ContextStrategy> {
    match role {
        AgentRole::Coordinator => Box::new(SemanticSummaryStrategy::default()),
        AgentRole::DataFetcher => Box::new(SlidingWindowStrategy { window: 20 }),
        AgentRole::ToolUser => Box::new(ToolOutputTruncationStrategy::default()),
    }
}
```

---

## 5. Persistence & State Management

### Separate Storage for Raw + Summary (EXCELLENT IDEA)

**Systems**: Plandex, Roo Code, Goose

**Pattern**: Raw messages stored indefinitely, summaries in separate table/structure

**Plandex**:
- `conversation_messages` table (all raw messages)
- `convo_summaries` table (summaries with boundary markers)
- Prompt assembly selects summary + tail query

**Roo Code**:
- `api_conversation_history.json` (all messages with tags)
- Summary messages have `isSummary: true`, unique `condenseId`
- Effective view filtered by `condenseParent` tag

**Why Excellent for Swarms**:
- Auditability (regulatory compliance)
- Debugging (time-travel to any point)
- Recompaction (re-summarize with better prompt)
- Disaster recovery (restore from raw if summary corrupted)

**Swarm Application**:
```rust
struct SwarmPersistence {
    raw_messages: MessageLog,      // Append-only
    summaries: SummaryStore,        // Keyed by compaction_id
    agent_state: HashMap<AgentId, AgentState>,
}

struct SummaryStore {
    db: sled::Db,  // Key: compaction_id, Value: Summary
}

// Query pattern
fn build_prompt(&self, agent_id: AgentId) -> Vec<Message> {
    let state = self.agent_state.get(agent_id);
    if let Some(anchor) = state.summary_anchor {
        let summary_msg = self.summaries.get(anchor.compaction_id);
        let tail = self.raw_messages.after(anchor.boundary_timestamp);
        vec![summary_msg].into_iter().chain(tail).collect()
    } else {
        self.raw_messages.all_for_agent(agent_id)
    }
}
```

### Event-Sourced Compaction (GOOD IDEA)

**Systems**: OpenHands, LangGraph

**Pattern**: Compaction is event in log; view projection skips forgotten IDs

**OpenHands**:
- `CondensationAction` event contains `forgotten_event_ids` + `summary`
- `View.from_events()` filters forgotten IDs
- Inserts `AgentCondensationObservation(summary)` at boundary

**LangGraph**:
- `RemoveMessage(id=...)` is state update
- `add_messages` reducer processes removal
- State checkpoints include current visible set

**Why Good**:
- Fits event-sourcing architecture
- Compaction is versioned (can rollback)
- Replay-safe

**Why Not Excellent**:
- More complex than simple tag filtering
- Requires event log infrastructure

**Swarm Application**: Use if swarm already event-sourced. Otherwise, tag-based is simpler.

---

## 6. Anti-Patterns to Avoid

### Recursive Summary-of-Summary (BAD IDEA)

**Observed in**: CAMEL (without proper guards)

**Problem**: Summarize → summary grows → summarize summary → information decay

**Solution**:
- Full compaction mode resets token count (CAMEL does this)
- Or max depth limit (e.g., 3 compactions → hard reset)

**Swarm Application**: Track compaction depth per agent, force full reset after N iterations.

### Character-Based Chunking for Token Budgets (BAD IDEA)

**System**: CrewAI

**Problem**: Chunks context by character length (`cut_size = context_window`), not actual tokens

**Why Bad**:
- Token/char ratio varies (code vs prose)
- Can still overflow despite "fitting" char budget
- Wastes budget (over-conservative estimate)

**Swarm Application**: Always use proper tokenizer. Cache tokenization results.

### Disable Auto-Compaction by Default (BAD IDEA for production)

**System**: OpenCode (requires manual enable)

**Why Bad for Swarms**:
- Agents will crash on long tasks
- Requires operator intervention
- Defeats autonomy

**Exception**: Useful for debugging (reproduce exact overflow scenario)

**Swarm Application**: Auto-compaction ON by default, with kill-switch for debugging.

### No Compaction Events (BAD IDEA for orchestration)

**Systems**: Several don't emit explicit events

**Why Bad for Swarms**:
- Coordinator can't track agent memory pressure
- No telemetry for optimization
- Hard to debug "why did agent forget X?"

**Swarm Application**: Always emit structured events:
```rust
enum SwarmEvent {
    CompactionStarted { agent_id: AgentId, reason: CompactionTrigger },
    CompactionComplete {
        agent_id: AgentId,
        tokens_before: usize,
        tokens_after: usize,
        summary_id: Uuid,
    },
    CompactionFailed { agent_id: AgentId, error: String },
}
```

---

## 7. Swarm-Specific Considerations

### Inter-Agent Message Compaction (NEW PROBLEM)

**Challenge**: Swarm has coordinator ↔ worker messages, not just user ↔ agent

**Solution from Analysis**: Combine Goose dual-visibility + Roo Code tagging

```rust
struct SwarmMessage {
    content: String,
    sender: AgentId,
    recipients: Vec<AgentId>,
    visibility: MessageVisibility,
    compaction_tag: Option<CompactionId>,
}

// Agent-specific effective history
fn effective_history_for_agent(
    raw: &[SwarmMessage],
    agent_id: AgentId,
    active_compactions: &HashMap<AgentId, CompactionId>
) -> Vec<SwarmMessage> {
    raw.iter()
        .filter(|m| {
            // Visible to this agent
            m.recipients.contains(&agent_id) || m.visibility == MessageVisibility::Public
        })
        .filter(|m| {
            // Not compacted for this agent
            !m.compaction_tag.map_or(false, |tag| {
                active_compactions.get(&agent_id) == Some(&tag)
            })
        })
        .cloned()
        .collect()
}
```

### Budget Allocation Across Swarm (NEW PROBLEM)

**Challenge**: Fixed total context budget, N agents need shares

**Solution**: Dynamic allocation based on agent activity

```rust
struct SwarmBudget {
    total_tokens: usize,
    per_agent: HashMap<AgentId, AgentBudget>,
}

impl SwarmBudget {
    fn allocate(&mut self, usage: &HashMap<AgentId, usize>) {
        let total_used: usize = usage.values().sum();

        for (agent_id, agent_budget) in &mut self.per_agent {
            let agent_usage = usage.get(agent_id).unwrap_or(&0);

            // Active agents get more budget
            if *agent_usage > agent_budget.allocated * 0.8 {
                agent_budget.allocated = min(
                    agent_budget.allocated * 1.5,
                    self.total_tokens / self.per_agent.len()
                );
            }

            // Idle agents budget shrinks
            if *agent_usage < agent_budget.allocated * 0.2 {
                agent_budget.allocated *= 0.8;
            }
        }
    }
}
```

### Compaction Coordination (NEW PROBLEM)

**Challenge**: Multiple agents compacting simultaneously → thundering herd on summarizer LLM

**Solution**: Queue + rate limiting from OpenClaw retry semantics

```rust
struct CompactionCoordinator {
    queue: async_channel::Receiver<CompactionRequest>,
    in_flight: HashSet<AgentId>,
    max_concurrent: usize,
}

impl CompactionCoordinator {
    async fn process_queue(&mut self) {
        while let Ok(req) = self.queue.recv().await {
            // Rate limiting
            while self.in_flight.len() >= self.max_concurrent {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }

            self.in_flight.insert(req.agent_id);

            let coordinator = self.clone();
            tokio::spawn(async move {
                let summary = summarize_llm(req.history).await;
                coordinator.store_summary(req.agent_id, summary).await;
                coordinator.in_flight.remove(&req.agent_id);
            });
        }
    }
}
```

---

## 8. Recommended Architecture for Swarms

### Core Components

```rust
// 1. Pluggable strategy per agent role
trait CompactionStrategy: Send + Sync {
    async fn compact(&self, history: &[Message]) -> CompactionResult;
    fn should_compact(&self, usage: &TokenUsage) -> bool;
}

// 2. Tag-based non-destructive storage
struct MessageStore {
    raw: AppendOnlyLog<Message>,
    summaries: HashMap<CompactionId, Summary>,
    agent_anchors: HashMap<AgentId, Option<SummaryAnchor>>,
}

// 3. Dual visibility projection
fn effective_history(
    agent_id: AgentId,
    store: &MessageStore,
    visibility_filter: impl Fn(&Message, AgentId) -> bool,
) -> Vec<Message> {
    let anchor = store.agent_anchors.get(&agent_id).unwrap();

    let messages = if let Some(anchor) = anchor {
        let summary_msg = store.summaries.get(&anchor.compaction_id).unwrap();
        let tail = store.raw.after(anchor.boundary);
        vec![summary_msg.clone()].into_iter().chain(tail).collect()
    } else {
        store.raw.all()
    };

    messages.into_iter()
        .filter(|m| visibility_filter(m, agent_id))
        .collect()
}

// 4. Background async summarization
struct BackgroundSummarizer {
    queue: mpsc::Sender<SummarizationTask>,
    model: FastLLM,
}

impl BackgroundSummarizer {
    async fn run(&self) {
        while let Some(task) = self.queue.recv().await {
            let summary = self.model.summarize(task.history).await;
            task.result_tx.send(summary).await;
        }
    }
}

// 5. Budget-based trigger with overflow fallback
struct CompactionTrigger {
    threshold: f32,  // 0.8
    last_check: Instant,
    overflow_count: u32,
}

impl CompactionTrigger {
    fn should_compact(&mut self, usage: &TokenUsage) -> bool {
        let ratio = usage.total() as f32 / usage.context_window as f32;

        // Primary: threshold trigger
        if ratio >= self.threshold {
            return true;
        }

        // Fallback: overflow exception (with backoff)
        if usage.overflow_detected && self.overflow_count < 3 {
            self.overflow_count += 1;
            return true;
        }

        false
    }
}

// 6. Event emission for observability
enum CompactionEvent {
    Triggered { agent_id: AgentId, reason: TriggerReason, tokens: usize },
    Started { agent_id: AgentId, compaction_id: CompactionId },
    Completed { agent_id: AgentId, compaction_id: CompactionId,
                tokens_before: usize, tokens_after: usize },
    Failed { agent_id: AgentId, error: String },
}
```

### Integration Example

```rust
struct SwarmAgent {
    id: AgentId,
    role: AgentRole,
    store: Arc<MessageStore>,
    strategy: Box<dyn CompactionStrategy>,
    trigger: CompactionTrigger,
    summarizer: BackgroundSummarizer,
    event_bus: EventBus,
}

impl SwarmAgent {
    async fn step(&mut self, input: Message) -> Result<Message> {
        // 1. Get effective history for this agent
        let history = effective_history(
            self.id,
            &self.store,
            |msg, agent_id| msg.visible_to(agent_id)
        );

        // 2. Check if compaction needed
        let usage = self.count_tokens(&history);
        if self.trigger.should_compact(&usage) {
            self.event_bus.emit(CompactionEvent::Triggered {
                agent_id: self.id,
                reason: TriggerReason::Threshold,
                tokens: usage.total(),
            });

            // 3. Trigger background summarization
            let (tx, rx) = oneshot::channel();
            self.summarizer.queue.send(SummarizationTask {
                agent_id: self.id,
                history: history.clone(),
                result_tx: tx,
            }).await?;

            // 4. Continue with current effective history (async summary)
            // Next turn will use the summary
        }

        // 5. Execute agent step with effective history
        let response = self.llm.complete(&history, &input).await?;

        // 6. Store raw message (never deleted)
        self.store.raw.append(Message {
            sender: self.id,
            content: response.clone(),
            timestamp: Utc::now(),
            visibility: MessageVisibility::Public,
            compaction_tag: None,
        });

        Ok(response)
    }
}
```

---

## 9. Summary Rating Table

| System | Trigger | Algorithm | Hiding | Rating | Best For |
|--------|---------|-----------|--------|--------|----------|
| Aider | Threshold | LLM Summary | Summary anchor | GOOD | Single-agent CLI |
| AutoGen | Pluggable | Buffer/Head+Tail/Token | View projection | EXCELLENT | Multi-agent frameworks |
| AutoGPT | Per-episode | LLM Summary | Prompt substitution | EXCELLENT | Autonomous agents |
| CAMEL | Threshold + adaptive | Progressive/Full summary | Memory rewrite | GOOD | Research agents |
| Cline | Threshold | LLM Summary | Range masking + rewrites | EXCELLENT | VSCode agents |
| Continue CLI | Threshold | LLM Summary | Tag-based | EXCELLENT | CLI coding |
| Continue IDE | Manual | LLM Summary | Summary anchor | GOOD | IDE extensions |
| CrewAI | Overflow | Chunked summary | In-place mutation | BAD | (avoid pattern) |
| Crush | Threshold + overflow | LLM Summary | Summary anchor | GOOD | Go-based agents |
| Goose | Threshold + manual | LLM Summary + tool-pair | Dual visibility flags | EXCELLENT | Production swarms |
| LangGraph | User-driven | State updates | Reducer filtering | EXCELLENT | Workflow systems |
| MetaGPT | Pre-send | Token/msg cut | Transport-layer | GOOD | Workflow agents |
| Open Interpreter | Per-request | Token trim + image | Inline pruning | GOOD | REPL agents |
| OpenClaw | Overflow + threshold | Semantic summary | Anchor + transcript | GOOD | Embedded agents |
| OpenCode | Overflow | LLM Summary + tool prune | Tag-based | EXCELLENT | Code agents |
| OpenHands | Overflow + threshold | Event-sourced condensation | Forgotten IDs | GOOD | SWE agents |
| Plandex | Threshold | Async summary selection | Prompt substitution | EXCELLENT | Planning agents |
| Roo Code | Threshold | LLM Summary | Tag-based projection | EXCELLENT | VSCode swarms |
| SWE-agent | Per-step | Composable processors | Processor pipeline | EXCELLENT | Task agents |

---

## 10. Final Recommendations for Hatchery Swarm

### MUST ADOPT (EXCELLENT IDEAS)

1. **Token threshold triggers (0.8-0.9 ratio)** — Goose, Continue, Crush pattern
2. **LLM semantic summarization** — with dedicated fast model
3. **Tag-based non-destructive projection** — Roo Code pattern
4. **Dual visibility (user/agent)** — Goose pattern, extend to inter-agent
5. **Pluggable context strategies** — AutoGen/SWE-agent pattern, per agent role
6. **Background async summarization** — Plandex pattern, non-blocking
7. **Separate raw + summary storage** — full auditability
8. **Tool output truncation** — orthogonal to conversation compaction
9. **Compaction events** — observability for coordinator

### CONSIDER (GOOD IDEAS)

10. **Head + tail for ephemeral workers** — simple, zero-cost
11. **Overflow exception fallback** — safety net, not primary
12. **Progressive tool-output removal** — Goose fallback strategy
13. **Event-sourced compaction** — if already using event sourcing

### AVOID (BAD IDEAS)

14. In-place mutation (CrewAI pattern)
15. Character-based chunking (CrewAI)
16. Pure sliding window for stateful agents
17. Manual-only compaction for autonomous agents
18. Recursive summary-of-summary without guards

### SWARM-SPECIFIC ADDITIONS

19. **Inter-agent message filtering** — extend visibility model
20. **Dynamic budget allocation** — based on agent activity
21. **Compaction coordinator** — rate limiting, queue management
22. **Per-role strategies** — coordinator vs worker vs data-fetcher

---

## 11. Code Snippet Library

### Threshold Trigger (from Goose)
```rust
const DEFAULT_THRESHOLD: f32 = 0.8;

fn should_compact(usage: &TokenUsage, threshold: f32) -> bool {
    if threshold <= 0.0 || threshold >= 1.0 {
        return false;  // Disabled
    }

    let total = usage.input_tokens + usage.output_tokens +
                usage.cache_read + usage.cache_write;
    let ratio = total as f32 / usage.context_window as f32;

    ratio >= threshold
}
```

### Summary Anchor (from Crush)
```rust
struct AgentSession {
    messages: Vec<Message>,
    summary_anchor: Option<SummaryAnchor>,
}

struct SummaryAnchor {
    message_id: Uuid,
    summary_text: String,
    tokens_before: usize,
    tokens_after: usize,
}

fn get_effective_messages(session: &AgentSession) -> Vec<Message> {
    if let Some(anchor) = &session.summary_anchor {
        let idx = session.messages.iter()
            .position(|m| m.id == anchor.message_id)
            .unwrap();

        session.messages[idx..].to_vec()
    } else {
        session.messages.clone()
    }
}
```

### Tag-Based Projection (from Roo Code)
```rust
struct Message {
    id: Uuid,
    content: String,
    compaction_parent: Option<Uuid>,  // Hidden by this compaction
    is_summary: bool,
    condense_id: Option<Uuid>,  // If this is a summary
}

fn effective_history(messages: &[Message]) -> Vec<Message> {
    // Find active summary (last one)
    let active_summary = messages.iter()
        .filter(|m| m.is_summary)
        .last();

    if let Some(summary) = active_summary {
        messages.iter()
            .filter(|m| {
                // Keep if not hidden by active summary
                m.compaction_parent != summary.condense_id
            })
            .cloned()
            .collect()
    } else {
        messages.to_vec()
    }
}
```

### Dual Visibility (from Goose)
```rust
struct Message {
    content: String,
    user_visible: bool,
    agent_visible: bool,
}

fn user_view(messages: &[Message]) -> Vec<Message> {
    messages.iter()
        .filter(|m| m.user_visible)
        .cloned()
        .collect()
}

fn agent_view(messages: &[Message]) -> Vec<Message> {
    messages.iter()
        .filter(|m| m.agent_visible)
        .cloned()
        .collect()
}
```

### Async Summarization (from Plandex)
```go
// After reply stored, summarize in background
go func() {
    summary := summarizeConvo(history, prevSummary)
    storeSummary(summary)
}()

// Next turn selects best summary
func selectSummary(summaries []Summary, budget int) *Summary {
    for _, s := range summaries {
        newTokens := (currentTokens - s.CoveredTokens) + s.SummaryTokens
        if newTokens < budget {
            return &s
        }
    }
    return nil
}
```

---

## Conclusion

The analysis reveals convergence on key patterns:

1. **Token thresholds (80-90%)** are universal best practice
2. **LLM summarization** beats naive truncation for semantic preservation
3. **Non-destructive storage** enables debugging and compliance
4. **Dual visibility** separates user UX from agent memory
5. **Async summarization** prevents blocking critical path

For **Hatchery swarm orchestration**, adopt:
- Goose dual-visibility model (extend to inter-agent)
- Roo Code tag-based projection (auditability)
- Plandex async summarization (throughput)
- AutoGen pluggable strategies (per-role optimization)
- SWE-agent composable processors (flexibility)

Avoid CrewAI's destructive mutation and character-based chunking. Implement swarm-specific budget allocation and compaction coordination to prevent thundering herd.

Final implementation should combine best-of-breed patterns into unified architecture with clear separation of concerns: trigger detection, strategy selection, summarization execution, and view projection.
