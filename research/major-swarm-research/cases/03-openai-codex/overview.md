# OpenAI Codex: Multi-Agent Coding System - Overview

## 1. Overview & Scale

### Product Overview

OpenAI Codex is a multi-surface AI coding system that launched as a desktop application for macOS on **February 2, 2026**, positioning itself as a "command center for agents" that transforms software development from single-agent collaboration into managing a team of autonomous AI workers.

**Key Surfaces:**
- **Codex Desktop App** (macOS, launched Feb 2, 2026; Windows in development)
- **Codex CLI** (Terminal-based interface)
- **IDE Extensions** (VS Code, JetBrains, Xcode)
- **Web Runtime** (Browser-based interface)
- **API Access** (Coming soon for GPT-5.3-Codex)

All surfaces are powered by the same **Codex harness** through the **Codex App Server**, a bidirectional JSON-RPC API that serves as a unified backend.

### Scale & Adoption

> "More than a million developers used Codex in the past month, with usage nearly doubling since the launch of GPT-5.2-Codex in mid-December, and overall Codex usage has grown more than 20x since August."

**Enterprise Adoption:**
- Startups: Harvey, Sierra
- Large Enterprises: Cisco
- Internal OpenAI usage across multiple teams

### GPT-5.3-Codex Model (Launched February 5, 2026)

The latest and most capable agentic coding model, GPT-5.3-Codex represents a significant leap:

**Performance:**
- **25% faster** than GPT-5.2-Codex
- **SWE-Bench Pro**: 56.8% (vs. 56.4% for GPT-5.2-Codex, 55.6% for GPT-5.2)
- **Terminal-Bench 2.0**: 77.3%
- **OSWorld-Verified**: 64.7%

**Key Innovation:**
> "GPT-5.3-Codex is the first model that was instrumental in creating itself. The Codex team used early versions to debug its own training, manage its own deployment, and diagnose test results and evaluations during development."

**Model Characteristics:**
- Unifies frontier coding performance of GPT-5.2-Codex with reasoning and professional knowledge of GPT-5.2
- Interactive collaboration: "Much like a colleague, you can steer and interact with GPT-5.3-Codex while it's working, without losing context"
- Extended thinking capabilities with adjustable reasoning effort (Standard, High, Extra High 'xhigh')

### Case Study: Sora Android App

**Timeline:** October 8 - November 5, 2025 (28 days)
**Team Size:** 4 engineers
**Token Consumption:** ~5 billion tokens
**AI Code Contribution:** 85%
**Model Used:** GPT-5.1-Codex (early version)

**Results:**
- Launched publicly in November 2025
- #1 on Google Play Store on Day 1
- 1M+ videos generated in first 24 hours
- **99.9% crash-free rate**

**Development Strategy:**
> "For a project that OpenAI estimates was 85% written by Codex, a carefully planned foundation avoided costly backtracking and refactoring. The team wrote a few representative features end-to-end and documented project-wide patterns. By pointing Codex to representative features, it was able to work more independently within their standards."

---

## 2. Architecture

### Cloud Sandboxes & Isolation

**Codex Cloud:**
- Runs in **isolated OpenAI-managed containers**
- Prevents access to host system or unrelated data
- Container-per-task architecture for security

**Local CLI/IDE Extensions:**
- OS-level sandbox enforcement
- Default restrictions:
  - No network access
  - Write permissions limited to active workspace
- Expandable access when intentionally needed

### Parallel Agents

**Multi-Agent Capabilities:**
- Desktop app functions as "command center" for managing multiple agents
- Agents can run **up to 30 minutes autonomously** before returning completed code
- Each agent operates in separate threads organized by projects
- Developers can delegate multiple tasks simultaneously and switch between contexts without losing state

**Worktrees Implementation:**
> "The app includes built-in support for worktrees, so multiple agents can work on the same repo without conflicts, with each agent working on an isolated copy of your code, allowing you to explore different paths without needing to track how they impact your codebase."

**Key Architecture Features:**
- Each agent works on isolated code copies via Git worktrees
- Changes reviewed before merging to prevent conflicts
- Agents don't write to local git state until explicitly approved
- Enables parallel exploration of architectural alternatives

### Automations (Background Tasks)

> "With Automations, Codex works unprompted, picking up routine but important work like issue triage, alert monitoring, CI/CD, and more, so you can stay focused on building."

**Capabilities:**
- Daily issue triage
- Finding and summarizing CI failures
- Generating daily release briefs
- Bug detection
- Scheduled background execution
- Results sent to review queue for developer oversight

**Security Considerations:**
> "If your sandbox mode is full access, background automations carry elevated risk, as Codex may modify files, run commands, and access network without asking. Consider updating sandbox settings to workspace write, and using rules to selectively define which commands the agent can run with full access."

### App Server Architecture

**Codex App Server:**
- Bidirectional **JSON-RPC 2.0** protocol
- Long-lived process hosting Codex core threads
- Four main components:
  1. **stdio reader** - Input handling
  2. **Codex message processor** - Translation layer
  3. **Thread manager** - Session orchestration
  4. **Core threads** - One per active session

**Protocol Features:**
- Fully bidirectional communication
- Streams JSONL over stdio
- Server can initiate requests when agent needs input (e.g., approvals)
- Translates client JSON-RPC requests into Codex core operations
- Transforms low-level events into stable, UI-ready JSON-RPC notifications

**Cross-Platform Support:**
Powers all first-party and third-party integrations uniformly.

---

## 3. Communication & Coordination

### Inter-Agent Communication Model

**Isolation-First Design:**
Each agent operates in an isolated container/worktree with minimal direct inter-agent communication. The architecture prioritizes **isolation over coordination**.

**Coordination Mechanisms (via Agents SDK):**
When using the OpenAI Agents SDK for multi-agent orchestration:

> "The project manager agent writes REQUIREMENTS.md, TEST.md, and AGENT_TASKS.md, then coordinates hand-offs across the designer, frontend, backend, and tester agents. Each agent writes scoped artifacts in its own folder before handing control back to the project manager."

**Parallel Execution:**
> "When design specifications exist, the system hands off in parallel to both a Frontend Developer agent and a Backend Developer agent with relevant documentation, then waits for both to produce their outputs before proceeding."

**Execution Model:**
- Agent calls executed in **parallel**
- Other tool calls processed **sequentially** to respect dependencies
- Traceability: "Codex automatically records traces that capture every prompt, tool call, and hand-off"

### Shared State Management

**Session-Scoped State:**
- Model client lifecycle refactored to be session-scoped
- Reduced implicit client state
- Shell executions receive `CODEX_THREAD_ID` for session detection
- Each task processed independently in separate isolated environment preloaded with codebase

**Multi-Agent State:**
- Agents run in parallel threads with worktree isolation
- Each agent operates on isolated code copy
- No direct shared memory between agents
- Coordination via file artifacts (REQUIREMENTS.md, TEST.md, etc.) when using orchestration frameworks

### Community Implementations

**Subagent Orchestration (Community PR #3655):**
Feature request and implementation for high-quality sub-agent collaboration built into Codex, indicating that native subagent orchestration is a community-driven enhancement rather than core feature.

**Third-Party Orchestration:**
Projects like **Goose** (by Block/Square) implement subagent support, demonstrating that sophisticated multi-agent coordination often happens at the orchestration layer above Codex core.

---

## 4. Git & Code Integration

### Worktrees Integration

**Core Feature:**
Built-in Git worktrees support enables parallel development without merge conflicts.

**Implementation:**
- Each agent gets isolated copy of repository
- Multiple agents work on same repo simultaneously
- Changes isolated until explicit approval
- Exploration of different architectural paths without impacting main codebase

**Workflow:**
1. Agent spawns in dedicated worktree
2. Agent makes changes in isolation
3. Developer reviews changes
4. Upon approval, changes merge to main codebase
5. Agents don't write to local git state until approved

### PR Workflow

**GitHub Integration:**
- Codex available as coding agent for Copilot Pro+ and Enterprise customers
- Can start agent sessions from github.com, GitHub Mobile, VS Code
- Work assigned directly from issues, pull requests, Agents tab
- Launched February 4, 2026: "Claude by Anthropic and OpenAI Codex are now available as coding agents"

**GitHub Actions Integration (openai/codex-action):**

Repository: `github.com/openai/codex-action`

> "The Codex GitHub Action (openai/codex-action@v1) can be used to run Codex in CI/CD jobs, apply patches, or post reviews from a GitHub Actions workflow."

**Capabilities:**
- Automate Codex feedback on PRs or releases
- Gate changes on Codex-driven quality checks in CI pipeline
- Run repeatable tasks (code review, release prep, migrations) from workflow files
- Automatically generate and propose fixes when builds/tests fail

**Security:**
- `drop-sudo` strategy (default) revokes sudo before invoking Codex
- Runs on Linux/macOS runners
- Windows requires `safety-strategy: unsafe`

### Code Review & Quality Control

**Review Capabilities:**
> "Codex includes code review capabilities trained to catch critical flaws. Unlike static analysis tools, it matches the stated intent of a PR to the actual diff, reasons over the entire codebase and dependencies, and executes code and tests to validate behavior."

**Quality Philosophy:**
> "When deploying the code review agent, OpenAI explicitly accepted a measured tradeoff: modestly reduced recall in exchange for high signal quality and developer trust. They optimize for signal-to-noise first, and only then push recall without compromising reliability."

---

## 5. What Worked & What Failed

### What Worked

#### 1. **Scaling to 1M+ Developers**
- 20x usage growth since August 2025
- Nearly doubled usage since GPT-5.2-Codex launch (mid-December 2025)
- Successful enterprise adoption (Cisco, Harvey, Sierra)

#### 2. **Sora Android App Case Study**
- 85% AI-written code with 99.9% crash-free rate
- 4 engineers + Codex shipped in 28 days
- #1 Google Play Store app on launch day
- Validates multi-month autonomous agent tasks

#### 3. **Benchmark Performance**
- GPT-5.3-Codex: 56.8% on SWE-Bench Pro (industry high)
- 77.3% on Terminal-Bench 2.0
- 25% performance improvement over GPT-5.2-Codex

#### 4. **Worktrees & Parallel Development**
Successfully enabled exploration of architectural alternatives without merge conflicts, traditionally forcing sequential experimentation.

#### 5. **Multi-Surface Unified Architecture**
App Server successfully powers web, CLI, desktop, and IDE extensions uniformly through single JSON-RPC API.

### What Failed / Challenges

#### 1. **Long-Running Agent Limitations**

> "Thirty-minute agent sessions revealed that users often can't precisely specify complex tasks upfront, requiring new interaction patterns where agents first generate plans for user approval before execution."

> "The models also developed human-like limitations—occasionally giving up on overly complex tasks with messages like 'sorry, I don't have enough time to do this.'"

#### 2. **Windows Sandbox Delays**

**Reason for macOS-only launch:**
> "OpenAI's Alexander Embiricos explained that the company needs more time to get 'really solid sandboxing working on Windows, where there are fewer OS-level primitives for it.' The Codex app needs robust security controls to safely run AI-generated code on your machine, and Windows just doesn't have the same built-in tools that macOS offers for this kind of isolation."

**Status:** Windows version "coming soon" (as of February 2026), experimental WSL-based sandbox available in CLI.

#### 3. **Code Quality Issues**

> "Codex can generate incorrect, incomplete, or inefficient code, requiring human review always, especially for mission-critical systems."

> "Real-world codebases lack consistent testing frameworks, documentation standards and development practices for agents to rely on."

#### 4. **Context Management Challenges**

- Effective context window limited to 258k tokens (272k with 0.95 auto-compaction threshold)
- Community requests to expand to 350k tokens
- Performance is quadratic with conversation length without prompt caching
- Very long sessions struggle with compaction: "Unable to compact the context in a VERY long running session" (Issue #10823)

#### 5. **Security Vulnerabilities**

**Published CVE:**
- GHSA-w5fx-fh39-j5rw (September 19, 2025)
- Sandbox bypass due to bug in path configuration logic

#### 6. **Data Privacy Concerns**

> "There is fear of proprietary code being used to train OpenAI's models and a lack of unambiguous, easily accessible data privacy policies specifically for Codex interactions."

> "Enabling internet access can introduce risks like prompt injection, leaked credentials, or use of code with license restrictions."

#### 7. **IDE Integration Gaps**

> "The lack of a VSCode plugin makes Codex feel 'useless' to many developers, as workflows are IDE-rooted and a cloud or Github-bound tool feels clunky."

(Note: VS Code extension does exist, but community feedback suggests integration gaps remain.)

#### 8. **Usage Limits Friction**

> "Codex usage limits are quotas set by OpenAI to manage server load and ensure fair access, and when hit, you've maxed out your allowance for API calls, tokens, or compute time under your current plan."

Community reports of stricter limits after updates causing workflow disruptions.

---

## 6. Open Source & Artifacts

### Open Source Security Layer

**Announcement:**
> "OpenAI has launched a Codex desktop app for macOS, positioning it as a command centre for AI coding agents and pairing it with a native, **open-source and configurable system-level sandbox** to make autonomous development safer and more transparent for enterprises."

### Primary Repository: github.com/openai/codex

**License:** Apache-2.0

**Key Components:**
- Lightweight coding agent for terminal
- Cross-platform sandbox implementations
- Documentation for security, configuration, integration

### Sandbox Implementations

#### Linux: Bubblewrap Integration
> "Codex added vendored Bubblewrap + FFI wiring in the Linux sandbox as groundwork for upcoming runtime integration." (PR #10413: "feat(linux-sandbox): vendor bubblewrap and wire it with FFI" by @viyatb-oai)

**Features:**
- Gated Bubblewrap (bwrap) Linux sandbox path
- Filesystem isolation
- Foreign Function Interface (FFI) bindings
- Containerization technology for enhanced security

#### Windows: Security Whitepaper
Documentation: `docs/windows_sandbox_security.md` (github.com/openai/codex)

**Features:**
- Restricted Windows token configuration
- Allowlist policy scoped to workspace roots
- Write blocks outside workspace roots
- Proactive denial of:
  - Alternate data streams
  - UNC paths
  - Common escape vectors

**Raw Documentation:**
Available at `github.com/openai/codex/blob/main/docs/sandbox.md`

### Additional Repositories

#### 1. **github.com/openai/codex-action**
GitHub Actions integration for CI/CD workflows.

#### 2. **github.com/openai/skills**
**Skills Catalog for Codex**

**Organization:**
- `.system/` - Automatically installed skills (latest Codex)
- `.curated/` - Installable by name via `$skill-installer`
- `.experimental/` - Experimental skills requiring folder specification

**Skill Structure:**
- Directory with `SKILL.md` file
- Optional scripts and references
- Required: `name` and `description` fields

**Examples:**
- `skill-creator` - Meta-skill for creating new skills
- `create-plan` - Planning and execution plans
- Notion integration templates
- Research documentation workflows
- Competitor analysis templates

**Markdown Format:**
> "Skills are easy to author (at their most basic, just a markdown file) and context efficient by default (only preloads yaml front-matter, can lazy load more markdown files as needed). Every skill consists of a required SKILL.md file and optional bundled resources."

### MCP (Model Context Protocol) Integration

**Official Documentation:**
- `developers.openai.com/codex/mcp/`
- `openai.github.io/openai-agents-python/mcp/`

> "Model Context Protocol (MCP) connects models to tools and context, allowing you to give Codex access to third-party documentation or let it interact with developer tools like your browser or Figma."

**Codex as MCP Server:**
> "You can run Codex as an MCP server and connect it from other MCP clients, exposing two tools—codex() to start a conversation and codex-reply() to continue one."

**Integration Command:**
```bash
codex mcp add
```

**Recent Updates:**
- Session-scoped "Allow and remember" for MCP/App tool approvals
- Caching for MCP actions to reduce load latency

### Community Forks & Extensions

#### 1. **Codex Kaioken** (Community Fork)
> "A Codex CLI fork called Codex Kaioken includes subagents that spawn specialized agents for exploration, execution, or research, with each streaming in its own pane so you can watch tool calls and diffs in real-time."

#### 2. **open-codex** (github.com/ymichael/open-codex)
Lightweight coding agent alternative.

#### 3. **awesome-agent-skills** (github.com/heilcheng/awesome-agent-skills)
Curated list of skills, tools, tutorials for AI coding agents (Claude, Codex, Copilot, VS Code).

### Documentation & Resources

**Official OpenAI Resources:**
- Codex Changelog: `developers.openai.com/codex/changelog/`
- Cookbook: `cookbook.openai.com/` (examples, guides, prompting techniques)
- Security: `developers.openai.com/codex/security/`
- App Features: `developers.openai.com/codex/app/features/`

**Key Whitepapers:**
- GPT-5.1-Codex-Max System Card (November 18, 2025): `cdn.openai.com/pdf/2a7d98b1-57e5-4147-8d0e-683894d782ae/5p1_codex_max_card_03.pdf`

### Open Source Philosophy

The open-source security layer represents OpenAI's approach to enterprise transparency:

> "The open source sandbox component is a key differentiator, allowing enterprises to audit and customize the security layer for their specific needs."

This aligns with the broader industry trend toward auditable AI systems, particularly for code execution where security risks are high.

---

## Summary

OpenAI Codex represents a **production-scale multi-agent coding system** with:
- **1M+ active developers** across multiple surfaces
- **Up to 30-minute autonomous operation** per agent
- **Worktrees-based parallel development** without merge conflicts
- **85% AI-written code** in production apps (Sora Android)
- **Open-source sandbox layer** for enterprise auditability
- **Unified App Server architecture** powering all surfaces

Key differentiators: isolation-first multi-agent design, native Git worktrees integration, background automations, and bidirectional JSON-RPC orchestration layer enabling both cloud and local execution modes.

---

## Sources

- [Introducing Codex | OpenAI](https://openai.com/index/introducing-codex/)
- [Introducing the Codex app | OpenAI](https://openai.com/index/introducing-the-codex-app/)
- [Introducing GPT-5.3-Codex | OpenAI](https://openai.com/index/introducing-gpt-5-3-codex/)
- [How we used Codex to build Sora for Android in 28 days | OpenAI](https://openai.com/index/shipping-sora-for-android-with-codex/)
- [Unlocking the Codex harness: how we built the App Server | OpenAI](https://openai.com/index/unlocking-the-codex-harness/)
- [OpenAI launches a Codex desktop app for macOS | VentureBeat](https://venturebeat.com/orchestration/openai-launches-a-codex-desktop-app-for-macos-to-run-multiple-ai-coding)
- [OpenAI Shifts Toward Autonomous Team Model | DevOps.com](https://devops.com/openai-shifts-toward-autonomous-team-model-with-codex-desktop-launch/)
- [OpenAI Launches Codex With Open Source Security Layer | Open Source For You](https://www.opensourceforu.com/2026/02/openai-launches-codex-with-open-source-security-layer-for-parallel-ai-coding-agents/)
- [GitHub - openai/codex](https://github.com/openai/codex)
- [GitHub - openai/skills](https://github.com/openai/skills)
- [GitHub - openai/codex-action](https://github.com/openai/codex-action)
- [Codex Security Documentation](https://developers.openai.com/codex/security/)
- [Codex App Server Documentation](https://developers.openai.com/codex/app-server/)
- [OpenAI Reveals Sora Android Development | 36Kr](https://eu.36kr.com/en/p/3596374382297093)
- [Terminal-Bench Leaderboard](https://www.tbench.ai/leaderboard/terminal-bench/2.0)
