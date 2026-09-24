# Open Source Multi-Agent Swarm Orchestration: Comprehensive Analysis Part 2

**Date**: 2026-02-08
**Researcher**: research-agent
**Scope**: GitHub repos, frameworks, and tools mentioned in swarm research case studies + additional multi-agent frameworks

---

## Executive Summary

This report analyzes **26 open source repositories and frameworks** related to multi-agent swarm orchestration, extracted from case study research files and additional web searches. Projects are categorized by swarm relevance (HIGH/MEDIUM/LOW) and include detailed metrics on stars, activity, and key orchestration features.

**Key Findings**:
- **LangGraph** and **CrewAI** dominate multi-agent orchestration (40k+ stars each)
- **Agent Skills specification** (anthropics/agentskills) is becoming industry standard (adopted by Microsoft, OpenAI, Atlassian, Figma, Cursor, GitHub)
- **Microsoft Skills** repository provides production-ready SDK patterns for 130+ Azure services
- Claude Code swarm ecosystem emerging with 6+ specialized orchestration projects
- Replit Agent 3 uses LangGraph but remains proprietary

---

## Category 1: MCP Servers (Model Context Protocol)

### Serena MCP (oraios/serena)
- **URL**: https://github.com/oraios/serena
- **Stars**: Not disclosed | **Forks**: Not disclosed | **License**: Free & open source
- **Last updated**: Active (2026)
- **What it is**: Powerful coding agent toolkit providing semantic retrieval and editing capabilities via language servers + MCP integration
- **Swarm relevance**: **MEDIUM**
- **Key files for swarm orchestration**:
  - MCP server implementation
  - Language server protocol integration
  - Semantic code navigation tools
- **Notable**: IDE-like tools for LLMs - enables agents to extract code entities at symbol level, navigate relational structure without reading entire files. Mentioned by Zach Wills as critical for 20-agent swarm.

**Sources**:
- [GitHub - oraios/serena](https://github.com/oraios/serena)
- [Serena Documentation](https://oraios.github.io/serena/01-about/000_intro.html)
- [MCP Registry - Serena](https://github.com/mcp/oraios/serena)

---

### Playwright MCP (microsoft/playwright-mcp)
- **URL**: https://github.com/microsoft/playwright-mcp
- **Stars**: Not disclosed | **Forks**: Not disclosed | **License**: Not specified
- **Last updated**: Active (2026)
- **What it is**: MCP server providing browser automation capabilities using Playwright accessibility tree (not screenshots)
- **Swarm relevance**: **HIGH**
- **Key files for swarm orchestration**:
  - MCP server implementation for browser control
  - Accessibility snapshot-based interaction
  - Test execution and exploration APIs
- **Notable**: Built into GitHub Copilot Coding Agent. Used by Zach Wills' swarm for autonomous test loops. Replit Agent 3 uses similar approach (3x faster, 10x cheaper than Computer Use models). Enables agents to validate UI changes autonomously.

**Sources**:
- [GitHub - microsoft/playwright-mcp](https://github.com/microsoft/playwright-mcp)
- [Playwright MCP Guide](https://executeautomation.github.io/mcp-playwright/docs/intro)
- [MCP Registry - Playwright](https://github.com/mcp/microsoft/playwright-mcp)

---

### Sequential Thinking MCP (modelcontextprotocol/servers)
- **URL**: https://github.com/modelcontextprotocol/servers/tree/main/src/sequentialthinking
- **Stars**: Repository-wide (multiple servers) | **Forks**: Not specified | **License**: Open source
- **Last updated**: Active (2026)
- **What it is**: MCP server enforcing dynamic and reflective problem-solving through structured thought sequences
- **Swarm relevance**: **MEDIUM**
- **Key files for swarm orchestration**:
  - `src/sequentialthinking/index.ts` - Main implementation
  - `README.md` - Installation and usage
- **Notable**: Forces AI to outline plan before executing, dramatically reducing "intent drift". Mentioned by Zach Wills as critical for keeping 20 agents on track. Allows thoughts to build on, question, or revise previous insights as understanding deepens.

**Community variants**:
- Python implementation: https://github.com/XD3an/python-sequential-thinking-mcp
- With tool suggestions: https://github.com/spences10/mcp-sequentialthinking-tools

**Sources**:
- [Sequential Thinking MCP Server](https://github.com/modelcontextprotocol/servers/tree/main/src/sequentialthinking)
- [MCP Servers Repository](https://github.com/modelcontextprotocol/servers)

---

## Category 2: Multi-Agent Orchestration Frameworks

### LangGraph (langchain-ai/langgraph)
- **URL**: https://github.com/langchain-ai/langgraph
- **Stars**: 40,000+ | **Forks**: Not specified | **License**: MIT
- **Last updated**: Active (2026)
- **What it is**: Low-level framework for building, managing, and deploying long-running, stateful multi-agent systems as graphs
- **Swarm relevance**: **HIGH**
- **Key files for swarm orchestration**:
  - Graph-based workflow definitions (nodes = agents, edges = data flow)
  - State management and persistence
  - Durable execution infrastructure
  - Human-in-the-loop capabilities
  - Memory (short-term and long-term)
- **Notable**: **Used by Replit Agent 3** for multi-agent architecture. Fastest framework with lowest latency. Supports single agent, multi-agent, hierarchical, and sequential control flows. Inspired by Pregel (Google's graph processing), Apache Beam, NetworkX. Available in Python (PyPI: `langgraph`) and JavaScript (`langgraphjs`).

**Related**:
- **LangGraph Swarm**: https://github.com/langchain-ai/langgraph-swarm-py - Out-of-box streaming, memory, human-in-the-loop

**Sources**:
- [GitHub - langchain-ai/langgraph](https://github.com/langchain-ai/langgraph)
- [LangGraph Overview](https://www.langchain.com/langgraph)
- [Multi-Agent Tutorial 2026](https://langchain-tutorials.github.io/langgraph-multi-agent-systems-2026/)

---

### Deep Agents (langchain-ai/deepagents)
- **URL**: https://github.com/langchain-ai/deepagents
- **Stars**: Not specified | **Forks**: Not specified | **License**: Open source
- **Last updated**: Active (2026)
- **What it is**: Agent harness built on LangChain/LangGraph with planning tools, filesystem backend, and **subagent spawning** capability
- **Swarm relevance**: **HIGH**
- **Key files for swarm orchestration**:
  - Task tool for spawning specialized subagents
  - Context isolation mechanism (keeps main agent clean while going deep on subtasks)
  - Filesystem backend for shared state
  - Planning tools
  - Examples: `examples/ralph_mode`, `examples/content-builder-agent`
- **Notable**: Referenced by Replit case study as similar pattern. Supports Claude, OpenAI, Google, or any LangChain-compatible model. Built on LangGraph with production features (streaming, persistence, checkpointing). Install: `pip install deepagents` or `uv add deepagents`. JavaScript/TypeScript version: https://github.com/langchain-ai/deepagentsjs

**Sources**:
- [GitHub - langchain-ai/deepagents](https://github.com/langchain-ai/deepagents)
- [Deep Agents Documentation](https://docs.langchain.com/oss/python/deepagents/overview)
- [Deep Agents Quickstarts](https://github.com/langchain-ai/deepagents-quickstarts)

---

### CrewAI (crewAIInc/crewAI)
- **URL**: https://github.com/crewAIInc/crewAI
- **Stars**: 43,600+ (as of 2026) | **Forks**: Not specified | **License**: Open source
- **Last updated**: Active (2026)
- **What it is**: Framework for orchestrating role-playing, autonomous AI agents fostering collaborative intelligence
- **Swarm relevance**: **HIGH**
- **Key files for swarm orchestration**:
  - Role-based agent architecture
  - Task delegation system
  - Inter-agent communication
  - Central state management for data flow
  - Examples: https://github.com/crewAIInc/crewAI-examples
- **Notable**: **Fundamentally designed for multi-agent collaboration**. Lean, lightning-fast Python framework built from scratch (completely independent of LangChain). Task delegation, inter-agent communication, and state management handled centrally at framework level. Visual workflow interface for rapid prototyping.

**Sources**:
- [GitHub - crewAIInc/crewAI](https://github.com/crewAIInc/crewAI)
- [CrewAI Open Source](https://www.crewai.com/open-source)
- [CrewAI 34k-Star Framework](https://www.glbgpt.com/resource/crewai-34k-star-open-source-framework-for-multi-agent-ai)

---

### AutoGen / Microsoft Agent Framework (microsoft/autogen → microsoft/agent-framework)
- **URL**:
  - https://github.com/microsoft/autogen (legacy, maintained)
  - https://github.com/microsoft/agent-framework (new unified framework)
- **Stars**: AutoGen 40,000+ | Agent Framework: New (2026) | **License**: Open source
- **Last updated**:
  - AutoGen: Stable API, critical bug fixes only
  - Agent Framework: Active development (2026)
- **What it is**: Framework for building, orchestrating, and deploying AI agents and multi-agent workflows (Python and .NET)
- **Swarm relevance**: **HIGH**
- **Key files for swarm orchestration**:
  - Multi-agent conversation framework
  - Agent collaboration and reflection
  - Cross-language support (.NET and Python)
  - Distributed runtime for flexibility
- **Notable**: **AutoGen and Semantic Kernel merging into Microsoft Agent Framework** (2026). AutoGen still maintained (stable API + security patches) but significant new features go to Agent Framework. Popular in research for experimentation with agent-to-agent loops. Event-driven agents, message passing, local and distributed runtime.

**Also available**: AG2 (community fork): https://github.com/ag2ai/ag2

**Sources**:
- [GitHub - microsoft/autogen](https://github.com/microsoft/autogen)
- [GitHub - microsoft/agent-framework](https://github.com/microsoft/agent-framework)
- [Agent Framework Overview](https://learn.microsoft.com/en-us/agent-framework/overview/agent-framework-overview)
- [AutoGen Research](https://www.microsoft.com/en-us/research/project/autogen/)

---

## Category 3: Agent Skills & Configuration Standards

### Agent Skills Specification (agentskills/agentskills)
- **URL**: https://github.com/agentskills/agentskills
- **Stars**: Not specified | **Forks**: Not specified | **License**: Apache 2.0 (code), CC-BY-4.0 (docs)
- **Last updated**: Active (published Dec 18, 2025)
- **What it is**: **Open standard** for modular agent capabilities with progressive disclosure loading
- **Swarm relevance**: **HIGH**
- **Key files for swarm orchestration**:
  - `docs/specification.mdx` - Full SKILL.md format specification
  - Validation tool: `skills-ref` reference library
  - Progressive disclosure mechanism (3-level loading: metadata → instructions → resources)
  - Directory structure standards
- **Notable**: **Industry-wide adoption**: Microsoft, OpenAI, Atlassian, Figma, Cursor, GitHub. Published at https://agentskills.io/specification. Enables agents to install many skills, load only what's relevant per task. Recommended: <500 lines / <5000 tokens per SKILL.md.

**SKILL.md Format**:
```yaml
---
name: skill-name              # Required, 1-64 chars, lowercase+hyphens
description: string           # Required, 1-1024 chars, what+when
license: Apache-2.0           # Optional
compatibility: string         # Optional
metadata:                     # Optional key-value map
  author: org-name
  version: "1.0"
allowed-tools: string         # Experimental
---

# Skill Instructions (Markdown)
```

**Sources**:
- [GitHub - agentskills/agentskills](https://github.com/agentskills/agentskills)
- [Agent Skills Specification](https://agentskills.io/specification)
- [Simon Willison on Agent Skills](https://simonwillison.net/2025/Dec/19/agent-skills/)

---

### Anthropic Skills (anthropics/skills)
- **URL**: https://github.com/anthropics/skills
- **Stars**: 65,400+ | **Forks**: 6,500+ | **License**: Apache 2.0 (examples), Proprietary (document skills)
- **Last updated**: Active (2026, 20 commits on main)
- **What it is**: Reference implementations for Agent Skills - creative, technical, and **document processing suite**
- **Swarm relevance**: **MEDIUM**
- **Key files for swarm orchestration**:
  - `skills/docx/` - Word document creation/editing (source-available)
  - `skills/pdf/` - PDF manipulation (source-available)
  - `skills/pptx/` - PowerPoint creation (source-available)
  - `skills/xlsx/` - Excel spreadsheet creation (source-available)
  - `template/` - Skill template starter
  - `.claude-plugin/marketplace.json` - Plugin configuration
- **Notable**: **Document skills are source-available (NOT open source)**, shared as reference for production AI. Creative & development skills are Apache 2.0. Languages: Python (91.3%), HTML (4.5%), Shell (2.5%), JavaScript (1.7%). Install via: `/plugin marketplace add anthropics/skills`

**Disclaimer**: "Provided for demonstration and educational purposes only. Always test thoroughly before critical tasks."

**Sources**:
- [GitHub - anthropics/skills](https://github.com/anthropics/skills)
- [SKILL.md Examples](https://github.com/anthropics/skills/blob/main/skills/docx/SKILL.md)

---

### Microsoft Skills (microsoft/skills)
- **URL**: https://github.com/microsoft/skills
- **Stars**: Not specified | **Forks**: Not specified | **License**: Apache 2.0
- **Last updated**: Active (2026)
- **What it is**: **130+ skills** for Azure SDK development with MCP servers, custom agents, agents.md templates
- **Swarm relevance**: **HIGH**
- **Key files for swarm orchestration**:
  - `.github/skills/` - 132 skills in flat structure
  - Skills organized by language: Python (41), .NET (28), TypeScript (24), Java (25), Rust (7), Core (5)
  - MCP server configurations
  - Agent persona definitions
  - Multi-agent symlink patterns
- **Notable**: **Production SDK grounding for coding agents**. Categories: Foundry & AI (23), Data & Storage (16), Messaging & Events (10), Entra & Security (11), Monitoring. Emphasizes "context rot prevention" - load only essential skills. Supports GitHub Copilot, Claude Code, OpenCode. Language detection via suffix (`-py`, `-dotnet`, `-ts`, `-java`, `-rust`). MCP-builder skill: generates MCP servers. Browse all: https://microsoft.github.io/skills/

**Sources**:
- [GitHub - microsoft/skills](https://github.com/microsoft/skills)
- [Agent Skills Documentation](https://microsoft.github.io/skills/)
- [Context-Driven Development](https://devblogs.microsoft.com/all-things-azure/context-driven-development-agent-skills-for-microsoft-foundry-and-azure/)

---

## Category 4: GitHub Copilot Ecosystem

### GitHub Copilot SDK (github/copilot-sdk)
- **URL**: https://github.com/github/copilot-sdk
- **Stars**: Not specified | **Forks**: Not specified | **License**: MIT
- **Last updated**: Technical Preview (Jan 14, 2026)
- **What it is**: Multi-platform SDK for integrating GitHub Copilot Agent into any application (Python, TypeScript, Go, .NET)
- **Swarm relevance**: **HIGH**
- **Key files for swarm orchestration**:
  - `nodejs/` - TypeScript/JavaScript implementation
  - `python/` - Python SDK
  - `go/` - Go implementation
  - `dotnet/` - .NET/C# implementation
  - `docs/getting-started.md` - Comprehensive documentation
  - JSON-RPC protocol over stdio/TCP
- **Notable**: **Exposes same production-tested agent runtime as Copilot CLI**. Automatic CLI process lifecycle management. Supports custom agents, skills, tools. BYOK (Bring Your Own Key): OpenAI, Azure AI Foundry, Anthropic. Default: all first-party tools enabled. Per-prompt billing counted toward premium quota (unless BYOK). Community SDKs: Java, Rust, C++, Clojure.

**Architecture**: `Application → SDK Client → JSON-RPC → Copilot CLI (server mode) → LLM Backend`

**Sources**:
- [GitHub - github/copilot-sdk](https://github.com/github/copilot-sdk)
- [Build an agent with Copilot SDK](https://github.blog/news-insights/company-news/build-an-agent-into-any-app-with-the-github-copilot-sdk/)
- [Copilot SDK + Agent Framework](https://devblogs.microsoft.com/semantic-kernel/build-ai-agents-with-github-copilot-sdk-and-microsoft-agent-framework/)

---

### GitHub Awesome Copilot (github/awesome-copilot)
- **URL**: https://github.com/github/awesome-copilot
- **Stars**: Not specified | **Forks**: Not specified | **License**: Open source
- **Last updated**: Active (2026)
- **What it is**: Community-contributed collection of custom agents, prompts, instructions, skills for GitHub Copilot
- **Swarm relevance**: **MEDIUM**
- **Key files for swarm orchestration**:
  - `agents/` - Custom agent definitions (*.agent.md)
  - `prompts/` - Task-specific prompts (*.prompt.md)
  - `instructions/` - Coding standards (*.instructions.md)
  - `skills/` - Self-contained SKILL.md folders
  - `AGENTS.md` - Main documentation
- **Notable**: **2,500+ repositories analyzed** to extract best practices. File naming: lowercase-with-hyphens. YAML frontmatter required. Automated README generation via `npm run build`. Validation: `npm run collection:validate` and `npm run skill:validate`. Examples: LaunchDarkly MCP agent, ADR (Architectural Decision Records) agent, Application Security remediation agent.

**File Format Example**:
```yaml
---
description: 'Required, single-quoted'
tools: ['read', 'edit', 'search']
model: 'gpt-4.1'  # Strongly recommended
---

Agent instructions here (max 30,000 chars)
```

**Sources**:
- [GitHub - github/awesome-copilot](https://github.com/github/awesome-copilot)
- [AGENTS.md Documentation](https://github.com/github/awesome-copilot/blob/main/AGENTS.md)
- [How to write great agents.md](https://github.blog/ai-and-ml/github-copilot/how-to-write-a-great-agents-md-lessons-from-over-2500-repositories/)

---

### Copilot Orchestra (ShepAlderson/copilot-orchestra)
- **URL**: https://github.com/ShepAlderson/copilot-orchestra
- **Stars**: Not specified | **Forks**: Not specified | **License**: Not specified
- **Last updated**: Active (2026)
- **What it is**: Multi-agent orchestration system for structured, test-driven software development with specialized subagents
- **Swarm relevance**: **HIGH**
- **Key files for swarm orchestration**:
  - Conductor agent (central orchestrator)
  - Planning subagent (gather context, draft plan)
  - Implementation subagent (code generation)
  - Code Review subagent (validation)
  - Complete development cycle: planning → implementation → review → commit
- **Notable**: **Workflow**: Conductor delegates research → drafts multi-phase plan (3-10 phases) → stops for user approval → implements. Each subagent optimized for specific role. Test-driven approach.

**Sources**:
- [GitHub - ShepAlderson/copilot-orchestra](https://github.com/ShepAlderson/copilot-orchestra)

---

### Agent Delegation Grid (adamerso/adg-parallels)
- **URL**: https://github.com/adamerso/adg-parallels
- **Stars**: Not specified | **Forks**: Not specified | **License**: Not specified
- **Last updated**: Active (2026)
- **What it is**: AI Delegation Grid - scalable multi-agent orchestration for Copilot and LLMs inside VS Code
- **Swarm relevance**: **HIGH**
- **Key files for swarm orchestration**:
  - Parallel task execution system
  - Adapter patterns for LLM integration
  - Automation workflows
- **Notable**: Designed for VS Code environment. Enables decomposing complex tasks across specialized agents rather than single monolithic AI.

**Sources**:
- [GitHub - adamerso/adg-parallels](https://github.com/adamerso/adg-parallels)

---

## Category 5: Claude Code Swarm Orchestration

### Claude-Flow (ruvnet/claude-flow)
- **URL**: https://github.com/ruvnet/claude-flow
- **Stars**: Not specified | **Forks**: Not specified | **License**: Not specified
- **Last updated**: Active (2026)
- **What it is**: **Leading agent orchestration platform for Claude** - deploy intelligent multi-agent swarms with enterprise-grade architecture
- **Swarm relevance**: **HIGH**
- **Key files for swarm orchestration**:
  - 60+ specialized agents in coordinated swarms
  - Self-learning capabilities
  - Fault-tolerant consensus
  - Enterprise-grade security
  - Distributed swarm intelligence
  - RAG integration
  - Native Claude Code support via MCP protocol
  - `CLAUDE.md` - Configuration and best practices
- **Notable**: **Ranked #1 in agent-based frameworks** (per repo description). Conversational AI systems, autonomous workflows. Native MCP support.

**Sources**:
- [GitHub - ruvnet/claude-flow](https://github.com/ruvnet/claude-flow)
- [CLAUDE.md](https://github.com/ruvnet/claude-flow/blob/main/CLAUDE.md)

---

### ccswarm (nwiizo/ccswarm)
- **URL**: https://github.com/nwiizo/ccswarm
- **Stars**: Not specified | **Forks**: Not specified | **License**: Not specified
- **Last updated**: Active (2026)
- **What it is**: Multi-agent orchestration system using Claude Code with **Git worktree isolation** and specialized AI agents
- **Swarm relevance**: **HIGH**
- **Key files for swarm orchestration**:
  - Rust-native patterns with zero-cost abstractions
  - Channel-based communication for efficient task delegation
  - Git worktree isolation per agent (prevents conflicts)
  - ACP (Agent Client Protocol) as default communication method
- **Notable**: **High-performance Rust implementation**. Uses Git worktree for true isolation (similar to Zach Wills' 4+ terminals + Neon database branching pattern). Each agent works in separate worktree.

**Sources**:
- [GitHub - nwiizo/ccswarm](https://github.com/nwiizo/ccswarm)

---

### oh-my-claudecode (Yeachan-Heo/oh-my-claudecode)
- **URL**: https://github.com/Yeachan-Heo/oh-my-claudecode
- **Stars**: Not specified | **Forks**: Not specified | **License**: Not specified
- **Last updated**: Active (2026)
- **What it is**: Multi-agent orchestration for Claude Code with **5 execution modes**
- **Swarm relevance**: **HIGH**
- **Key files for swarm orchestration**:
  - **Autopilot** - Autonomous mode
  - **Ultrapilot** - 3-5x parallel execution
  - **Swarm** - Coordinated agents
  - **Pipeline** - Sequential chains
  - **Ecomode** - Token-efficient execution
  - 31+ skills
  - 32 specialized agents
- **Notable**: **Zero learning curve** (per repo description). Multiple execution modes allow flexibility: parallel for speed, sequential for dependencies, eco for cost optimization. Comprehensive skill library.

**Sources**:
- [GitHub - Yeachan-Heo/oh-my-claudecode](https://github.com/Yeachan-Heo/oh-my-claudecode)

---

### wshobson/agents
- **URL**: https://github.com/wshobson/agents
- **Stars**: Not specified | **Forks**: Not specified | **License**: Not specified
- **Last updated**: Active (2026)
- **What it is**: Comprehensive production-ready system for intelligent automation and multi-agent orchestration for Claude Code
- **Swarm relevance**: **HIGH**
- **Key files for swarm orchestration**:
  - 112 specialized AI agents
  - 16 multi-agent workflow orchestrators
  - 146 agent skills
  - 79 development tools
  - Organized into 73 focused plugins
- **Notable**: **Massive scale**: 112 agents + 146 skills. Production-ready (not experimental). Organized as plugins for modularity.

**Sources**:
- [GitHub - wshobson/agents](https://github.com/wshobson/agents)

---

### Claude Code Swarm Orchestration Skill (Gist by kieranklaassen)
- **URL**: https://gist.github.com/kieranklaassen/4f2aba89594a4aea4ad64d753984b2ea
- **Stars**: N/A (Gist) | **Forks**: N/A | **License**: Not specified
- **Last updated**: 2026
- **What it is**: Complete guide to multi-agent coordination using Claude Code's **TeammateTool and Task system**
- **Swarm relevance**: **HIGH**
- **Key content**:
  - Master multi-agent orchestration patterns
  - TeammateTool usage (built-in Claude Code feature)
  - Task system for delegation
  - All coordination patterns documented
- **Notable**: **Educational resource** for Claude Code's native multi-agent capabilities. Shows built-in swarm features (not external framework).

**Related Gist**: https://gist.github.com/kieranklaassen/d2b35569be2c7f1412c64861a219d51f (Multi-Agent Orchestration System)

**Sources**:
- [Claude Code Swarm Orchestration Skill](https://gist.github.com/kieranklaassen/4f2aba89594a4aea4ad64d753984b2ea)
- [Hacker News Discussion](https://news.ycombinator.com/item?id=46743908)

---

### parruda/swarm
- **URL**: https://github.com/parruda/swarm
- **Stars**: Not specified | **Forks**: Not specified | **License**: Not specified
- **Last updated**: Active (2026)
- **What it is**: Ruby gems for general-purpose AI agent systems with SwarmSDK providing single-process orchestration
- **Swarm relevance**: **HIGH**
- **Key files for swarm orchestration**:
  - SwarmSDK - Single-process orchestration
  - SwarmMemory - Persistent memory with semantic search
  - SwarmCLI - Command-line interface
  - Node workflows
  - Hooks system
  - `CLAUDE.md` - Configuration
- **Notable**: **Ruby implementation** (rare in agent ecosystem). Persistent memory with semantic search (differentiator). Use cases: automation, research, data processing, customer support, content creation. Claude Swarm v1 for dev teams.

**Sources**:
- [GitHub - parruda/swarm](https://github.com/parruda/swarm)
- [CLAUDE.md](https://github.com/parruda/swarm/blob/main/CLAUDE.md)

---

## Category 6: Community Collections & Tools

### Awesome Agent Skills (skillmatic-ai/awesome-agent-skills)
- **URL**: https://github.com/skillmatic-ai/awesome-agent-skills
- **Stars**: Not specified | **Forks**: Not specified | **License**: Not specified
- **Last updated**: Active (2026)
- **What it is**: Definitive resource for Agent Skills - curated list revolutionizing AI agent architecture
- **Swarm relevance**: **MEDIUM**
- **Key content**:
  - Modular capabilities catalog
  - Multi-agent architecture patterns
  - Production agent systems
- **Notable**: Community-curated. Good starting point for discovering skills across ecosystems.

**Alternative**: https://github.com/heilcheng/awesome-agent-skills

**Sources**:
- [GitHub - skillmatic-ai/awesome-agent-skills](https://github.com/skillmatic-ai/awesome-agent-skills)

---

### Agent Skills for Context Engineering (muratcankoylan/Agent-Skills-for-Context-Engineering)
- **URL**: https://github.com/muratcankoylan/Agent-Skills-for-Context-Engineering
- **Stars**: Not specified | **Forks**: Not specified | **License**: Not specified
- **Last updated**: Active (2026)
- **What it is**: Comprehensive collection of Agent Skills for **context engineering** and multi-agent architectures
- **Swarm relevance**: **MEDIUM**
- **Key content**:
  - Context management patterns (critical for long-running swarms)
  - Multi-agent architecture patterns
  - Production agent systems design
- **Notable**: Focus on **context engineering** - how to manage agent memory/context in multi-agent scenarios (e.g., preventing context pollution, which Replit Agent 3 solved by isolating verifier subagent).

**Sources**:
- [GitHub - muratcankoylan/Agent-Skills-for-Context-Engineering](https://github.com/muratcankoylan/Agent-Skills-for-Context-Engineering)

---

### Code-and-Sorts/awesome-copilot-agents
- **URL**: https://github.com/Code-and-Sorts/awesome-copilot-agents
- **Stars**: Not specified | **Forks**: Not specified | **License**: Not specified
- **Last updated**: Active (2026)
- **What it is**: Curated list of GitHub Copilot instructions, prompts, skills, and agent markdown files
- **Swarm relevance**: **LOW**
- **Key content**:
  - GitHub Copilot-specific configurations
  - Prompt collections
  - Agent markdown examples
- **Notable**: Overlaps with github/awesome-copilot but community-maintained alternative. Good for prompt engineering patterns.

**Sources**:
- [GitHub - Code-and-Sorts/awesome-copilot-agents](https://github.com/Code-and-Sorts/awesome-copilot-agents)

---

## Category 7: Proprietary/Commercial (Not Open Source)

### Replit Agent 3
- **URL**: https://replit.com/agent3
- **Stars**: N/A (commercial product) | **License**: Proprietary
- **Last updated**: Active (2026)
- **What it is**: Multi-agent coding assistant with manager, editor, and verifier agents
- **Swarm relevance**: **HIGH** (but NOT open source)
- **Open source components used**:
  - **LangGraph** (MIT license) - Core orchestration
  - **LangSmith** (commercial) - Observability
- **Architecture**:
  - Manager agent - Orchestration
  - Editor agents - Code modifications (minimal scope principle)
  - Verifier agent - Browser automation testing (Playwright-based)
- **Notable**: **200+ minute autonomy**, 10x more autonomous than Agent 2. Self-healing reflection loop. REPL-based verification (3x faster, 10x cheaper than Computer Use). Case study: Rokt built **135 apps in 24 hours** with 700+ employees (including non-technical). Uses **decision-time guidance** (dynamic prompts) vs static prompts. **Production database deletion incident** (July 2025) led to improved safety: environment segregation, approval gates, read-only by default, just-in-time access.

**Cost concerns**: Users report $1k/week for editing pre-existing apps (higher than expected).

**NOT AVAILABLE**: Replit Agent source code, exact prompts, Python DSL schema, REPL verification implementation.

**Sources**:
- [Replit Agent 3](https://replit.com/agent3)
- [LangChain Case Study](https://www.langchain.com/breakoutagents/replit)
- [ZenML Production Guide](https://www.zenml.io/llmops-database/building-a-production-ready-multi-agent-coding-assistant)
- [Replit Blog](https://blog.replit.com/introducing-agent-3-our-most-autonomous-agent-yet)

---

### GitHub Agent HQ / Mission Control
- **URL**: https://github.com/features/copilot/agents
- **Stars**: N/A (commercial product) | **License**: Proprietary
- **Last updated**: Active (public preview 2026)
- **What it is**: Unified orchestration platform for multi-vendor AI coding agents (Copilot, Claude, Codex, Jules, etc.)
- **Swarm relevance**: **HIGH** (but NOT open source)
- **Open source components**:
  - **Agent Skills specification** (Apache 2.0) - Industry standard
  - **GitHub Copilot SDK** (MIT) - Agent runtime
  - **MCP servers** - Various licenses
- **Built-in specialized agents**:
  - **Explore** - Codebase analysis (isolated context)
  - **Task** - Command execution
  - **Plan** - Implementation planning
  - **Code-review** - Quality validation
- **Notable**: **Parallel subagent execution** (Jan 2026 update): "90 seconds sequential → 30 seconds parallel". Multi-vendor: Claude, Codex, Jules run on same platform. Unified governance (branch controls, identity management, audit logging). Session logs showing "internal monologue". Real-time steering (pause/refine/restart mid-run). Auto-compaction at 95% token capacity. Progressive disclosure for skills (3-level loading).

**NOT AVAILABLE**: Agent HQ platform source, Mission Control UI, Copilot CLI binary, agent runtime core.

**Sources**:
- [Agent HQ Announcement](https://github.blog/news-insights/company-news/welcome-home-agents/)
- [Mission Control Guide](https://github.blog/ai-and-ml/github-copilot/how-to-orchestrate-agents-using-mission-control/)
- [Claude + Codex Integration](https://github.blog/news-insights/company-news/pick-your-agent-use-claude-and-codex-on-agent-hq/)

---

## Additional Frameworks (Not Mentioned in Case Studies)

### Cline (AI Code Generation)
- **URL**: Not specified in search results
- **Stars**: Not specified | **License**: Open source
- **What it is**: Open-source AI coding agent for real-world development
- **Swarm relevance**: **MEDIUM**
- **Key features**:
  - Plan Mode
  - MCP integration
  - Terminal-first workflows
  - Transparent, auditable, end-to-end automation
- **Notable**: "Widely regarded as one of the best open-source AI coding agents" for real-world development (per 2026 analysis).

**Sources**:
- [Best Open Source AI Coding Agents 2026](https://www.theunwindai.com/p/best-open-source-ai-coding-agents-what-teams-can-actually-ship-with-in-2026)

---

### Langflow (Low-Code Multi-Agent)
- **URL**: Not specified in search results
- **Stars**: Not specified | **License**: Open source
- **What it is**: Low-code framework for building AI agents and workflows with visual interface
- **Swarm relevance**: **LOW**
- **Key features**:
  - User-friendly visual interface
  - Low-code development
  - Non-technical user support
- **Notable**: Good for rapid prototyping but less suitable for complex swarm orchestration (visual interface becomes unwieldy at scale).

**Sources**:
- [Best Multi-Agent Frameworks](https://getstream.io/blog/multiagent-ai-frameworks/)

---

## Analysis: Swarm Orchestration Patterns

### HIGH Relevance Projects (Swarm Orchestration Core)

**Frameworks**:
1. **LangGraph** - Production-proven (Replit uses it), fastest, MIT license
2. **CrewAI** - Designed for multi-agent from ground up, 43k+ stars
3. **AutoGen/Agent Framework** - Microsoft-backed, research + enterprise
4. **Deep Agents** - Subagent spawning, context isolation built-in

**Claude Code Swarms**:
1. **claude-flow** - 60+ agents, enterprise-grade
2. **ccswarm** - Rust performance, git worktree isolation
3. **oh-my-claudecode** - 5 execution modes including Swarm + Ultrapilot
4. **wshobson/agents** - Massive scale (112 agents, 146 skills)

**GitHub Copilot Orchestration**:
1. **Copilot SDK** - Official runtime (MIT), multi-language
2. **Copilot Orchestra** - Full development cycle automation
3. **Agent Delegation Grid** - Scalable parallel execution

**Standards**:
1. **Agent Skills** - Industry standard (Microsoft, OpenAI, Atlassian, Figma, Cursor, GitHub adopted)
2. **Microsoft Skills** - Production SDK patterns for 130+ services

**MCP Servers (Critical for Swarm Capabilities)**:
1. **Playwright MCP** - Autonomous testing (used by Zach Wills, Replit Agent 3)
2. **Sequential Thinking MCP** - Prevents drift (Zach Wills: "dramatically reduced drift")

---

### MEDIUM Relevance Projects (Supporting Tools)

1. **Serena MCP** - IDE-like semantic navigation (Zach Wills used for 20-agent swarm)
2. **Anthropic Skills** - Reference implementations (document processing)
3. **Awesome Agent Skills** - Curated collections
4. **Agent Skills for Context Engineering** - Context management patterns (critical for swarms)
5. **awesome-copilot** - Community patterns and examples

---

### LOW Relevance Projects (Tangential)

1. **Code-and-Sorts/awesome-copilot-agents** - Prompt collections (not orchestration)
2. **Langflow** - Low-code visual (doesn't scale for complex swarms)

---

## Key Insights for Hatchery Swarm Implementation

### 1. LangGraph is De Facto Standard
- Used by Replit Agent 3 (production-proven at enterprise scale)
- MIT license (free for commercial use)
- Fastest performance, lowest latency
- Python + JavaScript versions
- Durable execution, human-in-the-loop built-in

**Recommendation**: **Base Hatchery on LangGraph** for swarm orchestration.

---

### 2. Agent Skills Specification is Industry Standard
- Adopted by: Microsoft, OpenAI, Atlassian, Figma, Cursor, GitHub
- Apache 2.0 / CC-BY-4.0 licenses
- Progressive disclosure (3-level loading: metadata → instructions → resources)
- Validation tools available (`skills-ref`)

**Recommendation**: **Adopt Agent Skills format** for Hatchery agent definitions.

---

### 3. Context Management is Critical
- Replit Agent 3: Isolated verifier context prevents pollution
- Zach Wills: Fresh context spawning after milestones (Rule #7: "Be Ruthless About Restarting")
- GitHub Copilot: Auto-compaction at 95% token capacity
- Microsoft Skills: "Context rot" warning - load only essential skills

**Recommendation**: **Implement context isolation per agent** + periodic resets.

---

### 4. Autonomous Testing is Swarm Enabler
- Playwright MCP: Browser automation (Zach Wills, Replit Agent 3)
- Replit: 3x faster, 10x cheaper than Computer Use
- Reflection loop: test → fix → retest (enables 200+ min autonomy)

**Recommendation**: **Integrate Playwright MCP** for autonomous validation loops.

---

### 5. Minimal Scope Principle Reduces Errors
- Replit Agent 3: "Each agent limited to smallest possible task"
- Result: ~90% success rate for tool calls
- Rationale: "More you expose, more opportunities for incorrect choices"

**Recommendation**: **Constrain each Hatchery agent** to minimal necessary tools/context.

---

### 6. Safety Requires Multiple Layers
- Replit's database deletion incident (July 2025) lessons:
  1. Environment segregation (dev/prod isolation)
  2. Approval gates for high-impact actions
  3. Read-only by default
  4. Just-in-time access (not persistent privileges)
  5. Hybrid security (LLMs + deterministic static analysis)

**Recommendation**: **Implement approval gates** for destructive operations (main branch, database, file deletion).

---

### 7. Claude Code Swarm Ecosystem is Emerging
- 6+ specialized repos (claude-flow, ccswarm, oh-my-claudecode, wshobson/agents, parruda/swarm)
- Native TeammateTool and Task system (built-in swarm features)
- Git worktree isolation pattern (ccswarm)
- Multiple execution modes (Autopilot, Ultrapilot, Swarm, Pipeline, Ecomode)

**Recommendation**: **Study claude-flow and oh-my-claudecode** for Claude-specific patterns.

---

### 8. Microsoft Agent Framework is Future of AutoGen
- AutoGen + Semantic Kernel → Microsoft Agent Framework (2026)
- AutoGen stable API maintained (bug fixes only)
- New features go to Agent Framework
- Python + .NET support

**Recommendation**: **Track Agent Framework** (not AutoGen) for Microsoft ecosystem integration.

---

### 9. GitHub Copilot SDK Enables Custom Swarms
- MIT license, multi-language (Python, TS, Go, .NET)
- Production-tested runtime (same as Copilot CLI)
- BYOK support (OpenAI, Azure, Anthropic)
- JSON-RPC over stdio/TCP

**Recommendation**: **Consider Copilot SDK** for multi-language Hatchery client support.

---

### 10. Parallel Execution Requires Task Partitioning
- GitHub Agent HQ: "90 seconds sequential → 30 seconds parallel"
- Optimal for: Research, documentation, security review, separate modules
- Keep sequential: Dependencies, assumption validation, same-file changes (merge conflicts)

**Recommendation**: **Implement dependency graph** for task scheduling (DAG-based).

---

## Comparison Matrix: Top 5 Swarm Frameworks

| Feature | LangGraph | CrewAI | AutoGen/Agent Framework | Deep Agents | Copilot SDK |
|---------|-----------|--------|-------------------------|-------------|-------------|
| **License** | MIT | Open source | Open source | Open source | MIT |
| **Stars** | 40k+ | 43.6k+ | 40k+ (AutoGen) | Not specified | Not specified |
| **Primary Language** | Python/JS | Python | Python/.NET | Python/JS | Python/TS/Go/.NET |
| **Swarm Focus** | Graph-based multi-agent | Role-based teams | Agent-to-agent loops | Subagent spawning | Programmable runtime |
| **Production Use** | ✅ Replit Agent 3 | ✅ Various | ✅ Microsoft products | ⚠️ Reference impl | ✅ GitHub Copilot |
| **Speed** | Fastest (lowest latency) | Fast | Moderate | Not specified | Production-tested |
| **Human-in-Loop** | ✅ Built-in | ✅ Built-in | ✅ Built-in | ✅ Built-in | ✅ Built-in |
| **Persistence** | ✅ Durable execution | ✅ Central state | ✅ Durable | ✅ Checkpointing | ✅ Session logs |
| **Context Isolation** | ✅ Graph nodes | ⚠️ Central | ⚠️ Shared | ✅ Subagent isolation | ✅ Agent-specific |
| **Dependencies** | Standalone | Standalone | LangChain (optional) | LangChain/LangGraph | Copilot CLI (managed) |
| **Best For** | Performance-critical | Role-based teams | Research + enterprise | Context isolation | GitHub ecosystem |

**Winner for Hatchery**: **LangGraph** (production-proven, fastest, MIT license, used by Replit)

---

## Recommended Stack for Hatchery Swarm

Based on analysis of 26 repositories:

### Core Orchestration
- **LangGraph** (langchain-ai/langgraph) - MIT license, production-proven

### Agent Configuration
- **Agent Skills specification** (agentskills/agentskills) - Industry standard
- **Microsoft Skills** (microsoft/skills) - Reference for SDK patterns

### Critical MCP Servers
- **Playwright MCP** (microsoft/playwright-mcp) - Autonomous testing
- **Sequential Thinking MCP** (modelcontextprotocol/servers) - Prevent drift
- **Serena MCP** (oraios/serena) - Semantic codebase navigation

### Observability
- **LangSmith** (commercial, optional) - Used by Replit for trace management
- Session logs + checkpointing (LangGraph built-in)

### Safety Patterns
- Environment segregation (dev/prod isolation)
- Approval gates for high-impact actions
- Read-only by default
- Minimal scope principle (constrain each agent)

### Inspiration Sources
- **claude-flow** - 60+ agent swarm patterns
- **oh-my-claudecode** - Execution mode variety (Autopilot/Ultrapilot/Swarm/Pipeline/Ecomode)
- **ccswarm** - Git worktree isolation (Rust performance)
- **Copilot Orchestra** - Full development cycle automation

---

## Files Created

- `c:\Users\VA PC\CODING\ML_TRADING\nemo\hatchery\research\major-swarm-research\cases\00-opensource-analysis-part2.md`

---

## Summary

Analyzed **26 open source repositories and frameworks** for multi-agent swarm orchestration:

**HIGH Relevance (16)**:
- LangGraph, CrewAI, AutoGen/Agent Framework, Deep Agents
- Agent Skills, Microsoft Skills
- Playwright MCP, Sequential Thinking MCP
- GitHub Copilot SDK, Copilot Orchestra, Agent Delegation Grid
- claude-flow, ccswarm, oh-my-claudecode, wshobson/agents, parruda/swarm

**MEDIUM Relevance (8)**:
- Serena MCP
- Anthropic Skills
- Awesome Agent Skills collections (3 repos)
- Agent Skills for Context Engineering
- awesome-copilot

**LOW Relevance (2)**:
- Code-and-Sorts/awesome-copilot-agents
- Langflow

**Key Finding**: **LangGraph** (MIT, 40k+ stars) is production-proven (Replit Agent 3), fastest, and best foundation for Hatchery swarm. **Agent Skills specification** is industry standard adopted by Microsoft, OpenAI, Atlassian, Figma, Cursor, GitHub.

**Critical Dependencies**: Playwright MCP (autonomous testing), Sequential Thinking MCP (drift prevention), context isolation patterns.

---

## Sources

All sources are embedded as markdown hyperlinks throughout the report above. Key repositories analyzed:

**Frameworks**:
- [LangGraph](https://github.com/langchain-ai/langgraph)
- [CrewAI](https://github.com/crewAIInc/crewAI)
- [AutoGen](https://github.com/microsoft/autogen) / [Agent Framework](https://github.com/microsoft/agent-framework)
- [Deep Agents](https://github.com/langchain-ai/deepagents)

**Standards**:
- [Agent Skills](https://github.com/agentskills/agentskills)
- [Anthropic Skills](https://github.com/anthropics/skills)
- [Microsoft Skills](https://github.com/microsoft/skills)

**MCP Servers**:
- [Serena](https://github.com/oraios/serena)
- [Playwright MCP](https://github.com/microsoft/playwright-mcp)
- [Sequential Thinking MCP](https://github.com/modelcontextprotocol/servers/tree/main/src/sequentialthinking)

**GitHub Copilot**:
- [Copilot SDK](https://github.com/github/copilot-sdk)
- [Awesome Copilot](https://github.com/github/awesome-copilot)
- [Copilot Orchestra](https://github.com/ShepAlderson/copilot-orchestra)
- [Agent Delegation Grid](https://github.com/adamerso/adg-parallels)

**Claude Code Swarms**:
- [claude-flow](https://github.com/ruvnet/claude-flow)
- [ccswarm](https://github.com/nwiizo/ccswarm)
- [oh-my-claudecode](https://github.com/Yeachan-Heo/oh-my-claudecode)
- [wshobson/agents](https://github.com/wshobson/agents)
- [parruda/swarm](https://github.com/parruda/swarm)

**Case Studies**:
- [Replit Agent Case Study](https://www.langchain.com/breakoutagents/replit)
- [GitHub Agent HQ](https://github.blog/news-insights/company-news/welcome-home-agents/)

---

**Report Complete** | 26 repositories analyzed | Swarm relevance rated | Orchestration patterns extracted
