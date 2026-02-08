# CLI Agent Prompting Systems: Evaluation for Swarm Orchestration

**Analysis Date**: 2026-02-08
**Scope**: 18 CLI coding agent implementations
**Purpose**: Extract CORRECT and USEFUL patterns for building swarm orchestration systems

---

## Executive Summary

After analyzing 18 production CLI coding agents, the following architectural patterns emerge:

**EXCELLENT IDEAS (Must Adopt)**:
1. Layered prompt composition (not monolithic strings)
2. Mode/role-specific prompt variants
3. Dynamic context injection points
4. Filesystem-based instruction override (AGENTS.md, .cursorrules)
5. Separate system/user channel semantics
6. Tool-driven lazy-loading of context (skills)
7. Model-capability gates for prompt adaptation

**GOOD IDEAS (Consider)**:
1. Structured output via prompt injection fallback
2. History processors as prompt middleware
3. Cache-aware prompt segmentation
4. Runtime prompt mutation APIs

**BAD IDEAS (Avoid)**:
1. Tight coupling between prompt format and parsers
2. Overly fragmented prompt logic across many modules
3. Hard-coded ReAct textual protocols
4. Blind concatenation of all instructions

---

## 1. AIDER — Mode-Driven Prompt Packs

### Architecture
- Class-based prompt packs per edit mode (Ask/Architect/EditBlock/UnifiedDiff/WholeFile)
- Dynamic formatter fills placeholders (`{fence}`, `{shell_cmd_prompt}`, `{language}`, `{platform}`)
- Code context as synthetic conversation chunks (repo map, files)
- Model-specific strategy flags (`use_system_prompt`, `reminder`, `examples_as_sys_msg`)

### Evaluation

**EXCELLENT IDEA — Prompt packs bound to modes**
- Clear separation: different tasks need different instruction sets
- For swarms: each agent role should have its own prompt pack
- Implementation: `ResearchAgentPrompts`, `ImplementerPrompts`, `CoordinatorPrompts`

**EXCELLENT IDEA — Runtime formatting engine**
- Don't hardcode OS/shell/language specifics
- Inject at runtime from environment detection
- For swarms: format based on worker capabilities, target language, execution context

**EXCELLENT IDEA — Code context as conversation chunks**
- Not as tool-call payload, but as user/assistant dialogue
- Enables better model understanding with acknowledgements
- For swarms: worker reports can be framed as assistant messages with coordinator acknowledgements

**GOOD IDEA — Model-specific reminder strategy**
- `reminder="sys"` vs `reminder="user"` based on model family
- For swarms: adapt prompting to coordinator model capabilities

**BAD IDEA — Complex mode switching with history summarization**
- Switching edit formats triggers summarization to avoid contamination
- Too complex for swarm workers that should have stable roles
- Alternative: dedicated workers per task type, no switching

**Adoption Path**:
```rust
// hatchery/prompts/
pub struct PromptPack {
    main_system: String,
    system_reminder: Option<String>,
    example_messages: Vec<Message>,
    // ... per-role fields
}

pub fn get_prompt_pack(role: AgentRole) -> PromptPack {
    match role {
        AgentRole::ResearchAgent => research_prompts(),
        AgentRole::RustImplementer => rust_implementer_prompts(),
        // ...
    }
}
```

---

## 2. AUTOGEN — Stacked Prompt Surfaces

### Architecture
- Multi-layer: agent (system/context), team-manager (routing), model-compat, context window
- AssistantAgent: system messages + model_context + tool protocol + reflection
- Memory injection as explicit prompt text (not hidden)
- Different team types use different prompts (SelectorGroupChat vs Swarm vs Magentic-One)

### Evaluation

**EXCELLENT IDEA — Memory injection is prompt-level**
- Memory.update_context() modifies model_context before inference
- Appends as SystemMessage: "Relevant memory content (in chronological order): ..."
- For swarms: SharedMemory updates should explicitly modify worker prompts, not be opaque

**EXCELLENT IDEA — Tool/handoff as first-class prompt contract**
- Tools + handoff tools sent in model schema
- Runtime executes and appends FunctionExecutionResultMessage to context
- For swarms: handoff protocol should be explicit in prompts, not implicit routing

**EXCELLENT IDEA — Team-specific prompt layers**
- SelectorGroupChat has selector prompt (role-play style with {roles}, {participants}, {history})
- Magentic-One has full ledger protocol (facts/plan/progress JSON schemas)
- For swarms: Brood Lord (orchestrator of orchestrators) needs different prompt than Swarm Host

**GOOD IDEA — Reflection pass after tool execution**
- If `reflect_on_tool_use=True`, second model pass on enriched context
- If False, synthetic summary from formatter template
- For swarms: workers can optionally reflect on results before returning to coordinator

**GOOD IDEA — Model capability gates**
- `multiple_system_messages` toggles system/user role downgrade
- For swarms: adapt to different models for coordinator vs workers

**IRRELEVANT — Complex selector retry loop**
- On invalid output, inject corrective feedback and retry (max_selector_attempts)
- Swarms use deterministic handoff (like SwarmGroupChatManager), not LLM routing
- Skip this complexity

**Adoption Path**:
```rust
// hatchery/memory/
pub trait MemoryProvider {
    fn update_prompt_context(&self, base_prompt: &str) -> String;
    // Explicit: memory becomes visible prompt text
}

// Different orchestration modes get different prompts
pub enum OrchestrationMode {
    SwarmHost,    // uses ledger protocol
    BroodLord,    // uses swarm coordination protocol
}
```

---

## 3. AUTOGPT — Component Pipeline Prompting

### Architecture
- Protocol pipelines: DirectiveProvider, CommandProvider, MessageProvider
- One-shot strategy renders structured system + ordered messages per cycle
- JSON schema + prefill + optional function-calling enforce response shape
- Component graph + topological sort determines prompt content availability

### Evaluation

**EXCELLENT IDEA — Component-driven prompt assembly**
- Components collect directives/messages/commands per cycle
- Not static prompt, but dynamic based on enabled components
- For swarms: worker capabilities = enabled components, prompts auto-adapt

**GOOD IDEA — Response contract enforcement**
- JSON schema + prefill bias JSON start
- For swarms: structured task results enforced by prompt schema

**GOOD IDEA — Component ordering as architecture**
- Topological sort ensures correct directive/message availability
- For swarms: dependency graph of worker capabilities

**BAD IDEA — One-shot loop shape**
- Strict `system -> user -> messages -> trigger` every cycle
- Too rigid for multi-turn swarm coordination
- Alternative: stateful conversation with context windows

**IRRELEVANT — Separate agent profile generation**
- AgentProfileGenerator bootstraps identity/directives
- Swarms have fixed roles, no runtime identity synthesis needed

**Adoption Path**:
```rust
// hatchery/components/
pub trait PromptComponent {
    fn get_directives(&self) -> Vec<String>;
    fn get_messages(&self) -> Vec<Message>;
    fn run_after(&self) -> Vec<ComponentId>;
}

// Coordinator assembles final prompt from enabled components
```

---

## 4. CAMEL — Multi-Layer Prompt Architecture

### Architecture
- TextPrompt primitives with keyword introspection + partial-safe formatting
- Task prompt catalog by (TaskType, RoleType)
- Runtime prompt mutation in ChatAgent (update/append/reset system message)
- Structured output fallback via prompt injection when tools incompatible
- Auto-summarization triggered by token thresholds

### Evaluation

**EXCELLENT IDEA — Prompt primitives as typed objects**
- TextPrompt wraps strings with metadata and formatting logic
- Reusable, composable prompt objects
- For swarms: typed prompt fragments can be validated and combined safely

**EXCELLENT IDEA — Runtime prompt mutation APIs**
- `update_system_message()`, `append_to_system_message()`, `reset_to_original_system_message()`
- For swarms: workers can receive mid-task prompt adjustments from coordinator

**EXCELLENT IDEA — Structured output fallback**
- When native `response_format` incompatible with tools:
  1. Convert schema to textual JSON instruction
  2. Append to user input
  3. Set `response_format=None`
  4. Parse output manually
- For swarms: essential for model diversity (worker models may not support structured output)

**GOOD IDEA — Auto-summarization by token threshold**
- Trigger based on `summarize_threshold` and adaptive calculation
- For swarms: long-running workers need automatic context compression

**GOOD IDEA — Continuation prompt**
- If no termination signal: inject "Please continue."
- For swarms: workers can auto-continue complex tasks

**BAD IDEA — Highly fragmented prompt logic**
- Behavior tracing requires reading core agent + role_playing + workforce modules
- For swarms: keep prompt logic centralized in one module

**Adoption Path**:
```rust
// hatchery/prompts/
pub struct TypedPrompt {
    template: String,
    keywords: HashSet<String>,
}

impl TypedPrompt {
    pub fn format(&self, values: &HashMap<String, String>) -> String {
        // Partial-safe: missing keys remain as {key}
    }
}

// Runtime mutation
pub trait AgentSession {
    fn update_system_message(&mut self, new_msg: String);
    fn append_system_instruction(&mut self, instruction: String);
}
```

---

## 5. CLINE — Variant Registry + Component Assembly

### Architecture
- Variant registry for model-family specific prompts (generic, next-gen, native-next-gen, gpt-5, gemini-3, etc.)
- Component-based assembly with ordered `componentOrder` + `componentOverrides`
- User instruction layering from multiple sources (global/local `.clinerules`, `.cursorrules`, `AGENTS.md`)
- Skills: discovery vs activation (list in system, full content on `use_skill` tool call)
- Runtime hook-based context injection (TaskStart, UserPromptSubmit, etc.)

### Evaluation

**EXCELLENT IDEA — Variant registry for model families**
- Different models need different prompt structures
- Registry picks first matcher by SystemPromptContext
- For swarms: coordinator/worker models may differ, need variant support

**EXCELLENT IDEA — Skills discovery vs activation**
- System prompt lists skill names/descriptions
- Full content loaded on-demand via tool call
- For swarms: workers discover available skills, load only when needed (token efficiency)

**EXCELLENT IDEA — Multi-source instruction layering**
- Global `.clinerules` + local `.clinerules` + `.cursorrules` + `AGENTS.md` + `.clineignore`
- Precedence hierarchy clear
- For swarms: project-level + swarm-level + worker-level instructions

**EXCELLENT IDEA — Runtime hook context injection**
- Hooks output `contextModification` → appended as `<hook_context>` blocks
- For swarms: coordinator can inject runtime context via hooks without editing static prompts

**GOOD IDEA — Conditional rules evaluation**
- YAML frontmatter + conditional evaluation via request context
- For swarms: rules can activate based on task type, target language, etc.

**GOOD IDEA — Component ordering for stable caching**
- Extensions sorted by name for stable prompt order
- For swarms: deterministic prompt construction aids caching

**Adoption Path**:
```rust
// hatchery/prompts/registry.rs
pub struct PromptRegistry {
    variants: Vec<PromptVariant>,
}

impl PromptRegistry {
    pub fn get(&self, ctx: &PromptContext) -> PromptVariant {
        self.variants.iter()
            .find(|v| v.matches(ctx))
            .unwrap_or(self.default())
    }
}

// Skills
pub struct SkillRegistry {
    available: Vec<SkillMeta>, // name, description
}

// Load full skill on tool call
pub fn use_skill(name: &str) -> Result<String> {
    load_skill_content(name)
}
```

---

## 6. CONTINUE — Rule-Graph + Selective Projection

### Architecture
- Rule sources aggregated into config (`.continuerules`, markdown rules, `CodebaseRulesCache`, `AGENTS.md`)
- Applicability engine: `shouldApplyRule` checks policy/global/path/globs/regex
- CLI: large system message with env snapshot + directory tree + git status + user rules
- Skills as prompt-adjacent (loaded via tool, not unconditional system injection)

### Evaluation

**EXCELLENT IDEA — Rule applicability engine**
- Not blind concat, but selective based on:
  - policy override (on/off)
  - path matching from context items
  - directory-scoped behavior
  - globs + regex
- For swarms: workers apply only relevant rules for their current task/files

**EXCELLENT IDEA — Multi-source rule normalization**
- All sources → `RuleWithSource[]`
- Runtime chooses applicable subset
- For swarms: unified rule format from diverse sources (project, swarm, worker configs)

**GOOD IDEA — CLI env snapshot in system prompt**
- Agent identity + env snapshot + directory tree + git status
- For swarms: workers get consistent environment context

**GOOD IDEA — Skills via tool retrieval**
- Markdown skills loaded dynamically via `readSkill` tool
- For swarms: same pattern as Cline (discovery in prompt, full load on demand)

**IRRELEVANT — Legacy `config.systemMessage` conversion**
- JSON config → rule with `source: "json-systemMessage"`
- Swarms won't have legacy configs

**Adoption Path**:
```rust
// hatchery/rules/
pub struct Rule {
    source: String,
    content: String,
    policy: RulePolicy, // On, Off, Auto
    matchers: Vec<RuleMatcher>, // Path, Glob, Regex, etc.
}

pub fn apply_rules(rules: &[Rule], context: &TaskContext) -> Vec<&Rule> {
    rules.iter()
        .filter(|r| should_apply(r, context))
        .collect()
}
```

---

## 7. CREWAI — Catalog-Driven Prompt Compiler

### Architecture
- i18n catalog (`translations/en.json`) with prompt slices
- `Prompts.task_execution()` builds from slices: `role_playing` + `tools`/`no_tools` + `task`
- Split messages (`{system, user}`) vs merged prompt based on config
- Runtime task-prompt enrichment: output schema + context + memory + knowledge + training
- Context-overflow fallback: prompt-driven summarization

### Evaluation

**EXCELLENT IDEA — Prompt slices from catalog**
- Slices: `role_playing`, `tools`, `task`, `memory`, `formatted_task_instructions`
- Easy to override via custom `prompt_file`
- For swarms: swarm-level catalog with role/task slices

**EXCELLENT IDEA — Runtime task-prompt enrichment pipeline**
1. Base task.prompt()
2. Output schema instructions
3. Context injection
4. Memory context block
5. Knowledge retrieval
6. Training data
- For swarms: coordinator enriches worker prompts with task-specific context before dispatch

**GOOD IDEA — Hierarchical manager prompt from catalog**
- Manager role/goal/backstory from `hierarchical_manager_agent` key
- For swarms: Brood Lord / Swarm Host prompts from catalog

**GOOD IDEA — Reasoning/planning as sub-prompts**
- `reasoning.create_plan_prompt`, `reasoning.refine_plan_prompt`
- Optional function-calling schema for structured plan+ready
- For swarms: coordinator can use planning sub-prompts before dispatching work

**BAD IDEA — Heavy reliance on ReAct textual protocols**
- Tool list + tool-name constraints in prompt slices
- Strict action format enforcement
- Behavior quality depends on model compliance
- For swarms: use native tool-calling, not textual ReAct

**Adoption Path**:
```rust
// hatchery/prompts/catalog.rs
pub struct PromptCatalog {
    slices: HashMap<String, String>,
}

impl PromptCatalog {
    pub fn load_custom(path: &Path) -> Result<Self>;
    pub fn get_slice(&self, key: &str) -> &str;
}

// Task enrichment
pub fn enrich_task_prompt(
    base: &str,
    schema: Option<&str>,
    context: &str,
    memory: &str,
) -> String {
    // ...
}
```

---

## 8. CRUSH — Template-Driven Multi-Layer Composition

### Architecture
- Templates embedded at compile time, rendered per run (`coder.md.tpl`, `task.md.tpl`, `agentic_fetch_prompt.md.tpl`)
- Runtime metadata: working dir, platform, date, git snapshot
- Context files auto-discovered (`AGENTS.md`, `CLAUDE.md`, `.cursorrules`)
- Skills converted to XML and injected
- Provider-level `system_prompt_prefix` as separate top-level system message

### Evaluation

**EXCELLENT IDEA — Template + runtime metadata composition**
- Template body + env metadata + context files + skills XML
- For swarms: each worker renders prompt from template + runtime state

**EXCELLENT IDEA — Subagent prompt specialization**
- `task.md.tpl` for task subagents (minimal concise-worker prompt)
- `agentic_fetch_prompt.md.tpl` for specialized web fetch
- For swarms: different worker roles use different templates

**EXCELLENT IDEA — Provider prefix as separate system message**
- Not merged into template text, prepended at step preparation
- For swarms: model-specific prefixes can be added without touching role templates

**GOOD IDEA — MCP instructions runtime injection**
- Append `<mcp-instructions>` when MCP servers connected
- For swarms: tool/service availability modifies prompts dynamically

**GOOD IDEA — Auto-summarize trigger semantics**
- `StopWhen` checks remaining tokens, sets `shouldSummarize`
- For swarms: workers auto-compact when context full

**IRRELEVANT — Title and summary specialized flows**
- Separate prompt channels for session title, summarization
- Swarms don't need session titles

**Adoption Path**:
```rust
// hatchery/prompts/templates/
// coder.md.tpl, task_worker.md.tpl, researcher.md.tpl

pub struct PromptData {
    pub working_dir: PathBuf,
    pub platform: String,
    pub date: String,
    pub git_snapshot: Option<GitInfo>,
    pub context_files: Vec<String>,
    pub skills_xml: String,
}

pub fn render_template(template: &str, data: &PromptData) -> String {
    // ...
}
```

---

## 9. GOOSE — Layered Runtime-Composed System

### Architecture
- Registered template catalog with user override directory
- Runtime assembly: base template + extras/hints + chat-mode guard
- Dynamic inputs: extension list, date, mode flags, limits
- Explicit runtime mutation APIs (`extend_system_prompt`, `override_system_prompt`)
- Separate prompt channels for subagents, compaction, planning, permissions

### Evaluation

**EXCELLENT IDEA — User override directory**
- `Paths::config_dir()/prompts/<template_name>`
- User version wins over built-in
- For swarms: project can override swarm/worker prompts without forking code

**EXCELLENT IDEA — Runtime mutation APIs**
- `extend_system_prompt(instruction)` → append
- `override_system_prompt(template)` → replace
- For swarms: coordinator can dynamically reprogram worker prompts mid-task

**EXCELLENT IDEA — Separate prompt channels per subsystem**
- `system.md`, `subagent_system.md`, `compaction.md`, `plan.md`, `permission_judge.md`
- For swarms: clean separation of concerns

**GOOD IDEA — Extension list sorted for stable caching**
- Extensions sorted by name
- For swarms: deterministic prompt construction

**GOOD IDEA — Input sanitization**
- `sanitize_unicode_tags` on overrides/extras/extension instructions
- For swarms: prevent injection attacks in user-provided prompts

**Adoption Path**:
```rust
// hatchery/prompts/
pub struct PromptManager {
    templates: HashMap<String, String>,
    overrides: HashMap<String, String>,
}

impl PromptManager {
    pub fn load_with_overrides(builtin_dir: &Path, override_dir: &Path) -> Self;
    pub fn extend(&mut self, key: &str, instruction: &str);
    pub fn override_prompt(&mut self, key: &str, template: &str);
}
```

---

## 10. LANGGRAPH — Pipeline Prompt Architecture

### Architecture
- Core is prompt-agnostic, policy in agent layers
- `create_react_agent`: polymorphic prompt input (None/str/SystemMessage/Callable/Runnable/async)
- Pre-model hook: mutate messages or llm_input_messages before call
- Post-model hook: guardrails/approval before tool execution
- Structured output as separate final pass with optional system override

### Evaluation

**EXCELLENT IDEA — Prompt as polymorphic input**
- None → pass state["messages"]
- str → SystemMessage
- Callable(state) → dynamic prompt
- Runnable(state) → pipeline
- For swarms: workers accept different prompt input types (static, dynamic, state-driven)

**EXCELLENT IDEA — Pre-model hook for context curation**
- Can mutate `messages` (persisted) or `llm_input_messages` (transient)
- For swarms: coordinator curates context before worker inference (trimming, summarization)

**EXCELLENT IDEA — Post-model hook for guardrails**
- Inserted after model response, before tool execution
- Can participate in routing
- For swarms: coordinator validates worker outputs before execution

**GOOD IDEA — Structured output as separate final pass**
- After tool loop, dedicated node with `.with_structured_output(schema)`
- Supports tuple `(system_prompt, schema)` for final-pass-specific instruction
- For swarms: workers produce structured results in final step

**GOOD IDEA — Prompt callable can access runtime store**
- Prompt reads user-specific memory from store
- For swarms: prompts can be personalized per task/user context

**IRRELEVANT — Graph-composable prompt semantics**
- Multi-agent orchestration designed by node edges + node prompts
- Swarms have explicit orchestration layer, not graph routing

**Adoption Path**:
```rust
// hatchery/prompts/
pub enum PromptInput {
    Static(String),
    Dynamic(Box<dyn Fn(&State) -> String>),
    Pipeline(Box<dyn Runnable>),
}

// Hooks
pub trait PreModelHook {
    fn curate_context(&self, messages: &mut Vec<Message>);
}

pub trait PostModelHook {
    fn validate_output(&self, response: &Response) -> Result<()>;
}
```

---

## 11. METAGPT — Role-Prefix + Action Templates

### Architecture
- Role-level prefix (profile/name/goal/constraints) → `llm.system_prompt`
- Action templates (`PROMPT_TEMPLATE`) for task-specific generation
- ActionNode: schema-driven prompt compiler with review/revise loops
- Provider layer normalizes message assembly
- RoleZero: dynamic orchestration prompts with memory + experience

### Evaluation

**EXCELLENT IDEA — Role prefix propagation**
- Role sets `llm.system_prompt`, all actions inherit via `action.set_prefix()`
- For swarms: worker role identity propagates to all tasks

**EXCELLENT IDEA — ActionNode structured prompting**
- Compiles prompts from node trees: context + examples + field instructions + constraints
- Parses/validates into Pydantic models
- For swarms: structured task definitions with validation

**EXCELLENT IDEA — Review/revise loop**
- ActionNode reviews generated values vs requirements
- Revises only mismatched fields
- For swarms: workers self-correct outputs before returning to coordinator

**GOOD IDEA — Decentralized action templates**
- Per-action `PROMPT_TEMPLATE` patterns
- For swarms: each task type has dedicated template

**BAD IDEA — Provider-specific system semantics**
- OpenAI → messages array
- Anthropic → extract system into dedicated field
- For swarms: abstract away provider differences in orchestration layer

**IRRELEVANT — Role decision prompting**
- Multi-action mode builds state-selection prompt
- Swarms use explicit task routing, not LLM state selection

**Adoption Path**:
```rust
// hatchery/actions/
pub struct ActionNode {
    pub context: String,
    pub examples: Vec<String>,
    pub field_instructions: HashMap<String, String>,
    pub constraints: Vec<String>,
}

impl ActionNode {
    pub fn compile_prompt(&self) -> String;
    pub fn parse_and_validate(&self, output: &str) -> Result<Value>;
    pub fn review(&self, value: &Value) -> Vec<String>; // mismatches
    pub fn revise(&self, fields: &[String]) -> String; // prompt for revisions
}
```

---

## 12. OPEN INTERPRETER — LMC-Style Runtime Composition

### Architecture
- LMC-style messages on interpreter instance
- Dynamic system rendering: `{{ ... }}` blocks executed as Python, output substituted
- System layering: base + per-language + custom instructions + computer API
- Dual modes: tool-calling vs text-coding (execution instructions injected for text mode)
- Loop-mode continuation: inject `loop_message` between cycles

### Evaluation

**EXCELLENT IDEA — Dynamic system rendering**
- `{{ ... }}` blocks executed as code, output substituted into system prompt
- For swarms: prompts can include runtime computations (current task count, memory usage, etc.)

**GOOD IDEA — Per-language system messages**
- Each enabled terminal language has `language.system_message`
- For swarms: workers targeting different languages get language-specific instructions

**GOOD IDEA — Dual prompting modes**
- Tool-calling: native schema + parse tool deltas
- Text mode: append execution instructions + parse fenced code
- For swarms: support both modes based on worker model capabilities

**BAD IDEA — LMC message typing (message/code/console/image)**
- Custom message format converted to OpenAI-style
- For swarms: stick to standard message formats, avoid custom conversions

**IRRELEVANT — Computer API prompt expansion**
- Enabling `import_computer_api` adds large API signatures/docstrings
- Swarms have explicit tool schemas, not API documentation dumps

**Adoption Path**:
```rust
// Dynamic rendering
pub fn render_system_prompt(template: &str, context: &RuntimeContext) -> String {
    // Parse {{ expr }} blocks, evaluate, substitute
}

// Dual modes
pub enum PromptMode {
    ToolCalling,
    TextCoding { execution_instructions: String },
}
```

---

## 13. OPENCLAW — Layered Control-Plane

### Architecture
- Central compiler `buildAgentSystemPrompt(...)` with modes (full/minimal/none)
- Skills: multi-source merged (extra < bundled < managed < workspace)
- Plugin hooks: `before_agent_start` with `prependContext`
- Subagents use `minimal` mode (trim heavy sections)
- Embedded vs CLI runner paths

### Evaluation

**EXCELLENT IDEA — Prompt modes (full/minimal/none)**
- Full for main agent, minimal for subagents (trim memory recall, silent replies, heartbeats)
- For swarms: coordinator gets full prompt, workers get minimal task-focused prompts

**EXCELLENT IDEA — Multi-source skill merge with precedence**
- `extra < bundled < managed < workspace`
- For swarms: clear override hierarchy (built-in < swarm-level < project-level)

**GOOD IDEA — Plugin hook for context prepending**
- `before_agent_start` → `prependContext` modifies user prompt
- For swarms: plugins can inject runtime context before worker execution

**GOOD IDEA — Prompt observability**
- `system-prompt-report` captures metadata, chars, skill footprint
- For swarms: monitoring prompt sizes for debugging

**IRRELEVANT — CLI backend system prompt arg strategies**
- `systemPromptWhen` (first/never/etc.)
- Swarms use embedded runtime, not CLI backends

**Adoption Path**:
```rust
// hatchery/prompts/
pub enum PromptMode {
    Full,
    Minimal,
    None,
}

pub fn build_system_prompt(
    mode: PromptMode,
    skills: &[Skill],
    context: &RuntimeContext,
) -> String {
    match mode {
        PromptMode::Full => build_full(skills, context),
        PromptMode::Minimal => build_minimal(skills, context),
        PromptMode::None => build_identity_only(),
    }
}
```

---

## 14. OPENCODE — Multi-Channel Late-Binding

### Architecture
- Base provider/agent prompt template selection
- Instruction layering: `AGENTS.md` + `CLAUDE.md` + config globs + HTTP URLs
- Skills loaded lazily via tool calls
- File-scoped instruction auto-injection on `read` tool
- Plugin transforms: `chat.system.transform`, `chat.messages.transform`

### Evaluation

**EXCELLENT IDEA — File-scoped instruction injection**
- When `read` tool reads file, check parent dirs for nearby `AGENTS.md`/`CLAUDE.md`
- If found, append `<system-reminder>` with instructions
- For swarms: workers get context-specific instructions when accessing files

**EXCELLENT IDEA — Instruction sources include HTTP(S) URLs**
- `config.instructions` can be remote URLs (fetched with timeout)
- For swarms: shared instruction repositories, versioned prompts via URL

**EXCELLENT IDEA — Runtime reminder injection**
- Plan-mode reminder, build-switch reminder, max-steps reminder
- Injected as `<system-reminder>` blocks
- For swarms: coordinator sends reminders to workers at runtime

**GOOD IDEA — Provider/model-specific base prompts**
- gpt-5 → `codex_header.txt`, gemini → `gemini.txt`, claude → `anthropic.txt`
- For swarms: different models for coordinator/workers

**GOOD IDEA — Plugin transform points**
- `experimental.chat.system.transform` mutates system blocks
- For swarms: plugins can modify prompts before sending to workers

**BAD IDEA — Codex OAuth divergence**
- Special path for OpenAI OAuth: prompt in provider-level instruction option
- Adds complexity
- For swarms: stick to uniform prompt delivery

**Adoption Path**:
```rust
// hatchery/instructions/
pub enum InstructionSource {
    FilePath(PathBuf),
    HttpUrl(String),
}

pub fn load_instructions(sources: &[InstructionSource]) -> Result<Vec<String>> {
    // Load from files + fetch from URLs
}

// File-scoped injection
pub fn read_file_with_instructions(path: &Path) -> (String, Option<String>) {
    let content = fs::read_to_string(path)?;
    let instructions = find_nearby_instructions(path)?;
    (content, instructions)
}
```

---

## 15. OPENHANDS — Event-Layered Prompting

### Architecture
- Template selection by config (`system_prompt.j2`, `system_prompt_long_horizon.j2`)
- Event-driven layering: `SystemMessageAction` + `RecallObservation` events inject context
- Microagents: global/user/workspace sources, types (KNOWLEDGE/REPO_KNOWLEDGE/TASK)
- Prompt variant composition via Jinja include
- Platform rewrite pass (Windows: bash → powershell)

### Evaluation

**EXCELLENT IDEA — Microagents as prompt extensions**
- Global (`~/.openhands/microagents`) + workspace (`.openhands/microagents`) + `.cursorrules` + `AGENTS.md`
- Types: KNOWLEDGE (keyword-triggered), REPO_KNOWLEDGE (always-on), TASK (slash-triggered)
- For swarms: skills/knowledge as microagents, triggered by context

**EXCELLENT IDEA — Event-driven context injection**
- `RecallObservation(WORKSPACE_CONTEXT)` → workspace info + repo instructions + runtime info
- `RecallObservation(KNOWLEDGE)` → triggered microagent snippets
- For swarms: coordinator sends context events, worker memory converts to prompt blocks

**GOOD IDEA — Template includes for variants**
- `system_prompt_long_horizon.j2` includes `system_prompt.j2` + appends task-tracker rules
- For swarms: base prompts + mode-specific overlays

**GOOD IDEA — Platform rewrite pass**
- Windows: `execute_bash` → `execute_powershell`
- For swarms: platform-aware prompt adjustments

**IRRELEVANT — V1 core in external SDK**
- Current repo is legacy V0
- For swarms: learn patterns, but don't depend on V0 specifics

**Adoption Path**:
```rust
// hatchery/microagents/
pub enum MicroagentType {
    Knowledge { triggers: Vec<String> },
    RepoKnowledge, // always-on
    Task { name: String, inputs: Vec<String> },
}

pub struct Microagent {
    pub ty: MicroagentType,
    pub content: String,
}

// Event-driven injection
pub enum ContextEvent {
    WorkspaceContext(WorkspaceInfo),
    KnowledgeRecall(Vec<Microagent>),
}

pub fn inject_context_events(messages: &mut Vec<Message>, events: &[ContextEvent]) {
    // Convert events to user message blocks
}
```

---

## 16. PLANDEX — Protocol-Locked Staged Prompting

### Architecture
- Stage-driven: CONTEXT (context selection) → PLANNING (task breakdown) → IMPLEMENTATION (code changes)
- Distinct wrapper per stage with strict output contracts
- Parser/stream coupling: `### Tasks`, `Uses:`, `<PlandexBlock>`, `<PlandexFinish/>`
- Existing subtasks injected as live control context
- Chat mode has separate prompt policy (conversational vs executional)

### Evaluation

**EXCELLENT IDEA — Staged prompting**
- Different stages need different instructions and output formats
- For swarms: task decomposition stage vs execution stage vs review stage

**GOOD IDEA — Strict output protocols**
- Machine-readable output contracts enable deterministic parsing
- For swarms: structured task results (JSON) instead of freeform text

**GOOD IDEA — Existing plan injected as runtime memory**
- `### LATEST PLAN TASKS ###` in system prompt
- For swarms: coordinator injects current task graph into worker prompts

**BAD IDEA — Tight prompt/parser coupling**
- Prompts use `<PlandexBlock>`, parsers regex match opening tags
- Small format drift breaks automation
- For swarms: use JSON schemas, not custom textual protocols

**BAD IDEA — Auto-context two-pass workflow**
- First pass: context selection, second pass: planning/implementation
- Too complex
- For swarms: explicit task assignment, not LLM-driven context selection

**IRRELEVANT — Chat vs tell dual regimes**
- Swarms have task execution mode, not conversational chat mode

**Adoption Path**:
```rust
// hatchery/stages/
pub enum TaskStage {
    Planning,
    Implementation,
    Review,
}

pub fn get_stage_prompt(stage: TaskStage, context: &TaskContext) -> String {
    match stage {
        TaskStage::Planning => planning_prompt(context),
        TaskStage::Implementation => implementation_prompt(context),
        TaskStage::Review => review_prompt(context),
    }
}

// Use JSON output, not custom protocols
```

---

## 17. ROO-CODE — Override Hierarchy Compiler

### Architecture
- Mode selection: custom > built-in + promptComponent > default
- File override: `.roo/system-prompt-<mode>` hard-switches prompt
- Custom instructions aggregator: language + global + mode-specific + mode rules + AGENTS + generic rules
- Skills: mode-filtered, precedence (project > global > built-in)
- Settings-driven: `enableSubfolderRules`, `useAgentRules`, `isStealthModel`

### Evaluation

**EXCELLENT IDEA — Explicit override hierarchy**
- Custom mode > built-in + overrides > default
- For swarms: clear precedence for swarm configs vs project configs vs defaults

**EXCELLENT IDEA — Mode-specific rules**
- `.roo/rules-<mode>`, fallback `.roorules-<mode>`
- For swarms: research-agent rules vs implementer rules vs reviewer rules

**EXCELLENT IDEA — AGENTS.md + AGENTS.local.md**
- `AGENTS.md` in repo, `AGENTS.local.md` for local overrides
- For swarms: shared swarm instructions + developer-local tweaks

**GOOD IDEA — Settings-driven prompt behavior**
- `enableSubfolderRules` controls recursive discovery
- `isStealthModel` injects confidentiality block
- For swarms: configuration flags control prompt assembly

**GOOD IDEA — Skills mode filtering**
- Skills filtered by mode (research skills vs coding skills)
- For swarms: role-specific skill sets

**IRRELEVANT — Tools catalog hardcoded as empty**
- Comment: not included in system prompt, relies on native tool-calling
- For swarms: same approach (schema in runtime, not prompt text)

**Adoption Path**:
```rust
// hatchery/prompts/
pub struct PromptOverride {
    pub mode: Option<String>,
    pub file_path: Option<PathBuf>,
    pub custom_instructions: Vec<String>,
}

pub fn resolve_prompt(
    mode: &str,
    overrides: &[PromptOverride],
    defaults: &PromptDefaults,
) -> String {
    // Custom > built-in+overrides > default
}

// Mode-specific rules
pub fn load_rules(mode: &str, enable_subfolders: bool) -> Vec<Rule> {
    let mode_rules = load_mode_rules(mode);
    let generic_rules = load_generic_rules();
    let agents_rules = load_agents_md();
    // ...
}
```

---

## 18. SWE-AGENT — Template + State Compiler

### Architecture
- Template-driven (YAML-configurable): `system_template`, `instance_template`, `next_step_template`, etc.
- Runtime state interpolation via `_get_format_dict()` (command_docs, tool env, problem, repo, dynamic state)
- History processors as middleware (LastNObservations, CacheControl, RemoveRegex, ImageParsing)
- Error-requery loop: ephemeral requery history for correction
- Separate review/selection prompts in retry mode

### Evaluation

**EXCELLENT IDEA — Declarative templates (YAML-configurable)**
- Templates defined in config, not hardcoded
- For swarms: swarm orchestration config includes prompt templates

**EXCELLENT IDEA — History processors as middleware**
- Middleware transforms messages before model call
- Examples: observation elision, cache tags, regex removal, image parsing
- For swarms: coordinator applies processors to worker message history (trim, sanitize, cache)

**EXCELLENT IDEA — Error-requery loop**
- On parse/syntax errors: create temporary requery history with error template
- For swarms: workers auto-correct errors via requery before escalating to coordinator

**GOOD IDEA — Runtime state interpolation**
- `_get_format_dict()` combines static + dynamic state
- For swarms: task context (working_dir, diff, repo) injected into prompts

**GOOD IDEA — Separate review/selection prompts**
- Retry mode: reviewer, chooser, sampler prompts
- For swarms: coordinator review stage has dedicated prompts

**BAD IDEA — Demonstrations in history**
- `put_demos_in_history=true` replays demo trajectory
- Complex and brittle
- For swarms: use examples in system prompt, not history replay

**Adoption Path**:
```rust
// hatchery/prompts/
pub struct PromptTemplate {
    pub system: String,
    pub instance: String,
    pub next_step: String,
    pub error_requery: String,
}

// History processors
pub trait HistoryProcessor {
    fn process(&self, messages: Vec<Message>) -> Vec<Message>;
}

pub struct LastNObservations(usize);
pub struct RemoveRegex(Regex);
// ...

pub fn apply_processors(
    messages: Vec<Message>,
    processors: &[Box<dyn HistoryProcessor>],
) -> Vec<Message> {
    processors.iter().fold(messages, |msgs, p| p.process(msgs))
}
```

---

## Cross-Cutting Patterns Analysis

### 1. Layered vs Monolithic

**Winner: Layered Composition**

All modern agents use layered prompts:
- Base role definition
- Dynamic context (files, memory, tools)
- Runtime metadata (env, date, git)
- User instructions (AGENTS.md, rules)

**For Swarms**: Never use a single static prompt string. Use compositional layers:
```rust
pub struct SwarmPrompt {
    role: String,              // base identity
    task: String,              // current assignment
    context: Vec<String>,      // files, memory
    instructions: Vec<String>, // AGENTS.md, rules
    metadata: RuntimeMetadata, // env, git, date
}
```

---

### 2. Static vs Dynamic Rendering

**Winner: Dynamic Rendering**

Best agents (Cline, Crush, OpenCode, Open Interpreter) use:
- Templates with placeholders
- Runtime variable substitution
- Platform/model-specific adaptation

**For Swarms**:
```rust
pub fn render_worker_prompt(
    template: &str,
    role: &str,
    task: &Task,
    context: &RuntimeContext,
) -> String {
    template
        .replace("{role}", role)
        .replace("{task}", &task.description)
        .replace("{working_dir}", &context.working_dir.display())
        .replace("{platform}", &context.platform)
        // ...
}
```

---

### 3. AGENTS.md / .cursorrules / Project Instructions

**Winner: Multi-Source with Precedence**

Universal pattern:
- Global config dir (e.g., `~/.config/app/AGENTS.md`)
- Project root (`AGENTS.md`, `CLAUDE.md`, `.cursorrules`)
- Local overrides (`AGENTS.local.md`)
- Mode-specific (`.clinerules-<mode>`)

**For Swarms**:
```
Precedence: local > project > global
Mode-specific > generic

Sources:
1. ~/.config/hatchery/AGENTS.md          (global)
2. <project>/AGENTS.md                   (project)
3. <project>/AGENTS.local.md             (local, gitignored)
4. <project>/.hatchery/rules-research    (mode-specific)
5. <project>/.hatchery/rules             (generic)
```

---

### 4. Skills: Preload vs Lazy Load

**Winner: Discovery + Lazy Load**

Best pattern (Cline, Continue, OpenCode):
- System prompt: list skill names/descriptions (`<available_skills>`)
- Tool call: load full content on demand (`use_skill(name)`)

Why:
- Token efficiency (don't load all skills upfront)
- Explicit activation (model decides when to use)

**For Swarms**:
```rust
// System prompt
pub fn list_available_skills(role: &str) -> String {
    let skills = discover_skills_for_role(role);
    format!(
        "<available_skills>\n{}\n</available_skills>",
        skills.iter()
            .map(|s| format!("- {}: {}", s.name, s.description))
            .collect::<Vec<_>>()
            .join("\n")
    )
}

// Tool handler
pub fn use_skill(name: &str) -> Result<String> {
    let skill = load_skill_content(name)?;
    Ok(format!("<skill_content name=\"{}\">\n{}\n</skill_content>", name, skill))
}
```

---

### 5. Structured Output: Native vs Prompt Fallback

**Winner: Native with Prompt Fallback**

CAMEL pattern:
1. Try native `response_format` (preferred)
2. If incompatible with tools:
   - Convert schema to textual JSON instruction
   - Append to user message
   - Parse output manually

**For Swarms**:
```rust
pub fn enforce_structured_output(
    model: &Model,
    schema: &Schema,
    prompt: &str,
) -> String {
    if model.supports_structured_output() && !has_tool_conflicts() {
        // Use native response_format
        prompt
    } else {
        // Fallback: inject schema as prompt
        format!(
            "{}\n\nRespond with a JSON object matching this schema:\n```json\n{}\n```",
            prompt,
            serde_json::to_string_pretty(schema).unwrap()
        )
    }
}
```

---

### 6. Memory/Context Injection

**Winner: Explicit Prompt Text (Not Hidden)**

AutoGen pattern:
- Memory.update_context() modifies prompt
- Appends as SystemMessage: "Relevant memory content: ..."

**For Swarms**:
```rust
pub fn inject_shared_memory(prompt: &str, memory: &SharedMemory) -> String {
    let memory_block = format!(
        "<shared_memory>\n{}\n</shared_memory>",
        memory.recent_entries().join("\n")
    );
    format!("{}\n\n{}", prompt, memory_block)
}
```

---

### 7. Runtime Prompt Mutation

**Winner: Explicit APIs (CAMEL, Goose)**

```rust
pub trait AgentSession {
    fn update_system_message(&mut self, new: String);
    fn append_system_instruction(&mut self, instruction: String);
    fn reset_system_message(&mut self);
}
```

**For Swarms**: Coordinator can reprogram worker prompts mid-task:
```rust
// Worker stuck? Add clarification
worker.append_system_instruction(
    "Focus on extracting API endpoints, ignore rate limit details for now."
);
```

---

### 8. Model-Specific Variants

**Winner: Variant Registry (Cline)**

```rust
pub struct PromptVariant {
    pub matcher: Box<dyn Fn(&ModelInfo) -> bool>,
    pub template: String,
}

pub struct PromptRegistry {
    variants: Vec<PromptVariant>,
}

impl PromptRegistry {
    pub fn get(&self, model: &ModelInfo) -> &str {
        self.variants.iter()
            .find(|v| (v.matcher)(model))
            .map(|v| &v.template)
            .unwrap_or(&self.default)
    }
}
```

**For Swarms**: Coordinator (Opus) vs Workers (Sonnet/Haiku) need different prompts.

---

### 9. Tool Descriptions in Prompts

**Winner: Native Tool Schema (Not Textual)**

Modern agents (Roo, OpenCode, Cline) note:
- Tool catalog NOT in system prompt
- Rely on native tool-calling schema

**For Swarms**: Don't inject tool descriptions as text. Use native schema.

Exception: If model doesn't support native tools, use prompt-based fallback (like CAMEL structured output fallback).

---

### 10. Staged/Phased Prompting

**Winner: Explicit Stages (Plandex, MetaGPT)**

For multi-step workflows:
- Planning stage → task decomposition prompt
- Implementation stage → code generation prompt
- Review stage → quality check prompt

**For Swarms**:
```rust
pub enum WorkflowStage {
    Planning,
    Research,
    Implementation,
    Testing,
    Review,
}

pub fn get_stage_prompt(stage: WorkflowStage, role: AgentRole) -> String {
    // Stage-specific + role-specific prompt
}
```

---

## Summary: Adoption Roadmap for Hatchery

### Phase 1: Core Prompt Architecture

```rust
// hatchery/prompts/mod.rs

pub struct PromptPack {
    pub role_definition: String,
    pub main_system: String,
    pub system_reminder: Option<String>,
    pub examples: Vec<Message>,
}

pub struct PromptRegistry {
    packs: HashMap<AgentRole, PromptPack>,
    variants: HashMap<ModelFamily, PromptVariant>,
}

pub struct PromptBuilder {
    registry: PromptRegistry,
}

impl PromptBuilder {
    pub fn build(
        &self,
        role: AgentRole,
        model: &ModelInfo,
        task: &Task,
        context: &RuntimeContext,
    ) -> String {
        let pack = self.registry.get_pack(role);
        let variant = self.registry.get_variant(model);

        // Layer composition
        let mut layers = vec![
            pack.role_definition,
            pack.main_system,
        ];

        // Add dynamic context
        layers.push(self.build_context_layer(task, context));

        // Add instructions (AGENTS.md, rules)
        layers.extend(self.load_instructions(role, context));

        // Add skills (discovery only)
        layers.push(self.list_skills(role));

        // Add runtime metadata
        layers.push(self.build_metadata_layer(context));

        // Add reminder
        if let Some(reminder) = &pack.system_reminder {
            layers.push(reminder.clone());
        }

        // Render with variant-specific formatting
        variant.render(&layers.join("\n\n"))
    }
}
```

### Phase 2: Instruction Loading

```rust
// hatchery/instructions/mod.rs

pub struct InstructionLoader {
    sources: Vec<InstructionSource>,
}

pub enum InstructionSource {
    GlobalConfig,          // ~/.config/hatchery/AGENTS.md
    ProjectAgents,         // <project>/AGENTS.md
    ProjectLocal,          // <project>/AGENTS.local.md (gitignored)
    ModeRules(String),     // .hatchery/rules-<mode>/
    GenericRules,          // .hatchery/rules/
    CursorRules,           // .cursorrules (compat)
}

impl InstructionLoader {
    pub fn load(&self, role: AgentRole) -> Vec<String> {
        let mut instructions = vec![];

        // Load in precedence order
        for source in &self.sources {
            if let Some(content) = self.load_source(source, role) {
                instructions.push(content);
            }
        }

        instructions
    }

    fn load_source(&self, source: &InstructionSource, role: AgentRole) -> Option<String> {
        match source {
            InstructionSource::ModeRules(mode) => {
                // Load mode-specific rules for this role
                let path = format!(".hatchery/rules-{}/{}.md", mode, role.slug());
                self.read_if_exists(&path)
            },
            // ...
        }
    }
}
```

### Phase 3: Skills System

```rust
// hatchery/skills/mod.rs

pub struct SkillRegistry {
    skills: Vec<SkillMeta>,
}

pub struct SkillMeta {
    pub name: String,
    pub description: String,
    pub applicable_roles: Vec<AgentRole>,
    pub path: PathBuf,
}

impl SkillRegistry {
    pub fn discover(role: AgentRole) -> Vec<SkillMeta> {
        let mut skills = vec![];

        // Load from multiple sources with precedence
        skills.extend(Self::load_from(".claude/skills", role));
        skills.extend(Self::load_from(".agents/skills", role));
        skills.extend(Self::load_from(".hatchery/skills", role));
        skills.extend(Self::load_global_skills(role));

        // Dedupe by name, last wins (project > global)
        Self::dedupe_by_name(skills)
    }

    pub fn format_for_prompt(skills: &[SkillMeta]) -> String {
        format!(
            "<available_skills>\n{}\n</available_skills>",
            skills.iter()
                .map(|s| format!("- {}: {}", s.name, s.description))
                .collect::<Vec<_>>()
                .join("\n")
        )
    }
}

// Tool handler
pub fn use_skill_tool(name: &str) -> Result<String> {
    let skill_path = SkillRegistry::find_skill(name)?;
    let content = fs::read_to_string(skill_path)?;
    Ok(format!(
        "<skill_content name=\"{}\">\n{}\n</skill_content>",
        name, content
    ))
}
```

### Phase 4: Runtime Mutation & Hooks

```rust
// hatchery/session/mod.rs

pub trait AgentSession {
    fn system_message(&self) -> &str;
    fn update_system_message(&mut self, new: String);
    fn append_system_instruction(&mut self, instruction: String);
    fn reset_system_message(&mut self);
}

pub struct WorkerSession {
    role: AgentRole,
    original_system: String,
    current_system: String,
    messages: Vec<Message>,
}

impl AgentSession for WorkerSession {
    fn update_system_message(&mut self, new: String) {
        self.current_system = new;
    }

    fn append_system_instruction(&mut self, instruction: String) {
        self.current_system.push_str("\n\n");
        self.current_system.push_str(&instruction);
    }

    fn reset_system_message(&mut self) {
        self.current_system = self.original_system.clone();
    }
}
```

### Phase 5: Swarm-Specific Patterns

```rust
// hatchery/swarm/prompts.rs

pub struct SwarmPrompts;

impl SwarmPrompts {
    pub fn coordinator() -> PromptPack {
        PromptPack {
            role_definition: "You are the Swarm Host coordinator...".to_string(),
            main_system: include_str!("templates/swarm_host.md"),
            system_reminder: Some("Remember to update SharedMemory...".to_string()),
            examples: vec![],
        }
    }

    pub fn worker(role: AgentRole) -> PromptPack {
        match role {
            AgentRole::ResearchAgent => PromptPack {
                role_definition: "You are a research specialist...".to_string(),
                main_system: include_str!("templates/research_agent.md"),
                system_reminder: None,
                examples: vec![],
            },
            AgentRole::RustImplementer => PromptPack {
                role_definition: "You are a Rust implementation specialist...".to_string(),
                main_system: include_str!("templates/rust_implementer.md"),
                system_reminder: Some("Always run `cargo check` after edits...".to_string()),
                examples: vec![],
            },
            // ...
        }
    }

    pub fn enrich_task_prompt(
        base: &str,
        task: &Task,
        shared_memory: &SharedMemory,
    ) -> String {
        let mut enriched = base.to_string();

        // Add task context
        enriched.push_str(&format!("\n\n## Current Task\n{}", task.description));

        // Add shared memory
        if !shared_memory.is_empty() {
            enriched.push_str(&format!(
                "\n\n<shared_memory>\n{}\n</shared_memory>",
                shared_memory.recent_entries().join("\n")
            ));
        }

        // Add output schema
        if let Some(schema) = &task.output_schema {
            enriched.push_str(&format!(
                "\n\n## Required Output Format\n```json\n{}\n```",
                serde_json::to_string_pretty(schema).unwrap()
            ));
        }

        enriched
    }
}
```

---

## Final Recommendations

### MUST ADOPT

1. **Layered Prompt Composition**
   - Base role + task context + instructions + skills + metadata
   - Never monolithic strings

2. **AGENTS.md / Project Instructions**
   - Multi-source with clear precedence
   - Global → Project → Local
   - Mode-specific overrides

3. **Skills Discovery + Lazy Load**
   - List in prompt, load on tool call
   - Token-efficient, explicit activation

4. **Runtime Prompt Mutation**
   - Coordinator can adjust worker prompts mid-task
   - `update_system_message()`, `append_system_instruction()`

5. **Model-Specific Variants**
   - Coordinator (Opus) vs Workers (Sonnet/Haiku)
   - Variant registry with matchers

6. **Structured Output Fallback**
   - Native schema preferred
   - Prompt injection when native incompatible

7. **Explicit Memory Injection**
   - SharedMemory → visible prompt text
   - Not hidden side-channel

### CONSIDER

1. **History Processors**
   - Middleware to trim/sanitize messages
   - Cache-control tags for Anthropic

2. **Pre/Post Model Hooks**
   - Context curation before inference
   - Validation before execution

3. **Staged Prompting**
   - Planning → Implementation → Review
   - Different prompts per stage

### AVOID

1. **Tight Prompt/Parser Coupling**
   - Don't use custom textual protocols (`<PlandexBlock>`)
   - Use JSON schemas

2. **ReAct Textual Protocols**
   - Use native tool-calling
   - Fallback to structured output prompt injection

3. **Fragmented Prompt Logic**
   - Centralize in one module
   - Don't scatter across many files

4. **Blind Instruction Concatenation**
   - Use applicability rules
   - Filter by context/role/task

---

## Conclusion

The 18 CLI agents analyzed converge on a clear architecture:

**Prompting is not a string, it's a runtime compiler.**

Best practices:
- Layered composition (role + task + context + instructions + metadata)
- Filesystem-based overrides (AGENTS.md, mode-specific rules)
- Lazy-loaded skills (discovery in prompt, full load on tool call)
- Explicit memory injection (visible prompt text)
- Model-specific variants (different models need different prompts)
- Runtime mutation (coordinator adjusts worker prompts dynamically)
- Structured output fallback (native preferred, prompt injection when needed)

For Hatchery swarm orchestration:
- Coordinator gets full rich prompt with orchestration logic
- Workers get minimal task-focused prompts (role + task + relevant instructions)
- SharedMemory explicitly injected into worker prompts
- Skills/tools lazy-loaded via tool calls
- AGENTS.md + mode-specific rules for project-level customization
- Runtime prompt mutation for mid-task adjustments

This is the path forward.
