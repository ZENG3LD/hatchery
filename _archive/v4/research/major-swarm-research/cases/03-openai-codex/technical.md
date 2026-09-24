# OpenAI Codex: Multi-Agent Coding System - Technical Deep Dive

## 1. Prompts & Prompting Strategy

### System Prompt Templates

**Official Guidance:**
> "If you can, start with the standard Codex-Max prompt as your base and make tactical additions from there. The most critical snippets are those covering autonomy and persistence, codebase exploration, tool use, and frontend quality."

### Core Prompt Directives

#### Autonomy and Persistence
> "You should act as an autonomous senior engineer: once the user gives a direction, proactively gather context, plan, implement, test, and refine without waiting for additional prompts at each step. Persist until the task is fully handled end-to-end within the current turn whenever feasible."

#### Code Quality Standards
> "Act as a discerning engineer: optimize for correctness, clarity, and reliability over speed; avoid risky shortcuts, speculative changes, and messy hacks just to get the code to work."

#### Tool Usage Optimization
> "When multiple tool calls can be parallelized (e.g., todo updates with other actions, file searches, reading files), make these tool calls in parallel instead of sequential to ensure you can make progress efficiently."

### Task Decomposition: PLANS.md Approach

**Codex Execution Plans (ExecPlans):**
> "Codex Execution Plans (ExecPlans) are design documents that a coding agent can follow to deliver a working feature or system change. Codex and the gpt-5.2-codex model can be used to implement complex tasks that take significant time to research, design, and implement."

**Multi-Hour Problem Solving:**
> "The PLANS.md approach has enabled Codex to work for more than seven hours from a single prompt. This workflow guides the AI to generate a comprehensive, multi-step implementation plan for large projects before writing any code."

**Official Documentation:**
- `developers.openai.com/cookbook/articles/codex_exec_plans/`
- `cookbook.openai.com/articles/codex_exec_plans`

**Strategy:**
1. User provides high-level task
2. Codex generates detailed PLANS.md with multi-step breakdown
3. User reviews and approves plan
4. Codex executes plan step-by-step
5. Continuous validation against plan checkpoints

**Adaptive Planning:**
> "Planning in Claude Code demonstrates that it shouldn't be a rigid user setting, but rather an emergent behavior based on complexity. For simple questions it just plans and executes, while for complex tasks it triggers an AskUserQuestion tool flow to clarify requirements before generating a plan."

### Reasoning Model Selection

**Thinking Levels (Introduced September 2025):**
- **Standard** - Default for most tasks
- **Medium** - Recommended for interactive coding (balances intelligence and speed)
- **High** - Deep analysis for complex problems
- **Extra High ('xhigh')** - Non-latency-sensitive tasks requiring extended reasoning

> "For non-latency-sensitive tasks, OpenAI introduced a new Extra High ('xhigh') reasoning effort, which thinks for an even longer period of time for a better answer. The thinking level toggle introduced in September 2025 gives users more choice beyond Standard, allowing them to select the right thinking level for their question—whether they want lighter, faster responses or more extended reasoning when depth and accuracy matter more."

### 2026 Prompt Engineering Best Practices

**Clarity Over Length:**
> "Prompt engineering didn't become 'writing longer prompts' in 2026—it became writing clearer specs. The #1 prompt engineering best practice in 2026 is to write success criteria and an output contract."

**Official Resources:**
- Codex Prompting Guide: `developers.openai.com/cookbook/examples/gpt-5/codex_prompting_guide/`
- GPT-5.1-Codex-Max Prompting Guide: `cookbook.openai.com/examples/gpt-5/gpt-5-1-codex-max_prompting_guide`

---

## 2. Memory & Context Management

### Context Window

**Effective Context Window:**
> "Codex's effective context window is set at 272k tokens (400k-128k), with an auto-compaction threshold of 0.95 leaving users with 258k usable context window. However, the underlying GPT-5.2-Codex model supports approximately 400k tokens."

**Community Requests:**
- Issue #9429: Increase effective context window 272000 → 350000
- Issue #9857: Increase Codex CLI context window to match ~400k model capacity
- Rationale: Better support for larger codebases

**Quadratic Performance Problem:**
> "A key challenge in the agent loop is performance optimization. LLM inference performance is quadratic in terms of the amount of JSON sent to the Responses API over the course of the conversation. Prompt caching is key: by reusing the output of a previous inference call, inference performance becomes linear instead of quadratic."

### Session Persistence

**Rollout Files:**
> "Every Codex session is backed by a rollout file (a JSONL file where each line represents a timestamped item from the session), which enables session resume, forensics, and conversation branching."

**State Management (2026 Updates):**
- Model client lifecycle refactored to be session-scoped
- Reduced implicit client state
- Shell executions receive `CODEX_THREAD_ID` for session/thread detection
- Session-scoped "Allow and remember" for MCP/App tool approvals

### Memory System

**Memory Architecture (February 2026):**
- Initial memory plumbing (API client + local persistence)
- Thread memory summaries support

**Memory Strategy (From Cookbook):**
> "For memory management, use Markdown notes for flexible, human-readable memory. Memory Distillation captures dynamic insights during active turns by writing session notes via a dedicated tool. Memory Consolidation merges session-level notes into a dense set of global memories."

### Compaction

**Context Window Extension:**
> "Compaction unlocks significantly longer effective context windows, where user conversations can persist for many turns without hitting context window limits."

> "GPT-5.1-Codex-Max is built to operate across multiple context windows through a process called compaction, coherently working over millions of tokens in a single task."

**Known Issues:**
- Issue #10823: "Unable to compact the context in a VERY long running session"
- Indicates compaction can fail on extremely long sessions

### Skills: Progressive Disclosure

**Context-Efficient Design:**
> "Skills use progressive disclosure to manage context efficiently: Codex starts with each skill's metadata (name, description, file path, and optional metadata from agents/openai.yaml). This approach ensures that only essential information is loaded initially, with additional content loaded on-demand."

**Architecture:**
> "The overall design philosophy prioritizes skills sharing the context window with everything else Codex needs: system prompt, conversation history, other Skills' metadata, and the actual user request."

**2026 Updates:**
- Live skill update detection (file changes picked up without restart)
- Skills loadable from `.agents/skills`
- Nested-folder markers supported
- Clearer relative-path instructions

---

## 3. Task Distribution & Scheduling

### Agent Loop Architecture

**Core Loop:**
> "The Codex harness consists of a loop that takes input from a user and uses an LLM to generate tool calls or responses back to the user. The agent invokes tools with specified inputs and collects the output, while other events indicate reasoning outputs from the LLM, which are typically steps in a plan. Both tool calls and reasoning are then appended to the initial prompt, which is passed to the LLM again for more iterations of reasoning or tool calling."

**InfoQ Coverage (February 2026):**
OpenAI began an article series on Codex CLI internals, detailing the agent loop implementation.

### Parallel vs Sequential Execution

**Execution Rules:**
> "Agent calls are executed in parallel, while other tool calls are processed sequentially to respect dependencies."

**Practical Application:**
When orchestrating multi-agent workflows, agent spawns happen concurrently, but within a single agent, tool calls maintain dependency order.

### Orchestration with Agents SDK

**Multi-Agent Coordination:**
> "By exposing the CLI as a Model Context Protocol (MCP) server and orchestrating it with the OpenAI Agents SDK, you can create deterministic, auditable workflows that scale from a single agent to a complete software delivery pipeline."

**Coordination Pattern:**
> "The project manager agent writes REQUIREMENTS.md, TEST.md, and AGENT_TASKS.md, then coordinates hand-offs across the designer, frontend, backend, and tester agents. Each agent writes scoped artifacts in its own folder before handing control back to the project manager."

**Parallel Hand-Offs:**
> "When design specifications exist, the system hands off in parallel to both a Frontend Developer agent and a Backend Developer agent with relevant documentation, then waits for both to produce their outputs before proceeding."

### Traceability

**Automatic Trace Recording:**
> "Codex automatically records traces that capture every prompt, tool call, and hand-off. After the multi-agent run completes, open the Traces dashboard to inspect the execution timeline."

This enables:
- Debugging multi-agent workflows
- Auditing agent decisions
- Performance analysis
- Compliance verification

### Automations Scheduling

**Background Task Scheduling:**
> "Automations allow Codex to run scheduled background tasks such as issue triage, bug detection, and release summaries, with completed tasks sent to a review queue so developers can step in only when needed."

**Characteristics:**
- Continuous monitoring without manual trigger requirements
- Operates on natural language instructions with AI reasoning (not rigid scripts)
- Combines multiple skills
- Adapts to changing conditions
- Results surface in review queue

**Use Cases:**
- Daily issue triage
- CI failure summarization
- Release briefs generation
- Bug detection sweeps
- Alert monitoring

---

## 4. Validation & Quality Control

### Code Review System

**Intelligent Review:**
> "Codex includes code review capabilities trained to catch critical flaws. Unlike static analysis tools, it matches the stated intent of a PR to the actual diff, reasons over the entire codebase and dependencies, and executes code and tests to validate behavior."

**Quality Philosophy:**
> "When deploying the code review agent, OpenAI explicitly accepted a measured tradeoff: modestly reduced recall in exchange for high signal quality and developer trust. They optimize for signal-to-noise first, and only then push recall without compromising reliability."

### Review Queue

**Automation Oversight:**
> "When automations finish, the results land in a review queue so you can jump back in and continue working if needed. This feature allows developers to set scheduled tasks for Codex and review the outputs asynchronously."

**Human-in-the-Loop Design:**
Codex surfaces completed work for developer approval rather than auto-committing, maintaining human oversight on critical decisions.

### Testing & Validation

**Quality Standards:**
> "Codex raises baseline quality with more thorough designs, comprehensive testing, and high-signal code review—so issues are caught early and your team ships with confidence."

**Verification Mechanisms:**
> "OpenAI prioritized security and transparency when designing Codex so users can verify its outputs, and users can check Codex's work through citations, terminal logs and test results."

### Benchmarking

**SWE-Bench Pro:**
Measures ability to resolve real-world GitHub issues from popular repositories.

- GPT-5.3-Codex: **56.8%**
- GPT-5.2-Codex: 56.4%
- GPT-5.2: 55.6%

**Terminal-Bench 2.0:**
Evaluates command-line task completion and shell interaction.

- GPT-5.3-Codex: **77.3%**

**OSWorld-Verified:**
Tests operating system-level task execution.

- GPT-5.3-Codex: **64.7%**

**Real-World Validation:**
Sora Android app case study (85% AI-written, 99.9% crash-free) demonstrates production-quality code generation at scale.

### CI/CD Integration

**Automatic Fix Generation:**
> "You can embed the OpenAI Codex CLI into your CI/CD pipeline so that when your builds or tests fail, Codex automatically generates and proposes fixes."

**GitHub Actions Example:**
- Cookbook: `cookbook.openai.com/examples/codex/autofix-github-actions`
- Repository: `github.com/openai/codex-action`

**Quality Gates:**
> "Use cases include: Automating Codex feedback on pull requests or releases without managing the CLI yourself, gating changes on Codex-driven quality checks as part of your CI pipeline, and running repeatable Codex tasks like code review, release prep, and migrations from a workflow file."

---

## 5. Mailbox / Inbox Implementation

### NOT DISCLOSED: Direct Inter-Agent Communication

OpenAI Codex does **not** implement a traditional mailbox/inbox system for inter-agent communication. The architecture is **isolation-first**, with coordination happening through:

1. **File Artifacts** (when using orchestration frameworks)
2. **Worktree Isolation** (Git-based state separation)
3. **Review Queue** (for automation outputs)
4. **Orchestration Layer** (Agents SDK hand-offs)

### Isolation Model

**Core Design:**
> "Each task is processed independently in a separate, isolated environment preloaded with your codebase. For multi-agent scenarios, agents run in parallel threads with worktree isolation, with each agent operating on an isolated code copy through built-in worktree support."

**No Shared Memory:**
Agents do not share memory directly. Each operates in its own:
- Container (cloud) or sandbox (local)
- Worktree (Git isolation)
- Context window (separate conversation history)

### Coordination via Artifacts

**When Using Agents SDK:**
> "Each agent writes scoped artifacts in its own folder before handing control back to the project manager."

**Artifact Examples:**
- `REQUIREMENTS.md` - Specifications
- `TEST.md` - Test plans
- `AGENT_TASKS.md` - Task breakdown
- Code files in designated folders

**Hand-Off Mechanism:**
1. Agent A completes task, writes artifact
2. Orchestrator (project manager agent) reads artifact
3. Orchestrator delegates to Agent B with artifact as context
4. Agent B reads artifact, continues work

This is **file-based asynchronous communication**, not a message-passing system.

### Review Queue (Human Coordination)

**Purpose:**
Surface agent outputs to developers for approval/rejection.

**Use Cases:**
- Automation results
- Code review feedback
- PR suggestions
- Long-running task completions

**Implementation:**
Appears to be UI-based (desktop app, web) rather than API-accessible programmatically. Details on internal implementation NOT DISCLOSED.

### App Server Notifications

**Bidirectional Communication:**
> "The protocol is fully bidirectional, with a typical thread having a client request and many server notifications, and the server can initiate requests when the agent needs input like an approval."

**JSON-RPC Notification Types:**
While specific notification schemas are NOT DISCLOSED, the architecture supports:
- Agent status updates
- Tool call results
- Approval requests
- Progress indicators
- Error notifications

This is **agent-to-UI communication**, not agent-to-agent.

### Community Implementations

**Subagents (Community Feature Request #2604, PR #3655):**
Indicates that sophisticated inter-agent communication is a **community-driven enhancement** rather than core feature.

**Codex Kaioken (Fork):**
> "Includes subagents that spawn specialized agents for exploration, execution, or research, with each streaming in its own pane so you can watch tool calls and diffs in real-time."

This demonstrates that **advanced multi-agent coordination happens in forks/wrappers**, not in core Codex.

### MCP Integration (External Tools)

**Model Context Protocol:**
Enables communication between Codex and **external tools**, not between Codex agents.

> "Model Context Protocol (MCP) connects models to tools and context, allowing you to give Codex access to third-party documentation or let it interact with developer tools like your browser or Figma."

**Codex as MCP Server:**
> "You can run Codex as an MCP server and connect it from other MCP clients, exposing two tools—codex() to start a conversation and codex-reply() to continue one."

This is **external orchestration** (another system calling Codex), not inter-Codex-agent messaging.

---

## 6. Open Source Artifacts & Code

### Primary Repository: github.com/openai/codex

**License:** Apache-2.0

**Repository Contents:**
- Codex CLI source code
- Sandbox implementations (Linux Bubblewrap, Windows, macOS)
- Documentation (security, configuration, integration)
- GitHub Actions integration

**Key Files/Directories:**
- `docs/sandbox.md` - Sandbox architecture
- `docs/windows_sandbox_security.md` - Windows security implementation
- Source code for CLI agent loop
- FFI bindings for Bubblewrap (Linux)

### Sandbox Implementation Details

#### Linux: Bubblewrap Integration

**Code Location:** Vendored in repository

**Implementation Details (from changelog):**
> "Codex added vendored Bubblewrap + FFI wiring in the Linux sandbox as groundwork for upcoming runtime integration." (PR #10413: "feat(linux-sandbox): vendor bubblewrap and wire it with FFI" by @viyatb-oai)

**Technical Approach:**
- **Bubblewrap (bwrap):** Container technology for Linux
- **FFI (Foreign Function Interface):** Rust calling C libraries
- **Vendored:** Bubblewrap source included in Codex repo (no external dependency)
- **Gated path:** Feature flag for experimental usage

**Security Features:**
- Filesystem isolation
- Mount namespace separation
- User namespace restrictions
- Capability dropping

**NOT DISCLOSED:** Exact Rust FFI implementation code (source files not directly browsable via web search).

#### Windows: Restricted Token + Allowlist

**Documentation:** `github.com/openai/codex/blob/main/docs/windows_sandbox_security.md`

**Security Mechanisms:**
> "When commands run via codex sandbox windows, the launcher configures a restricted Windows token and an allowlist policy scoped to workspace roots, with writes blocked everywhere except inside those roots and common escape vectors such as alternate data streams and UNC paths denied proactively."

**Implementation Approach:**
- **Restricted Token:** Windows security token with limited privileges
- **Allowlist Policy:** Explicit list of permitted filesystem paths
- **Workspace Roots:** Designated safe directories
- **Blocked Vectors:**
  - Alternate Data Streams (ADS)
  - UNC paths (`\\server\share`)
  - Other known escape techniques

**Experimental Status:**
Windows sandbox available via WSL (Windows Subsystem for Linux) in CLI, full native implementation in development for desktop app.

**NOT DISCLOSED:** Source code implementation details (Windows-specific Rust code).

#### macOS: OS-Level Primitives

**Reason for macOS-First Launch:**
> "OpenAI's Alexander Embiricos explained that the company needs more time to get 'really solid sandboxing working on Windows, where there are fewer OS-level primitives for it.' The Codex app needs robust security controls to safely run AI-generated code on your machine, and Windows just doesn't have the same built-in tools that macOS offers for this kind of isolation."

**Implied Technologies:**
- **App Sandbox** (Apple's sandboxing framework)
- **Entitlements** (Fine-grained permission system)
- **XPC Services** (Inter-process communication isolation)
- **Code Signing** (Verification of executed code)

**NOT DISCLOSED:** Specific macOS sandbox implementation code.

### Skills Catalog: github.com/openai/skills

**License:** NOT DISCLOSED (repository is public, license not found in search results)

**Repository Structure:**
```
skills/
├── .system/           # Auto-installed in latest Codex
│   └── skill-creator/ # Meta-skill for creating skills
├── .curated/          # Installable via $skill-installer
├── .experimental/     # Requires explicit folder specification
└── README.md
```

**Key Skills:**
- `skill-creator` - Creates new skills
- `create-plan` - Planning and execution plans
- Notion integration templates
- Research documentation workflows
- Competitor analysis templates

**Skill Format Example:**
File: `skills/.system/skill-creator/SKILL.md`
- YAML frontmatter (name, description, metadata)
- Markdown body (instructions, examples)
- Optional bundled resources (scripts, templates)

**Installation:**
```bash
# Curated skills (by name)
$skill-installer skill-name

# Experimental skills (by path)
$skill-installer skills/.experimental/my-skill
```

**Progressive Loading:**
> "Only preloads yaml front-matter, can lazy load more markdown files as needed."

### GitHub Actions Integration: github.com/openai/codex-action

**Usage Example:**
```yaml
- uses: openai/codex-action@v1
  with:
    api-key: ${{ secrets.OPENAI_API_KEY }}
    prompt: "Review this PR and suggest improvements"
    safety-strategy: drop-sudo  # Linux/macOS
```

**Security Strategies:**
- `drop-sudo` (default, Linux/macOS) - Revokes sudo before Codex runs
- `unsafe` (Windows) - Required for Windows runners

**Key Capabilities:**
- Install Codex CLI in CI environment
- Start Responses API proxy
- Run `codex exec` with specified permissions
- Post reviews to PRs
- Apply patches automatically

**NOT DISCLOSED:** Full action source code (repository visible but source files not browsable via search).

### Additional Repositories (Third-Party)

#### Codex Kaioken (Community Fork)
**Features:**
- Subagents with specialized roles (exploration, execution, research)
- Real-time streaming in separate panes
- Tool call and diff visualization

**NOT DISCLOSED:** Exact repository URL or implementation details.

#### ymichael/open-codex
**Description:** Lightweight coding agent alternative
**NOT DISCLOSED:** Relationship to official Codex (independent implementation or fork unclear).

#### heilcheng/awesome-agent-skills
**Description:** Curated list of skills, tools, tutorials for AI coding agents
**Scope:** Cross-platform (Claude, Codex, Copilot, VS Code)
**URL:** `github.com/heilcheng/awesome-agent-skills`

### OpenAI Agents SDK

**Python Implementation:**
Repository: `openai.github.io/openai-agents-python/`

**Key Features:**
- Multi-agent orchestration framework
- MCP integration
- Trace recording
- Parallel agent execution
- Hand-off mechanisms

**Code Examples Available:**
- `cookbook.openai.com/examples/codex/build_code_review_with_codex_sdk`
- Multi-agent project manager pattern
- Frontend/backend parallel delegation

**NOT DISCLOSED:** Full SDK source code repository URL or comprehensive API documentation (only cookbook examples found).

### MCP Servers

**Official Documentation:**
- `developers.openai.com/codex/mcp/`
- `platform.openai.com/docs/mcp`

**Community MCP Servers:**
Codex supports third-party MCP servers for tool integration.

**Configuration:**
```bash
codex mcp add \
  --name my-server \
  --command "python mcp_server.py" \
  --env VAR=value
```

**Recent Updates (2026 Changelog):**
- Caching for MCP actions (reduced load latency)
- Session-scoped approvals

**NOT DISCLOSED:** Reference implementations of MCP servers for Codex (only protocol documentation available).

### Developer Resources

**OpenAI Cookbook (cookbook.openai.com):**
- Codex prompting guides
- Multi-agent orchestration examples
- CI/CD integration tutorials
- PLANS.md workflow documentation
- Code review automation examples

**Developer Documentation (developers.openai.com/codex/):**
- API reference (App Server JSON-RPC)
- Security configuration
- Skills documentation
- MCP integration guides
- Changelog with technical details

**System Cards & Whitepapers:**
- GPT-5.1-Codex-Max System Card: `cdn.openai.com/pdf/2a7d98b1-57e5-4147-8d0e-683894d782ae/5p1_codex_max_card_03.pdf`
  - Model capabilities
  - Safety evaluations
  - Known limitations

### Open Source Gaps

**NOT DISCLOSED / NOT OPEN SOURCE:**
1. **App Server Implementation** - JSON-RPC server code (protocol documented, implementation not released)
2. **Model Weights** - GPT-5.x-Codex models proprietary
3. **Desktop App Source** - macOS app binary-only distribution
4. **Core Agent Loop** - High-level architecture documented, full implementation not released
5. **Compaction Algorithm** - How multi-context-window operation works
6. **Memory Consolidation** - Exact algorithms for memory distillation
7. **Cloud Container Infrastructure** - Isolation implementation for cloud execution
8. **Review Queue Backend** - How automation results surface to UI
9. **Trace Recording System** - Internal format and storage mechanism
10. **Prompt Templates** - Full system prompts for GPT-5.x-Codex models

### Hacker News Discussions

**Relevant Threads (news.ycombinator.com):**
- GPT-5.3-Codex launch discussion (Item #46902638)
- Unrolling the Codex agent loop (Item #46737630)
- Skills announcement (Item #46334424)
- Codex CLI free tier (Item #46870868)
- Codex Kaioken fork (Item #46417772)

**Key Insights from HN:**
- Community building forks with subagent support (Codex Kaioken)
- Discussion of Skills composability and context efficiency
- Comparisons with Claude Code and other coding agents
- Feedback on usage limits and pricing

---

## Summary: Technical Architecture Insights

### What IS Open Source:
1. **Codex CLI core** (github.com/openai/codex, Apache-2.0)
2. **Sandbox implementations** (Linux Bubblewrap FFI, Windows security docs)
3. **Skills catalog** (github.com/openai/skills, markdown-based)
4. **GitHub Actions integration** (github.com/openai/codex-action)
5. **Documentation** (security, configuration, integration guides)

### What is DOCUMENTED but Not Open Source:
1. **App Server protocol** (JSON-RPC spec available, implementation proprietary)
2. **Agent loop architecture** (high-level design documented, code not released)
3. **MCP integration** (protocol spec open, Codex implementation details limited)
4. **Orchestration patterns** (cookbook examples, not full SDK source)

### What is NOT DISCLOSED:
1. **Model architecture** (GPT-5.x-Codex internals)
2. **Full system prompts** (prompting guides available, exact templates not public)
3. **Compaction algorithms** (capability documented, implementation secret)
4. **Memory system internals** (API exists, backend not detailed)
5. **Cloud infrastructure** (container isolation described, implementation proprietary)
6. **Inter-agent communication** (isolation-first architecture, no built-in mailbox system)
7. **Review queue backend** (UI feature exists, internal implementation not documented)

### Key Differentiators:
- **Isolation-first multi-agent** (no mailbox, coordination via artifacts/orchestration layer)
- **Worktrees-based parallelism** (Git-native isolation)
- **Bidirectional JSON-RPC** (unified App Server for all surfaces)
- **Progressive disclosure** (Skills, memory, context management)
- **Open sandbox layer** (enterprise auditability)
- **Extended thinking** (adjustable reasoning effort)
- **PLANS.md workflow** (7+ hour autonomous tasks)

---

## Sources

- [Codex Prompting Guide | OpenAI Cookbook](https://developers.openai.com/cookbook/examples/gpt-5/codex_prompting_guide/)
- [GPT-5.1-Codex-Max Prompting Guide | OpenAI Cookbook](https://cookbook.openai.com/examples/gpt-5/gpt-5-1-codex-max_prompting_guide)
- [Using PLANS.md for multi-hour problem solving | OpenAI Cookbook](https://cookbook.openai.com/articles/codex_exec_plans)
- [Unrolling the Codex agent loop | OpenAI](https://openai.com/index/unrolling-the-codex-agent-loop/)
- [Unlocking the Codex harness: how we built the App Server | OpenAI](https://openai.com/index/unlocking-the-codex-harness/)
- [OpenAI Begins Article Series on Codex CLI Internals - InfoQ](https://www.infoq.com/news/2026/02/codex-agent-loop/)
- [Introducing GPT-5.3-Codex | OpenAI](https://openai.com/index/introducing-gpt-5-3-codex/)
- [Introducing GPT-5.2-Codex | OpenAI](https://openai.com/index/introducing-gpt-5-2-codex/)
- [Building more with GPT-5.1-Codex-Max | OpenAI](https://openai.com/index/gpt-5-1-codex-max/)
- [Codex Changelog](https://developers.openai.com/codex/changelog/)
- [Codex Security Documentation](https://developers.openai.com/codex/security/)
- [Codex App Server Documentation](https://developers.openai.com/codex/app-server/)
- [Agent Skills | Codex](https://developers.openai.com/codex/skills)
- [Model Context Protocol | Codex](https://developers.openai.com/codex/mcp/)
- [Use Codex with the Agents SDK](https://developers.openai.com/codex/guides/agents-sdk/)
- [GitHub - openai/codex](https://github.com/openai/codex)
- [GitHub - openai/skills](https://github.com/openai/skills)
- [GitHub - openai/codex-action](https://github.com/openai/codex-action)
- [Codex GitHub Action Documentation](https://developers.openai.com/codex/github-action/)
- [Use Codex CLI to automatically fix CI failures | OpenAI Cookbook](https://cookbook.openai.com/examples/codex/autofix-github-actions)
- [Build Code Review with the Codex SDK | OpenAI Cookbook](https://cookbook.openai.com/examples/codex/build_code_review_with_codex_sdk)
- [Context Engineering for Personalization - OpenAI Agents SDK](https://developers.openai.com/cookbook/examples/agents_sdk/context_personalization)
- [Automations | Codex](https://developers.openai.com/codex/app/automations/)
- [A Practical Approach to Verifying Code at Scale | OpenAI Alignment](https://alignment.openai.com/scaling-code-verification/)
- [Codex Usage Limits Discussion | Apidog Blog](https://apidog.com/blog/solutions-to-codex-usage-limits/)
- [Codex Pricing Guide 2026 | eesel.ai](https://www.eesel.ai/blog/codex-pricing)
- [Feature Request: High-Quality Sub-Agent Collaboration · Issue #9846](https://github.com/openai/codex/issues/9846)
- [Subagent Support · Issue #2604](https://github.com/openai/codex/issues/2604)
- [Increase effective context window · Issue #9429](https://github.com/openai/codex/issues/9429)
- [Unable to compact context in long session · Issue #10823](https://github.com/openai/codex/issues/10823)
- [GPT-5.3-Codex | Hacker News](https://news.ycombinator.com/item?id=46902638)
- [Unrolling the Codex agent loop | Hacker News](https://news.ycombinator.com/item?id=46737630)
- [Skills Officially Comes to Codex | Hacker News](https://news.ycombinator.com/item?id=46334424)
- [Codex Kaioken Fork | Hacker News](https://news.ycombinator.com/item?id=46417772)
