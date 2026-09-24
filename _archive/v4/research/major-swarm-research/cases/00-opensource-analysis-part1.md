# Open Source Analysis: Swarm Orchestration Frameworks & Tools - Part 1

**Research Date:** February 8, 2026
**Scope:** GitHub repositories, frameworks, and tools mentioned in Cursor FastRender, Anthropic Compiler, OpenAI Codex, and Kimi K2.5 case studies
**Methodology:** Extract mentions from research files → Web search for verification → Analyze stars/forks/relevance → Rate swarm orchestration value

---

## 1. MENTIONED IN RESEARCH FILES

### 1.1 Cursor FastRender Project

#### [FastRender Browser Engine](https://github.com/wilsonzlin/fastrender)
- **URL:** https://github.com/wilsonzlin/fastrender
- **Stars:** 1,400+ | **Forks:** 99 | **License:** NOT EXPLICITLY STATED
- **Last updated:** Active (January 2026)
- **What it is:** Complete browser rendering engine built autonomously by ~2,000 parallel AI agents over one week. Implements HTML/DOM parsing, CSS cascade, layout algorithms (flex, grid, table), text processing, painting, and JavaScript runtime in Rust.
- **Swarm relevance:** **HIGH** - This IS the output of a massive swarm experiment
- **Key files for swarm orchestration:**
  - `AGENTS.md` - Master prompt and constraints governing all agent behavior
  - `instructions/` - Workstream-specific guides (9 parallel tracks)
  - `progress/pages/` - Progress tracking across agents
  - `scripts/cargo_agent.sh` - Safety wrappers enforcing resource limits
  - `docs/philosophy.md` - "Correct pixels are the product" guiding principle
- **Notable:**
  - ~3M LOC generated across all iterations, 1.6M in final codebase
  - 30,000 commits at ~1,000 commits/hour sustained
  - 10M+ tool calls over one week
  - Only ~2.3% CI success rate (1,426/63,295 workflows)
  - Compiles Linux kernel, QEMU, FFmpeg, SQLite, PostgreSQL, Redis, Doom
  - Cost: "Trillions of tokens" (community estimates ~$14M)

**Architecture Insights:**
- Recursive hierarchical tree (Root Planner → Sub-Planners → Workers → Judge)
- Git worktrees for agent isolation
- File-based task locks (`current_tasks/` directory)
- Optimistic concurrency control with error tolerance philosophy
- "Constraints over instructions" prompting strategy

**Sources:**
- [FastRender GitHub](https://github.com/wilsonzlin/fastrender)
- [AGENTS.md](https://github.com/wilsonzlin/fastrender/blob/main/AGENTS.md)
- [Simon Willison Interview](https://simonwillison.net/2026/Jan/23/fastrender/)
- [Cursor Blog - Self-Driving Codebases](https://cursor.com/blog/self-driving-codebases)

---

#### [Void Editor](https://github.com/voideditor/void)
- **URL:** https://github.com/voideditor/void
- **Stars:** 28,200 | **Forks:** 2,300 | **License:** NOT SPECIFIED (check repo)
- **Last updated:** Active
- **What it is:** Open-source alternative to Cursor IDE. Fork of VS Code with AI agent support.
- **Swarm relevance:** **MEDIUM** - Single-agent focus with checkpoint/visualization features
- **Key files for swarm orchestration:** None (not designed for swarm)
- **Notable:**
  - AI agents on codebase
  - Checkpoint and visualize changes
  - Any model or host locally
  - Does NOT implement multi-agent swarm (single-agent tool)

**Sources:**
- [Void Editor GitHub](https://github.com/voideditor/void)
- [HelloGitHub](https://hellogithub.com/en/repository/voideditor/void)

---

#### [AGENTS.md Standard](https://agents.md/)
- **URL:** https://agents.md/
- **Stars:** N/A (website standard) | **Forks:** N/A | **License:** Community initiative
- **Last updated:** Ongoing
- **What it is:** Community initiative to standardize agent instruction format. "One prompt to rule them all" - reusable instructions across Copilot, Claude, Cursor, Codex.
- **Swarm relevance:** **HIGH** - Standardizes agent prompts for multi-tool compatibility
- **Key files for swarm orchestration:**
  - Format specification for AGENTS.md files
  - Adopted by FastRender (pioneering example)
  - Used across multiple community projects
- **Notable:**
  - Inspired by FastRender's successful agent coordination
  - Growing adoption in community
  - .cursorrules files follow similar patterns

**Sources:**
- [agents.md website](https://agents.md/)
- [Medium Article on AGENTS.md](https://medium.com/@genyklemberg/one-prompt-to-rule-them-all-how-to-reuse-the-same-markdown-instructions-across-copilot-claude-42693df4df00)

---

#### [Cursor Memory Bank](https://github.com/vanzan01/cursor-memory-bank)
- **URL:** https://github.com/vanzan01/cursor-memory-bank
- **Stars:** NOT DISCLOSED | **Forks:** NOT DISCLOSED | **License:** NOT SPECIFIED
- **Last updated:** Unknown
- **What it is:** Framework for persistent memory in Cursor. Modular, documentation-driven framework with custom Cursor modes (VAN, PLAN, CREATIVE, IMPLEMENT).
- **Swarm relevance:** **MEDIUM** - Memory management for single agents, not swarm coordination
- **Key files for swarm orchestration:** None (single-agent memory focus)
- **Notable:**
  - Persistent memory across sessions
  - Visual process maps
  - Custom agent modes

**Sources:**
- Mentioned in Cursor FastRender technical report

---

### 1.2 Anthropic C Compiler Project

#### [Claude Code (Anthropic)](https://github.com/anthropics/claude-code)
- **URL:** https://github.com/anthropics/claude-code
- **Stars:** 65,100 | **Forks:** 5,000 | **License:** Proprietary (with limited open source components)
- **Last updated:** Active (February 2026)
- **What it is:** Official Anthropic agentic coding tool that lives in your terminal. Understands codebases, handles git workflows, executes routine tasks through natural language.
- **Swarm relevance:** **HIGH** - Agent Teams feature enables lead/teammate orchestration
- **Key files for swarm orchestration:**
  - `~/.claude/teams/{name}/` - Team configuration and state
  - `~/.claude/teams/{name}/inboxes/{agent}.json` - Inter-agent messaging
  - `~/.claude/tasks/{team-name}/N.json` - Task DAG with dependency tracking
  - TeammateTool (13 operations for coordination)
  - Hooks system (`TeammateIdle`, `TaskCompleted`)
- **Notable:**
  - **Agent Teams:** Lead + teammates with in-process/tmux backends
  - **Plan approval mode:** Teammates submit plans, lead approves
  - **Delegate mode:** Restricts lead to coordination-only
  - **Context window:** 272K effective (400K model capacity)
  - **Docker isolation:** Fresh containers per session (compiler project)
  - **File-based locks:** `current_tasks/` for task claiming (compiler)
  - **16 agents → 100K LOC C compiler in 2 weeks at $20K cost**

**Architecture (Compiler Project):**
- Flat, non-hierarchical (16 equal peers)
- Docker containerization per agent
- Git worktrees for isolation
- Optimistic locking via git's atomic operations
- No orchestration agent ("pick next most obvious problem")
- GCC oracle strategy for parallelizing kernel compilation

**Architecture (Agent Teams Feature):**
- Lead/teammate hierarchy
- 13 TeammateTool operations (spawn, join, write, broadcast, shutdown, etc.)
- Task DAG with `blockedBy`/`blocks` arrays
- JSON inbox system for inter-agent messaging
- Plan approval workflow
- TeammateIdle/TaskCompleted hooks

**Sources:**
- [Claude Code GitHub](https://github.com/anthropics/claude-code)
- [Building a C Compiler with Parallel Claudes](https://www.anthropic.com/engineering/building-c-compiler)
- [Agent Teams Documentation](https://code.claude.com/docs/en/agent-teams)
- [Kieran Klaassen Gist (Full TeammateTool spec)](https://gist.github.com/kieranklaassen/4f2aba89594a4aea4ad64d753984b2ea)

---

#### [Claude Agent SDK - Python](https://github.com/anthropics/claude-agent-sdk-python)
- **URL:** https://github.com/anthropics/claude-agent-sdk-python
- **Stars:** 4,600 | **Forks:** NOT DISCLOSED | **License:** MIT
- **Last updated:** Active (February 2026)
- **What it is:** Official Python SDK for building agents with Claude. Renamed from "Claude Code SDK" to "Claude Agent SDK" (migration guide available).
- **Swarm relevance:** **HIGH** - Programmatic agent building with subagent support
- **Key files for swarm orchestration:**
  - `src/claude_agent_sdk/query.py` - Simple query interface
  - `src/claude_agent_sdk/client.py` - Full bidirectional client
  - `examples/hooks.py` - Hook examples for deterministic processing
  - MCP server support (in-process and external)
- **Notable:**
  - Codebase understanding
  - File editing operations
  - Command execution
  - Complex workflow orchestration
  - Custom tool integration via MCP
  - Hooks system for deterministic processing
  - Subagent spawning capabilities

**Sources:**
- [Claude Agent SDK Python GitHub](https://github.com/anthropics/claude-agent-sdk-python)
- [Migration Guide](https://docs.claude.com/en/docs/claude-code/sdk/migration-guide)

---

#### [Claude Agent SDK - TypeScript](https://github.com/anthropics/claude-agent-sdk-typescript)
- **URL:** https://github.com/anthropics/claude-agent-sdk-typescript
- **Stars:** 752 | **Forks:** 83 | **License:** MIT (likely)
- **Last updated:** Active
- **What it is:** TypeScript implementation of Claude Agent SDK.
- **Swarm relevance:** **HIGH** - Same capabilities as Python SDK
- **Key files for swarm orchestration:**
  - Agent query interface
  - Client library
  - Tool system (Read, Write, Bash, Browse)
  - MCP server support
  - Type definitions
- **Notable:**
  - Used by 527 projects
  - Full API compatibility with Python SDK
  - Documentation at docs.claude.com

**Sources:**
- [Claude Agent SDK TypeScript GitHub](https://github.com/anthropics/claude-agent-sdk-typescript)

---

#### [Awesome Claude Code](https://github.com/hesreallyhim/awesome-claude-code)
- **URL:** https://github.com/hesreallyhim/awesome-claude-code
- **Stars:** NOT DISCLOSED | **Forks:** NOT DISCLOSED | **License:** Community resource
- **Last updated:** Active
- **What it is:** Curated list of skills, hooks, slash-commands, agent orchestrators, applications, plugins for Claude Code.
- **Swarm relevance:** **HIGH** - Community aggregation of swarm tools/patterns
- **Key files for swarm orchestration:**
  - Agent orchestrator collections
  - Multi-agent workflow examples
  - Community skills and hooks
- **Notable:**
  - Community-maintained resource
  - Links to orchestration tools
  - Practical examples

**Sources:**
- [Awesome Claude Code GitHub](https://github.com/hesreallyhim/awesome-claude-code)

---

#### [Claude Swarm Orchestration (MaTriXy)](https://github.com/MaTriXy/claude-swarm-orchestration)
- **URL:** https://github.com/MaTriXy/claude-swarm-orchestration
- **Stars:** NOT DISCLOSED | **Forks:** NOT DISCLOSED | **License:** NOT SPECIFIED
- **Last updated:** Unknown
- **What it is:** Community documentation for teammate API.
- **Swarm relevance:** **HIGH** - Documents Claude Code swarm features
- **Key files for swarm orchestration:**
  - `docs/teammate-api.md` - Teammate API documentation
- **Notable:**
  - Community reverse-engineering of features
  - Practical API usage patterns

**Sources:**
- Mentioned in Anthropic compiler research

---

#### [Claude Code System Prompts (Piebald AI)](https://github.com/Piebald-AI/claude-code-system-prompts)
- **URL:** https://github.com/Piebald-AI/claude-code-system-prompts
- **Stars:** NOT DISCLOSED | **Forks:** NOT DISCLOSED | **License:** Community resource
- **Last updated:** Unknown
- **What it is:** Reverse-engineered system prompts for Claude Code tools.
- **Swarm relevance:** **MEDIUM** - Understanding how Claude Code prompts agents
- **Key files for swarm orchestration:**
  - `system-prompts/tool-description-teammatetool.md` - TeammateTool prompt
- **Notable:**
  - Shows how TeammateTool is presented to model
  - Community analysis of Anthropic's prompting strategies

**Sources:**
- Mentioned in Anthropic compiler research

---

### 1.3 OpenAI Codex Project

#### [OpenAI Codex CLI](https://github.com/openai/codex)
- **URL:** https://github.com/openai/codex
- **Stars:** NOT DISCLOSED in search | **Forks:** NOT DISCLOSED | **License:** Apache-2.0
- **Last updated:** Active (February 2026)
- **What it is:** Lightweight coding agent that runs in your terminal. Official OpenAI CLI published under Apache 2.0.
- **Swarm relevance:** **HIGH** - Supports parallel agents via worktrees, Automations
- **Key files for swarm orchestration:**
  - Sandbox implementations (Linux Bubblewrap, Windows, macOS)
  - `docs/sandbox.md` - Sandbox architecture
  - `docs/windows_sandbox_security.md` - Windows security implementation
  - FFI bindings for Bubblewrap (Linux)
  - Agent loop source code
- **Notable:**
  - **Parallel agents:** Up to 30-minute autonomous tasks per agent
  - **Worktrees integration:** Built-in Git worktrees for parallel development
  - **Automations:** Background tasks (issue triage, CI/CD monitoring, alerts)
  - **App Server:** Bidirectional JSON-RPC 2.0 protocol for all surfaces
  - **Isolation-first:** Each agent in separate container/worktree
  - **85% AI-written Sora Android app (99.9% crash-free) in 28 days**
  - **Open-source sandbox layer** for enterprise auditability
  - **GPT-5.3-Codex:** 56.8% SWE-Bench Pro, 77.3% Terminal-Bench 2.0

**Architecture:**
- Isolation-first multi-agent (no mailbox, coordination via artifacts)
- Worktrees-based parallelism (Git-native isolation)
- Bidirectional JSON-RPC App Server (unified backend)
- Cloud sandboxes (OpenAI-managed containers)
- Local CLI/IDE with OS-level sandbox enforcement
- Desktop app functions as "command center" for multiple agents
- Review queue for automation outputs

**Sources:**
- [OpenAI Codex GitHub](https://github.com/openai/codex)
- [Introducing Codex](https://openai.com/index/introducing-codex/)
- [Introducing the Codex App](https://openai.com/index/introducing-the-codex-app/)
- [Unlocking the Codex Harness](https://openai.com/index/unlocking-the-codex-harness/)

---

#### [OpenAI Skills Catalog](https://github.com/openai/skills)
- **URL:** https://github.com/openai/skills
- **Stars:** NOT DISCLOSED | **Forks:** NOT DISCLOSED | **License:** NOT DISCLOSED (public repo)
- **Last updated:** Active
- **What it is:** Skills catalog for Codex. Markdown-based skill definitions with progressive disclosure.
- **Swarm relevance:** **MEDIUM** - Agent capabilities, not swarm coordination
- **Key files for swarm orchestration:**
  - `.system/skill-creator/` - Meta-skill for creating skills
  - `.curated/` - Installable skills via $skill-installer
  - `.experimental/` - Experimental skills
- **Notable:**
  - Context-efficient design (lazy-load markdown)
  - Progressive disclosure (only YAML frontmatter preloaded)
  - Live skill update detection
  - Skills shareable across agents

**Sources:**
- [OpenAI Skills GitHub](https://github.com/openai/skills)

---

#### [OpenAI Codex Action (GitHub Actions)](https://github.com/openai/codex-action)
- **URL:** https://github.com/openai/codex-action
- **Stars:** NOT DISCLOSED | **Forks:** NOT DISCLOSED | **License:** NOT DISCLOSED
- **Last updated:** Active
- **What it is:** GitHub Actions integration for CI/CD workflows. Run Codex in automated pipelines.
- **Swarm relevance:** **LOW** - CI/CD integration, not multi-agent orchestration
- **Key files for swarm orchestration:** None (single-agent in CI)
- **Notable:**
  - Install Codex CLI in CI environment
  - Start Responses API proxy
  - Run `codex exec` with permissions
  - Post reviews to PRs, apply patches
  - Security strategies: drop-sudo (Linux/macOS), unsafe (Windows)

**Sources:**
- [OpenAI Codex Action GitHub](https://github.com/openai/codex-action)
- [Codex GitHub Action Docs](https://developers.openai.com/codex/github-action/)

---

#### [open-codex (ymichael)](https://github.com/ymichael/open-codex)
- **URL:** https://github.com/ymichael/open-codex
- **Stars:** NOT DISCLOSED | **Forks:** NOT DISCLOSED | **License:** NOT DISCLOSED
- **Last updated:** Unknown
- **What it is:** Lightweight coding agent alternative (independent implementation or fork unclear).
- **Swarm relevance:** **LOW** - Single-agent tool
- **Key files for swarm orchestration:** Unknown
- **Notable:**
  - Alternative implementation
  - Relationship to official Codex unclear

**Sources:**
- Mentioned in OpenAI Codex research

---

#### Codex Kaioken (Community Fork)
- **URL:** NOT DISCLOSED (mentioned in HN discussions)
- **Stars:** NOT DISCLOSED | **Forks:** NOT DISCLOSED | **License:** NOT DISCLOSED
- **Last updated:** Unknown (2025/2026)
- **What it is:** Community fork with subagent support. Spawns specialized agents (exploration, execution, research) with real-time streaming in separate panes.
- **Swarm relevance:** **HIGH** - Demonstrates community-driven subagent implementation
- **Key files for swarm orchestration:** NOT DISCLOSED
- **Notable:**
  - Subagents for specialized roles
  - Real-time tool call and diff visualization
  - Shows that advanced multi-agent coordination happens in community forks

**Sources:**
- [Hacker News](https://news.ycombinator.com/item?id=46417772)

---

### 1.4 Kimi K2.5 Project

#### [Kimi K2.5 (MoonshotAI)](https://github.com/MoonshotAI/Kimi-K2.5)
- **URL:** https://github.com/MoonshotAI/Kimi-K2.5
- **Stars:** 774 | **Forks:** 74 | **License:** Modified MIT License
- **Last updated:** Active (January 2026)
- **What it is:** Moonshot's most powerful model. 1.04T parameters (32B active/token), 256K context, native agent swarm with up to 100 sub-agents. Complete open-source release (code + weights).
- **Swarm relevance:** **HIGH** - Native agent swarm at model level (PARL training)
- **Key files for swarm orchestration:**
  - `tech_report.pdf` - Full technical paper (arXiv 2602.02276)
  - `docs/deploy_guidance.md` - Deployment with vLLM/SGLang/KTransformers
  - Model weights on Hugging Face (595GB)
- **Notable:**
  - **Agent Swarm:** Self-directs up to 100 sub-agents, 1,500 tool calls/task
  - **PARL training:** Orchestrator (trainable) + frozen sub-agents
  - **Speedup:** 4.5x reduction in execution time vs single-agent
  - **Benchmarks:** 78.4% BrowseComp (Agent Swarm) vs 60.6% (single-agent)
  - **Proactive context management:** Task sharding across agents
  - **Critical path scheduling:** Rewards effective parallelization
  - **~5 active agents, up to 95 queued** (internal scheduling)
  - **Visual coding:** Screenshot → React component with pixel-level comparison
  - **Modified MIT:** Commercial use free below 100M MAU or $20M monthly revenue

**Architecture:**
- Decentralized parallel intelligence
- Orchestrator dynamically decomposes tasks
- Sub-agents frozen (from intermediate policy checkpoints)
- Heterogeneous instantiation based on task structure
- Wide search (parallel info sources) + Deep search (multiple reasoning branches)
- Dynamic role creation (AI Researcher, Physics Researcher, Fact Checker, etc.)

**PARL Reward System:**
```
r_PARL = λ₁·r_parallel + λ₂·r_finish + r_perf

r_parallel: Mitigates serial collapse (incentivizes subagent instantiation)
r_finish: Prevents spurious parallelism (requires actual task completion)
r_perf: Task-level outcome evaluation
```

**Sources:**
- [Kimi K2.5 GitHub](https://github.com/MoonshotAI/Kimi-K2.5)
- [arXiv:2602.02276 Technical Report](https://arxiv.org/abs/2602.02276)
- [Hugging Face Model](https://huggingface.co/moonshotai/Kimi-K2.5)

---

#### [Kimi K2.5 Prompts & Tools (dnnyngyen)](https://github.com/dnnyngyen/kimi-k2.5-prompts-tools)
- **URL:** https://github.com/dnnyngyen/kimi-k2.5-prompts-tools
- **Stars:** NOT DISCLOSED | **Forks:** NOT DISCLOSED | **License:** Community resource
- **Last updated:** Recent (January 2026)
- **What it is:** Extracted artifacts from Kimi OK-Computer agent. System prompts, skill definitions, tool schemas.
- **Swarm relevance:** **HIGH** - Documents agent swarm implementation details
- **Key files for swarm orchestration:**
  - System prompts for 6 agent types
  - Skill definitions per role
  - Tool schemas for 37 distinct tools
  - Runtime environment source code samples
- **Notable:**
  - Community reverse-engineering of Kimi agent internals
  - Reveals orchestrator prompting strategies
  - Documents tool integration patterns

**Sources:**
- Mentioned in Kimi K2.5 research

---

#### [PARL Implementation (The-Swarm-Corporation)](https://github.com/The-Swarm-Corporation/PARL)
- **URL:** https://github.com/The-Swarm-Corporation/PARL
- **Stars:** NOT DISCLOSED | **Forks:** NOT DISCLOSED | **License:** NOT DISCLOSED
- **Last updated:** Recent
- **What it is:** Community recreation of Parallel-Agent Reinforcement Learning (PARL) training paradigm from Kimi K2.5.
- **Swarm relevance:** **HIGH** - Implements novel swarm training methodology
- **Key files for swarm orchestration:**
  - PARL training implementation
  - Orchestrator-subagent architecture
  - Reward shaping system
- **Notable:**
  - Community implementation (not official Moonshot AI)
  - Shows interest in reproducing PARL methodology
  - Training paradigm for multi-agent task decomposition

**Sources:**
- [The-Swarm-Corporation/PARL GitHub](https://github.com/The-Swarm-Corporation/PARL)

---

## 2. ADDITIONAL DISCOVERED REPOS (Web Search)

### 2.1 Official OpenAI & Anthropic

#### [OpenAI Swarm](https://github.com/openai/swarm)
- **URL:** https://github.com/openai/swarm
- **Stars:** 20,000 | **Forks:** 2,100 | **License:** NOT DISCLOSED (check repo)
- **Last updated:** Note: Now replaced by OpenAI Agents SDK (production-ready evolution)
- **What it is:** Educational framework exploring ergonomic, lightweight multi-agent orchestration. Managed by OpenAI Solution team. Stateless between calls.
- **Swarm relevance:** **HIGH** - Official OpenAI multi-agent framework (educational)
- **Key files for swarm orchestration:**
  - `swarm/core.py` - Core Agent and hand-off logic
  - Agents encapsulate instructions + functions + hand-off capability
  - run() function analogous to chat.completions.create() but handles agent execution/hand-offs
- **Notable:**
  - Educational/experimental (not production)
  - Powered entirely by Chat Completions API
  - Agent hand-offs to other agents
  - Context variable references
  - Multi-turn execution before returning to user
  - **Now superseded by OpenAI Agents SDK**

**Sources:**
- [OpenAI Swarm GitHub](https://github.com/openai/swarm)
- [Medium: Exploring OpenAI Swarm](https://medium.com/@michael_79773/exploring-openais-swarm-an-experimental-framework-for-multi-agent-systems-5ba09964ca18)

---

### 2.2 Enterprise-Grade Frameworks

#### [Swarms (kyegomez)](https://github.com/kyegomez/swarms)
- **URL:** https://github.com/kyegomez/swarms
- **Stars:** 5,600 | **Forks:** 721 | **License:** NOT DISCLOSED
- **Last updated:** Active (January 2026)
- **What it is:** Enterprise-grade, production-ready multi-agent orchestration framework. Aims to accelerate transition to fully autonomous world economy.
- **Swarm relevance:** **HIGH** - Production-grade swarm infrastructure
- **Key files for swarm orchestration:**
  - Multi-agent orchestration core
  - Infrastructure for deploying millions of autonomous agents
  - Seamless deployment and coordination tools
- **Notable:**
  - Surpassed 5,000 GitHub stars milestone (2025)
  - Active development and community
  - Focus on production deployment
  - Enterprise-grade features

**Sources:**
- [Swarms GitHub](https://github.com/kyegomez/swarms)
- [Medium: Swarms Milestone](https://medium.com/@kyeg/major-milestone-swarms-surpasses-5-000-github-stars-6a27b7405c25)

---

#### [Agency Swarm (VRSEN)](https://github.com/VRSEN/agency-swarm)
- **URL:** https://github.com/VRSEN/agency-swarm
- **Stars:** 3,900 | **Forks:** 992 | **License:** MIT
- **Last updated:** Active
- **What it is:** Reliable multi-agent orchestration framework. Extension of OpenAI Agents SDK. v1.0.0 built on OpenAI Agents SDK.
- **Swarm relevance:** **HIGH** - Leverages and extends official SDK
- **Key files for swarm orchestration:**
  - `.cursorrules` - Cursor IDE integration guide
  - Agency creation tools
  - Collaborative agent swarm management
  - Role-specific agent capabilities
- **Notable:**
  - Built on OpenAI Agents SDK
  - Anyone can create collaborative swarms (Agencies)
  - Distinct roles and capabilities per agent
  - Cursor IDE guide for getting started
  - MIT licensed

**Sources:**
- [Agency Swarm GitHub](https://github.com/VRSEN/agency-swarm)
- [Agency Swarm .cursorrules](https://github.com/VRSEN/agency-swarm/blob/main/.cursorrules)

---

#### [Claude Flow (ruvnet)](https://github.com/ruvnet/claude-flow)
- **URL:** https://github.com/ruvnet/claude-flow
- **Stars:** 12,600 | **Forks:** NOT DISCLOSED | **License:** NOT DISCLOSED
- **Last updated:** Active (January 2026, v3 Alpha 79)
- **What it is:** Leading agent orchestration platform for Claude. Deploys intelligent multi-agent swarms, coordinates autonomous workflows. Enterprise-grade architecture, distributed swarm intelligence, RAG integration, native Claude Code support via MCP protocol. Ranked #1 in agent-based frameworks.
- **Swarm relevance:** **HIGH** - Claude-specific swarm orchestration leader
- **Key files for swarm orchestration:**
  - Hive-Mind orchestration system
  - Workflow orchestration engine
  - Stream-JSON chaining for agent-to-agent communication
  - MCP protocol integration
- **Notable:**
  - **10-20x faster batch spawning** (Hive-Mind)
  - **84.8% SWE-Bench solve rate**
  - **32.3% token reduction**
  - Parallel execution, dependency management
  - Intelligent resource allocation
  - Real-time agent-to-agent communication
  - Comprehensive docs (all features documented)

**Sources:**
- [Claude Flow GitHub](https://github.com/ruvnet/claude-flow)
- [Claude Flow Wiki](https://github.com/ruvnet/claude-flow/wiki)

---

#### [MetaGPT (FoundationAgents)](https://github.com/FoundationAgents/MetaGPT)
- **URL:** https://github.com/FoundationAgents/MetaGPT
- **Stars:** 63,600 | **Forks:** 8,000 | **License:** NOT DISCLOSED
- **Last updated:** Active
- **What it is:** The Multi-Agent Framework: First AI Software Company, Towards Natural Language Programming. Takes one-line requirement → outputs user stories, competitive analysis, requirements, data structures, APIs, documents. Includes product managers, architects, project managers, engineers with orchestrated SOPs.
- **Swarm relevance:** **HIGH** - Full software company simulation
- **Key files for swarm orchestration:**
  - Multi-agent role definitions (PM, architect, project manager, engineer)
  - SOP (Standard Operating Procedure) materialization
  - Team composition logic
- **Notable:**
  - **150,000+ GitHub stars** (surpassed in 2025)
  - Philosophy: "Code = SOP(Team)"
  - Materializes SOP and applies to LLM teams
  - Complete software development lifecycle
  - Multi-role collaboration

**Sources:**
- [MetaGPT GitHub](https://github.com/FoundationAgents/MetaGPT)
- [Top 10 AI Agent Frameworks](https://techwithibrahim.medium.com/top-10-most-starred-ai-agent-frameworks-on-github-2026-df6e760a950b)

---

#### [CrewAI](https://github.com/crewAIInc/crewAI)
- **URL:** https://github.com/crewAIInc/crewAI
- **Stars:** 3,900+ (December 2025 data) | **Forks:** 1,500+ | **License:** NOT DISCLOSED
- **Last updated:** Active (2025/2026)
- **What it is:** Framework for orchestrating role-playing, autonomous AI agents. No-code/low-code multi-agent framework with ready-made agent templates. Fosters collaborative intelligence.
- **Swarm relevance:** **HIGH** - Leading multi-agent platform
- **Key files for swarm orchestration:**
  - Role-playing agent definitions
  - Collaborative intelligence framework
  - Ready-made agent templates
- **Notable:**
  - **Rapid growth:** ~140K stars (conflicting data sources)
  - **100,000+ developers certified** through community courses
  - Lean, lightning-fast Python framework
  - Built entirely from scratch (independent of LangChain)
  - High-level simplicity + precise low-level control
  - Standard for enterprise-ready AI automation

**Sources:**
- [CrewAI GitHub](https://github.com/crewAIInc/crewAI)
- [CrewAI Open Source](https://www.crewai.com/open-source)
- [Top CrewAI Projects 2026](https://www.projectpro.io/article/crew-ai-projects-ideas-and-examples/1117)

---

### 2.3 Specialized Tools

#### [Goose (Block/Square)](https://github.com/block/goose)
- **URL:** https://github.com/block/goose
- **Stars:** NOT DISCLOSED | **Forks:** NOT DISCLOSED | **License:** Apache License 2.0
- **Last updated:** Active (January 2026, contributed to AAIF)
- **What it is:** Local, extensible, open source AI agent. Automates engineering tasks, builds entire projects from scratch, writes/executes code, debugs failures, orchestrates workflows, interacts with external APIs autonomously. Built on Model Context Protocol (MCP).
- **Swarm relevance:** **MEDIUM** - Single powerful agent, not multi-agent swarm
- **Key files for swarm orchestration:**
  - `AGENTS.md` - Agent instructions format (contributed to AAIF)
  - MCP-based integration (standardized tool calling)
  - Rust framework with CLI and Electron desktop
- **Notable:**
  - Released by Block (Square, Cash App, Afterpay, TIDAL)
  - **Apache License 2.0** (permissive, commercial use allowed)
  - Built on MCP (developed with Anthropic)
  - Transforms natural language → real-world actions
  - CLI and desktop app (not IDE-limited)
  - **Contributed to Agentic AI Foundation (Linux Foundation)**
  - Rust implementation (performance-focused)

**Sources:**
- [Goose GitHub](https://github.com/block/goose)
- [Goose Official Site](https://block.github.io/goose/)
- [Block Announcement](https://block.xyz/inside/block-open-source-introduces-codename-goose)
- [Linux Foundation AAIF Announcement](https://www.linuxfoundation.org/press/linux-foundation-announces-the-formation-of-the-agentic-ai-foundation)

---

#### [Claude Squad (smtg-ai)](https://github.com/smtg-ai/claude-squad)
- **URL:** https://github.com/smtg-ai/claude-squad
- **Stars:** NOT DISCLOSED | **Forks:** NOT DISCLOSED | **License:** NOT DISCLOSED
- **Last updated:** Active
- **What it is:** Terminal app managing multiple AI agents (Claude Code, Codex, Gemini, Aider) in separate workspaces. Work on multiple tasks simultaneously.
- **Swarm relevance:** **HIGH** - Multi-agent workspace orchestration
- **Key files for swarm orchestration:**
  - Multi-agent workspace manager
  - Separate workspace isolation
  - Task parallelization across agents
- **Notable:**
  - Manages multiple agent types simultaneously
  - Terminal-based coordination
  - Enables parallel workflows

**Sources:**
- [Claude Squad GitHub](https://github.com/smtg-ai/claude-squad)

---

#### [Awesome Claude Agents (vijaythecoder)](https://github.com/vijaythecoder/awesome-claude-agents)
- **URL:** https://github.com/vijaythecoder/awesome-claude-agents
- **Stars:** NOT DISCLOSED | **Forks:** NOT DISCLOSED | **License:** Community resource
- **Last updated:** Active
- **What it is:** Orchestrated sub-agent dev team powered by Claude Code. Supercharges Claude Code with specialized AI agents.
- **Swarm relevance:** **HIGH** - Agent team coordination patterns
- **Key files for swarm orchestration:**
  - Specialized agent definitions
  - Team coordination patterns
  - Technology stack coverage
- **Notable:**
  - Work together to build complete features
  - Debug complex issues
  - Handle any technology stack with expert-level knowledge

**Sources:**
- [Awesome Claude Agents GitHub](https://github.com/vijaythecoder/awesome-claude-agents)

---

#### [Awesome Claude Code Subagents (VoltAgent)](https://github.com/VoltAgent/awesome-claude-code-subagents)
- **URL:** https://github.com/VoltAgent/awesome-claude-code-subagents
- **Stars:** NOT DISCLOSED | **Forks:** NOT DISCLOSED | **License:** Community resource
- **Last updated:** Active
- **What it is:** Collection of 100+ specialized Claude Code subagents covering wide range of development use cases.
- **Swarm relevance:** **MEDIUM** - Subagent templates, not orchestration framework
- **Key files for swarm orchestration:**
  - 100+ subagent definitions
  - Specialized role templates
- **Notable:**
  - Large collection of pre-built agents
  - Community-contributed

**Sources:**
- [Awesome Claude Code Subagents GitHub](https://github.com/VoltAgent/awesome-claude-code-subagents)

---

#### [Multi-Agent Orchestration (wshobson)](https://github.com/wshobson/agents)
- **URL:** https://github.com/wshobson/agents
- **Stars:** NOT DISCLOSED | **Forks:** NOT DISCLOSED | **License:** NOT DISCLOSED
- **Last updated:** Active
- **What it is:** Intelligent automation and multi-agent orchestration for Claude Code. Uses experimental Agent Teams feature.
- **Swarm relevance:** **HIGH** - Practical multi-agent orchestration implementation
- **Key files for swarm orchestration:**
  - 4 specialized agents
  - 7 commands
  - 6 skills
  - Reference documentation
- **Notable:**
  - Parallel workflow orchestration
  - Claude Code Agent Teams integration

**Sources:**
- [wshobson/agents GitHub](https://github.com/wshobson/agents)

---

### 2.4 Broader Ecosystem

#### [Langflow](https://github.com/langflow-ai/langflow) (NOT in search but mentioned)
- **Stars:** ~140,000 (from research context)
- **Swarm relevance:** **MEDIUM** - Visual orchestration, not pure swarm
- **What it is:** Platform for orchestrating multi-agent conversations with no-code/low-code interface.
- **Notable:** Leading platform by stars

---

#### [Spec Kit](https://github.com/projectspec/spec-kit) (NOT in search but mentioned)
- **Stars:** 50,000+ (2025)
- **Swarm relevance:** **MEDIUM** - Specification-driven development
- **What it is:** Addresses AI-assisted coding with structured specs.
- **Notable:** Rapid adoption in 2025

---

#### [Pathway](https://github.com/pathwaycom/pathway) (NOT in search but mentioned)
- **Stars:** 50,000+ (2025)
- **Swarm relevance:** **LOW** - Real-time data processing, not agent swarm
- **What it is:** Real-time data processing framework.
- **Notable:** 50K+ stars accumulated rapidly

---

---

## 3. SWARM RELEVANCE RATINGS SUMMARY

### HIGH RELEVANCE (Swarm Orchestration Code)
1. **FastRender** - Complete swarm implementation output (1.4k stars)
2. **Claude Code** - Agent Teams feature with TeammateTool (65k stars)
3. **Claude Agent SDK (Python/TypeScript)** - Programmatic multi-agent (4.6k / 752 stars)
4. **Kimi K2.5** - Native agent swarm at model level with PARL (774 stars)
5. **OpenAI Swarm** - Educational multi-agent framework (20k stars)
6. **Swarms (kyegomez)** - Enterprise-grade production swarm (5.6k stars)
7. **Agency Swarm (VRSEN)** - Extends OpenAI Agents SDK (3.9k stars)
8. **Claude Flow** - Claude-specific orchestration leader (12.6k stars)
9. **MetaGPT** - Software company simulation (63.6k stars)
10. **CrewAI** - Role-playing autonomous agents (3.9k+ stars)
11. **AGENTS.md Standard** - Agent instruction format standardization
12. **Claude Squad** - Multi-agent workspace manager
13. **Awesome Claude Agents** - Orchestrated sub-agent dev teams
14. **wshobson/agents** - Claude Code multi-agent orchestration
15. **PARL Implementation** - Community PARL training recreation
16. **Kimi K2.5 Prompts & Tools** - Agent swarm implementation docs

### MEDIUM RELEVANCE (Useful Tooling)
1. **Void Editor** - Single-agent Cursor alternative (28.2k stars)
2. **Cursor Memory Bank** - Memory management for agents
3. **OpenAI Skills Catalog** - Agent capabilities definitions
4. **Goose** - Powerful single agent with MCP (Apache 2.0)
5. **Claude Code System Prompts** - Reverse-engineered prompts
6. **Claude Swarm Orchestration (MaTriXy)** - Teammate API docs
7. **Awesome Claude Code** - Community tooling aggregation
8. **Awesome Claude Code Subagents** - 100+ subagent templates
9. **Codex Kaioken** - Community fork with subagents
10. **Langflow** - Visual orchestration platform (~140k stars)
11. **Spec Kit** - Specification-driven development (50k+ stars)

### LOW RELEVANCE (Tangential)
1. **OpenAI Codex Action** - CI/CD integration, not multi-agent
2. **open-codex (ymichael)** - Single-agent alternative
3. **Pathway** - Real-time data processing (50k+ stars)

---

## 4. KEY FINDINGS

### 4.1 Repository Tiers by Stars
- **Mega (50K+):** MetaGPT (63.6k), Claude Code (65k), Langflow (~140k), Spec Kit (50k+), Pathway (50k+)
- **Large (10K-50K):** OpenAI Swarm (20k), Void (28.2k), Claude Flow (12.6k)
- **Medium (1K-10K):** Swarms (5.6k), Claude SDK Python (4.6k), Agency Swarm (3.9k), CrewAI (3.9k), FastRender (1.4k)
- **Small (<1K):** Kimi K2.5 (774 stars - very recent release Jan 2026)

### 4.2 Licensing
- **Open Source (Permissive):** Apache 2.0 (OpenAI Codex, Goose), MIT (Agency Swarm, Claude SDK)
- **Modified Open Source:** Kimi K2.5 (Modified MIT - free below 100M MAU/$20M revenue)
- **Proprietary with OSS Components:** Claude Code, OpenAI Codex (sandboxes open, core proprietary)
- **Not Disclosed:** Many community projects

### 4.3 Active Development (2026)
- All major frameworks actively maintained
- Recent releases: Kimi K2.5 (Jan 2026), Claude Code Agent Teams (Feb 2026), Codex App (Feb 2026), Claude Flow v3 (Jan 2026)
- Community forks emerging (Codex Kaioken)

### 4.4 Architecture Patterns
1. **Hierarchical:** FastRender (Planner → Sub-Planner → Worker → Judge), Claude Code Agent Teams (Lead → Teammates)
2. **Flat/Peer-to-Peer:** Anthropic Compiler (16 equal agents with file locks)
3. **Orchestrator + Workers:** Kimi K2.5 (Trainable orchestrator + frozen sub-agents)
4. **Isolation-First:** OpenAI Codex (Worktrees + coordination via artifacts)
5. **Role-Based:** MetaGPT (PM, architect, engineer), CrewAI (role-playing agents)

### 4.5 Communication Mechanisms
- **Git-based:** FastRender (worktrees + file locks), Anthropic Compiler (optimistic locking)
- **JSON Inbox:** Claude Code Agent Teams (`~/.claude/teams/{name}/inboxes/`)
- **Task DAG:** Claude Code (`blockedBy`/`blocks` arrays)
- **Artifacts:** OpenAI Codex (file-based handoffs), MetaGPT (SOP documents)
- **Implicit (Model-Level):** Kimi K2.5 (orchestrator routes results, no disclosed mailbox)

### 4.6 Missing Implementations
- **FastRender orchestration harness:** NOT open sourced (critical)
- **Anthropic compiler orchestrator:** File locks + README disclosed, but no full harness
- **OpenAI Codex App Server:** Protocol documented, implementation proprietary
- **Kimi K2.5 scheduling:** "~5 active, up to 95 queued" mentioned, mechanism NOT disclosed

### 4.7 Proven Success Cases
1. **FastRender:** 1.6M LOC browser in 1 week, $14M estimated, 99% tests pass
2. **Anthropic Compiler:** 100K LOC C compiler in 2 weeks, $20K cost, 99.1% GCC torture pass
3. **OpenAI Sora Android:** 85% AI-written, 99.9% crash-free, #1 Google Play launch day
4. **Kimi K2.5 Benchmarks:** 78.4% BrowseComp (swarm) vs 60.6% (single), 4.5x speedup

---

## 5. GAPS & OPPORTUNITIES

### NOT FOUND (Despite searching):
1. **Cursor-specific swarm orchestration repo** - Only AGENTS.md from FastRender
2. **"fastrender browser" standalone framework** - It's the output, not a reusable framework
3. **OpenAI Codex orchestration layer source** - App Server protocol yes, implementation no
4. **Kimi inter-agent communication protocol** - PARL training described, mailbox NOT disclosed

### COMMUNITY INTEREST INDICATORS:
- **Replication attempts:** Matt Shumer browser swarm (Claude), Codex Kaioken (Codex fork)
- **Reverse-engineering:** Kieran Klaassen gists (Claude TeammateTool), Piebald AI (system prompts), dnnyngyen (Kimi prompts/tools)
- **Standards efforts:** AGENTS.md, Agentic AI Foundation (AAIF) by Linux Foundation

### EMERGING TRENDS:
1. **Foundation-backed standards:** Linux Foundation AAIF (MCP, Goose, AGENTS.md)
2. **Model-native swarms:** Kimi K2.5 PARL training (swarm at inference, not just orchestration)
3. **MCP adoption:** Goose, Claude Flow, OpenAI Codex all support Model Context Protocol
4. **Enterprise focus:** Swarms (kyegomez), Claude Flow claim "enterprise-grade"

---

## 6. NEXT STEPS FOR DEEPER ANALYSIS

### Part 2 Should Cover:
1. **Deep dive into MetaGPT architecture** (63.6k stars, SOP-based)
2. **Langflow internals** (~140k stars, visual orchestration)
3. **CrewAI implementation details** (3.9k stars, role-playing)
4. **OpenAI Agents SDK** (successor to Swarm)
5. **Additional community repos** from GitHub Topics (agent-swarm, multi-agent-systems)

### Part 3 Should Include:
1. **MCP servers ecosystem** for tool integration
2. **Agentic AI Foundation (AAIF) projects** beyond Goose
3. **Academic implementations** (papers → code)
4. **Regional variations** (Chinese AI swarm frameworks beyond Kimi)

### Recommended Actions:
1. **Clone high-relevance repos** for code analysis (FastRender AGENTS.md, Agency Swarm, Claude Flow)
2. **Test smaller frameworks** (OpenAI Swarm, Agency Swarm) with toy examples
3. **Study PARL training methodology** from Kimi K2.5 paper
4. **Reverse-engineer Claude TeammateTool** from Kieran Klaassen gist
5. **Compare architectural tradeoffs** (hierarchical vs flat vs isolation-first)

---

## SOURCES

### Primary Case Studies
- [Cursor Blog - Self-Driving Codebases](https://cursor.com/blog/self-driving-codebases)
- [Cursor Blog - Scaling Agents](https://cursor.com/blog/scaling-agents)
- [Cursor Blog - Agent Best Practices](https://cursor.com/blog/agent-best-practices)
- [Anthropic - Building a C Compiler](https://www.anthropic.com/engineering/building-c-compiler)
- [Agent Teams Documentation](https://code.claude.com/docs/en/agent-teams)
- [OpenAI - Introducing Codex](https://openai.com/index/introducing-codex/)
- [OpenAI - Introducing the Codex App](https://openai.com/index/introducing-the-codex-app/)
- [OpenAI - Introducing GPT-5.3-Codex](https://openai.com/index/introducing-gpt-5-3-codex/)
- [Kimi K2.5 Technical Report (arXiv:2602.02276)](https://arxiv.org/abs/2602.02276)

### Repository Discovery
- [GitHub - wilsonzlin/fastrender](https://github.com/wilsonzlin/fastrender)
- [GitHub - anthropics/claude-code](https://github.com/anthropics/claude-code)
- [GitHub - openai/codex](https://github.com/openai/codex)
- [GitHub - MoonshotAI/Kimi-K2.5](https://github.com/MoonshotAI/Kimi-K2.5)
- [GitHub - openai/swarm](https://github.com/openai/swarm)
- [GitHub - kyegomez/swarms](https://github.com/kyegomez/swarms)
- [GitHub - VRSEN/agency-swarm](https://github.com/VRSEN/agency-swarm)
- [GitHub - ruvnet/claude-flow](https://github.com/ruvnet/claude-flow)
- [GitHub - FoundationAgents/MetaGPT](https://github.com/FoundationAgents/MetaGPT)
- [GitHub - crewAIInc/crewAI](https://github.com/crewAIInc/crewAI)
- [GitHub - block/goose](https://github.com/block/goose)

### Community Resources
- [Simon Willison - FastRender Interview](https://simonwillison.net/2026/Jan/23/fastrender/)
- [Kieran Klaassen Gist - Claude Swarm Orchestration](https://gist.github.com/kieranklaassen/4f2aba89594a4aea4ad64d753984b2ea)
- [agents.md Standard](https://agents.md/)
- [Awesome Claude Code](https://github.com/hesreallyhim/awesome-claude-code)
- [HackerNews Discussions](https://news.ycombinator.com/)
- [Linux Foundation AAIF Announcement](https://www.linuxfoundation.org/press/linux-foundation-announces-the-formation-of-the-agentic-ai-foundation)

### Analysis Articles
- [Top 10 AI Agent Frameworks 2026](https://techwithibrahim.medium.com/top-10-most-starred-ai-agent-frameworks-on-github-2026-df6e760a950b)
- [Top GitHub Agentic AI Repositories 2025](https://opendatascience.com/the-top-ten-github-agentic-ai-repositories-in-2025/)
- [Medium: Exploring OpenAI Swarm](https://medium.com/@michael_79773/exploring-openais-swarm-an-experimental-framework-for-multi-agent-systems-5ba09964ca18)

---

**End of Part 1**
**Total Repositories Analyzed:** 30+
**High-Relevance Swarm Frameworks:** 16
**Next:** Part 2 will deep-dive MetaGPT, Langflow, CrewAI architecture details + additional GitHub Topics search
