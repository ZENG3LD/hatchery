# Open Source Swarm & Multi-Agent Orchestration Analysis

Comprehensive analysis of open source repositories, frameworks, and tools for multi-agent orchestration and swarm intelligence, extracted from Windsurf, Devin, and elizaOS research files plus additional GitHub exploration.

---

## Table of Contents

1. [Major Multi-Agent Frameworks](#major-multi-agent-frameworks)
2. [Coding Agent Platforms](#coding-agent-platforms)
3. [Orchestration & Coordination Tools](#orchestration--coordination-tools)
4. [Research & Academic Projects](#research--academic-projects)
5. [Community Resources & Tooling](#community-resources--tooling)
6. [Analysis & Recommendations](#analysis--recommendations)

---

## Major Multi-Agent Frameworks

### elizaOS (ai16z)
- **URL**: https://github.com/elizaOS/eliza
- **Stars**: 17,500+ | **Forks**: 5,400+ | **Contributors**: 1,352
- **License**: MIT
- **Last updated**: Active (February 2026)
- **Language**: TypeScript (95.3%)

**What it is**: The Open-Source Framework for Multi-Agent AI Development. An "Agentic Operating System" for building autonomous AI agents that think, learn, and act independently. Originally ai16z, rebranded to elizaOS.

**Swarm relevance**: **HIGH**

**Key files for swarm orchestration**:
- `packages/core/src/` - Core runtime implementation
- `packages/plugin-bootstrap/` - Mandatory message handling plugin (unified event bus)
- `agent/src/index.ts` - Agent initialization and lifecycle
- https://github.com/elizaOS/the-org - Multi-agent organizational system example
- https://github.com/elizaOS/agentmemory - Vector memory (ChromaDB + PostgreSQL)
- https://github.com/elizaos-plugins/registry - Plugin registry (200+ plugins)

**Notable**:
- **Worlds & Rooms architecture**: Each agent keeps its own context yet can signal others for delegation, consensus, load-balancing
- **Self-consistency voting**: Swarms of homogeneous agents vote via majority mechanism for final decisions
- **Agent-to-agent economy**: PayAI integration enables machine-to-machine payments (agents hire other agents)
- **Unified message bus**: "One event pipeline for every interface — Discord, Telegram, X, HTTP or onchain"
- **Real deployments**: Agent Hub marketplace (Ensemble), The Org multi-agent system, Solana DeFi trading agents
- **Peak market cap**: $2B (December 2025) → $28M (February 2026) - demonstrates volatility in AI agent tokens
- **200+ plugins** including blockchain integrations (Solana, Ethereum, Base), AI models (OpenAI, Anthropic, local LLMs), and social platforms

---

### MetaGPT
- **URL**: https://github.com/FoundationAgents/MetaGPT (formerly geekan/MetaGPT)
- **Stars**: 31,400+ | **Forks**: Not specified | **License**: MIT
- **Last updated**: Active
- **Language**: Python

**What it is**: "The Multi-Agent Framework: First AI Software Company, Towards Natural Language Programming." Meta-programming framework incorporating efficient human workflows into LLM-based multi-agent collaborations.

**Swarm relevance**: **HIGH**

**Key files for swarm orchestration**:
- `docs/guide/tutorials/multi_agent_101.md` - Multi-agent tutorial
- SOPs (Standardized Operating Procedures) encoded into prompt sequences
- Assembly line paradigm for role assignment

**Notable**:
- **Assembly line paradigm**: Assigns diverse roles to various agents (Product Manager, Architect, Engineer, QA)
- **SOPs as prompts**: Encodes human workflows into prompt sequences for streamlined collaboration
- **Human-like domain expertise**: Agents verify intermediate results to reduce errors
- **Academic paper**: "MetaGPT: Meta Programming for a Multi-Agent Collaborative Framework" (arXiv:2308.00352v6)
- **Given one line requirement, returns PRD, Design, Tasks, Repo**

---

### CrewAI
- **URL**: https://github.com/crewAIInc/crewAI
- **Stars**: 40,900+ | **Forks**: 5,500+ | **Contributors**: 281+
- **License**: MIT
- **Last updated**: Active (GA 1.0 release)
- **Language**: Python

**What it is**: Framework for orchestrating role-playing, autonomous AI agents. "The Leading Multi-Agent Platform." Built entirely from scratch, independent of LangChain.

**Swarm relevance**: **HIGH**

**Key files for swarm orchestration**:
- Core orchestration engine (lean, lightning-fast Python framework)
- Role-playing agent system
- Task delegation and collaboration primitives

**Notable**:
- **60% of Fortune 500 using it**
- **1.4 billion agentic automations** executed
- **100,000+ certified developers**
- **Production-ready**: Backed by robust documentation and enterprise support
- **Role-playing agents**: Agents take on specialized roles for task execution
- **Collaborative intelligence**: Agents work together seamlessly on complex tasks

---

### AutoGen (Microsoft) → Microsoft Agent Framework
- **URL**: https://github.com/microsoft/autogen
- **Stars**: 50,400+ | **Forks**: Not specified | **Contributors**: 559
- **License**: Not specified
- **Last updated**: **Maintenance mode** (critical bug fixes only)
- **Language**: Python

**What it is**: "A programming framework for agentic AI." Pioneered the multi-agent orchestration paradigm now widely adopted.

**Swarm relevance**: **MEDIUM** (historical relevance, but deprecated)

**Key files for swarm orchestration**:
- Multi-agent conversation framework (0.2 docs)
- Agent chat primitives

**Notable**:
- **Placed in maintenance mode**: No new features, only bug fixes and security patches
- **Microsoft Agent Framework**: Merging AutoGen + Semantic Kernel → GA Q1 2026 target
- **Pioneered multi-agent orchestration**: Influenced many frameworks that followed
- **Deprecated but influential**: Many current frameworks cite AutoGen as inspiration

---

### LangGraph (LangChain)
- **URL**: https://github.com/langchain-ai/langgraph
- **Stars**: 24,400+ | **Forks**: 4,200+
- **License**: MIT
- **Last updated**: Active
- **Language**: Python (JavaScript version: langchain-ai/langgraphjs)

**What it is**: "Build resilient language agents as graphs." Agent orchestration framework for reliable AI agents with graph-based workflows.

**Swarm relevance**: **HIGH**

**Key files for swarm orchestration**:
- `examples/multi_agent/multi-agent-collaboration.ipynb` - Multi-agent collaboration example
- https://github.com/langchain-ai/langgraph-swarm-py - Swarm-style multi-agent systems
- https://github.com/langchain-ai/langgraph-supervisor-py - Hierarchical supervisor pattern

**Notable**:
- **Graph-based coordination**: Each agent is a node, connections are edges, control flow managed by graph state
- **Multiple control flows**: Single-agent, multi-agent, hierarchical, sequential — all in one framework
- **Swarm architecture**: Dynamic handoff between specialized agents
- **Hierarchical/Supervisor pattern**: Central supervisor coordinating specialized agents
- **Trusted by**: Klarna, Replit, Elastic
- **Multi-agent network tutorials** with state management and agent communication

---

### ChatDev
- **URL**: https://github.com/OpenBMB/ChatDev
- **Stars**: 15,000+ (as of 2024)
- **License**: Not specified
- **Last updated**: Active
- **Language**: Python

**What it is**: "ChatDev 2.0: Dev All through LLM-powered Multi-Agent Collaboration." Virtual software company through multi-agent organizational structure.

**Swarm relevance**: **HIGH**

**Key files for swarm orchestration**:
- Chat chain for communication structure
- Communicative dehallucination mechanisms
- Specialized functional seminars (design, code, test, document)

**Notable**:
- **Virtual software company**: CEO, CTO, Programmer, Tester roles in organizational structure
- **Chat-powered development**: Agents guided in what to communicate (chat chain) and how to communicate (dehallucination)
- **Specialized seminars**: Agents collaborate through functional seminars for different tasks
- **Academic paper**: "ChatDev: Communicative Agents for Software Development" (ACL 2024)
- **Autonomous software development** through structured multi-agent collaboration

---

### Swarms (kyegomez)
- **URL**: https://github.com/kyegomez/swarms
- **Stars**: 5,600+
- **License**: Not specified
- **Last updated**: Active
- **Language**: Python

**What it is**: "The Enterprise-Grade Production-Ready Multi-Agent Orchestration Framework." Production-scale multi-agent infrastructure platform.

**Swarm relevance**: **HIGH**

**Key files for swarm orchestration**:
- Hierarchical agent swarms
- Parallel processing pipelines
- Sequential workflow orchestration
- Graph-based agent networks
- Dynamic agent composition
- Agent registry management

**Notable**:
- **Enterprise-grade**: Built for production-scale deployments
- **Mission**: "Build the agentic economy enabling startups, organizations, and institutions to build fully autonomous organizations with multi-agent collaboration"
- **Multiple orchestration patterns**: Hierarchical, parallel, sequential, graph-based
- **Growing community**: 5,000+ stars milestone achieved
- **Website**: https://swarms.ai

---

### Agency Swarm (VRSEN)
- **URL**: https://github.com/VRSEN/agency-swarm
- **Stars**: 3,900+ | **Forks**: 1,000+
- **License**: MIT
- **Last updated**: Active
- **Language**: Python

**What it is**: "Reliable Multi-Agent Orchestration Framework (Extension of Agents SDK)." Built on OpenAI Agents SDK with specialized orchestration features.

**Swarm relevance**: **HIGH**

**Key files for swarm orchestration**:
- `docs/` - Documentation on multi-agent orchestration
- Agency architecture (CEO, Virtual Assistant, Developer roles)
- OpenAI Agents SDK integration layer

**Notable**:
- **Customizable agent roles**: Define distinct roles with tailored instructions, tools, capabilities
- **Built on OpenAI Agents SDK**: Extends official SDK with structured orchestration layer
- **Production-ready**: Designed for reliability and real-world deployment
- **Real-world organizational structures**: Think about automation in terms of companies/teams
- **Arsenii Shatokhin (VRSEN)**: Original vision continues in this fork

---

## Coding Agent Platforms

### OpenHands (formerly OpenDevin)
- **URL**: https://github.com/OpenHands/OpenHands
- **Stars**: 65,000+
- **License**: MIT (except enterprise/ directory)
- **Last updated**: Active
- **Language**: Python

**What it is**: "AI-Driven Development" — open-source platform for AI-powered software development. The leading open-source alternative to Devin AI.

**Swarm relevance**: **MEDIUM** (single-agent architecture, but highly relevant to coding agents)

**Key files for swarm orchestration**:
- Generalist AI agent capabilities
- Code modification, command execution, web research, API integration
- File system management

**Notable**:
- **SWE-bench performance**: Solves >50% of real GitHub issues in benchmarks
- **65,000+ GitHub stars**: Massive community adoption
- **Thriving community**: Collective innovation making autonomous development accessible
- **Powered by any LLM**: Claude, GPT, or custom models
- **Docker support**: Headless mode for scripting, convenient local setup
- **The open-source Devin killer**

---

### SWE-agent (Princeton/Stanford)
- **URL**: https://github.com/SWE-agent/SWE-agent
- **Stars**: Not explicitly stated
- **License**: Not specified
- **Last updated**: Active (NeurIPS 2024)
- **Language**: Python

**What it is**: "SWE-agent takes a GitHub issue and tries to automatically fix it, using your LM of choice." Academic project from Princeton and Stanford.

**Swarm relevance**: **MEDIUM** (single-agent, but benchmark standard)

**Key files for swarm orchestration**:
- Agent-Computer Interface (ACI) for tool usage
- SWE-bench evaluation harness

**Notable**:
- **Mini-SWE-agent**: 100-line agent that scores >74% on SWE-bench verified
- **Academic provenance**: John Yang, Carlos E. Jimenez, Alexander Wettig, Kilian Lieret, Shunyu Yao, Karthik Narasimhan, Ofir Press
- **State-of-the-art**: Top performance on SWE-bench benchmark
- **Vulnerability discovery**: Can also be used for finding vulnerabilities
- **Competitive coding**: Applicable to coding challenges beyond GitHub issues
- **Organization**: https://github.com/SWE-agent

---

### gpt-engineer
- **URL**: https://github.com/AntonOsika/gpt-engineer
- **Stars**: 54,700+
- **License**: Not specified
- **Last updated**: Active (precursor to Lovable.dev)
- **Language**: Python

**What it is**: "CLI platform to experiment with codegen." AI agent that writes entire codebase from a prompt.

**Swarm relevance**: **LOW** (single-agent, but influential)

**Key files for swarm orchestration**:
- Clarifying question system
- Technical spec generation
- Code generation pipeline

**Notable**:
- **Reached 40,000 stars**: Major milestone (now at 54.7k)
- **Clarifying questions**: Asks for clarification before building
- **Learns your style**: Adapts to how you want code to look
- **Vision support**: Can accept image inputs for vision-capable models
- **Community mission**: Maintain tools for coding agent builders, facilitate open-source collaboration
- **Precursor to Lovable.dev**: Commercial evolution of the project

---

### Devika
- **URL**: https://github.com/stitionai/devika
- **Stars**: 2,229
- **License**: MIT
- **Last updated**: Active
- **Language**: Python

**What it is**: "Devika is the first open-source implementation of an Agentic Software Engineer. Initially started as an open-source alternative to Devin."

**Swarm relevance**: **MEDIUM** (single-agent, but agentic architecture)

**Key files for swarm orchestration**:
- Agent planning and reasoning capabilities
- Contextual keyword extraction
- Web browsing and research
- Dynamic agent state tracking

**Notable**:
- **First open-source agentic engineer**: Pioneering implementation
- **SWE-bench goal**: Aims to match or beat Devin's score
- **Multi-LLM support**: Claude 3, GPT-4, Gemini, Mistral, Groq, local LLMs via Ollama
- **Advanced planning**: Breaks down high-level instructions into steps
- **Research capabilities**: Gathers information before coding
- **State visualization**: Dynamic tracking and visualization of agent state

---

### aider
- **URL**: https://github.com/Aider-AI/aider
- **Stars**: 40,100+
- **License**: Not specified
- **Last updated**: Active
- **Language**: Python

**What it is**: "aider is AI pair programming in your terminal." AI coding assistant with write access to your repository.

**Swarm relevance**: **LOW** (exploring multi-agent, not yet implemented)

**Key files for swarm orchestration**:
- Multi-agent flow feature request (Issue #1839)

**Notable**:
- **Write access**: Can modify files, create new files based on conversation
- **Multi-file editing**: Works across multiple files simultaneously
- **Example**: "Refactor these two files to use dependency injection" → both files edited
- **Multi-agent exploration**: GitHub issue discussing multi-agent coding plans
- **Terminal-based**: Lives in your terminal, not a separate IDE
- **40k+ stars**: Strong community adoption

---

### Devon (Entropy Research)
- **URL**: https://github.com/entropy-research/Devon
- **Stars**: ~200
- **License**: Not specified
- **Last updated**: Active
- **Language**: Python

**What it is**: "Devon: An open-source pair programmer." Open-source alternative to Devin.

**Swarm relevance**: **LOW** (single-agent)

**Key files for swarm orchestration**:
- Multi-file editing capabilities
- Git tool integration

**Notable**:
- **SWE agent focus**: Helps with software development and maintenance
- **Reliable multi-file editing**: Core feature
- **Git integration**: Built-in git workflow support
- **Smaller community**: ~200 stars compared to OpenHands (65k)
- **Another Devon exists**: github.com/vunderkind/devon (separate project)

---

### AgentGPT
- **URL**: https://github.com/reworkd/AgentGPT
- **Stars**: 35,700+
- **License**: Not specified
- **Last updated**: Active
- **Language**: TypeScript/JavaScript

**What it is**: "Assemble, configure, and deploy autonomous AI Agents in your browser." Browser-based autonomous agent platform.

**Swarm relevance**: **MEDIUM** (single-agent but autonomous)

**Key files for swarm orchestration**:
- Browser-based agent deployment
- Task agent creation and execution
- Web UI for agent configuration

**Notable**:
- **Browser-deployed**: No complex setup, runs in browser
- **Custom AI naming**: Name your agent and set goals
- **Goal-oriented**: Agent attempts to reach goals by thinking, executing, learning
- **Lightweight**: Best for experimentation
- **35.7k stars**: Top 10 open-source AI agent projects
- **Web UI**: Accessible to non-developers

---

### smol-ai/developer
- **URL**: https://github.com/smol-ai/developer
- **Stars**: 12,200+
- **License**: Not specified
- **Last updated**: Active
- **Language**: Python

**What it is**: "the first library to let you embed a developer agent in your own app!" Junior developer agent for scaffolding codebases.

**Swarm relevance**: **LOW** (single-agent, but embeddable)

**Key files for swarm orchestration**:
- `main.py` - Core agent logic
- Importable library design

**Notable**:
- **Rewritten v2**: Even smaller, importable as library
- **Whole program synthesis**: Transforms product specs into functional codebases
- **Scaffolding focus**: Creates entire project structure from specs
- **Swyx (founder)**: Working on Smol AI Company in SF/Singapore
- **GPT-4 powered**: Uses advanced prompting techniques
- **Embeddable**: Can be integrated into other applications

---

### Claude Code Ecosystem
- **Official**: https://github.com/anthropics/claude-code
- **Superpowers (obra)**: Hit #1 GitHub Trending, 21,815 stars (1,871 in 24 hours)
- **Claude Flow**: https://github.com/ruvnet/claude-flow - Multi-agent orchestration for Claude
- **Your Claude Engineer**: https://github.com/coleam00/your-claude-engineer - Agent harness with Slack/GitHub/Linear
- **Awesome Claude Code Subagents**: https://github.com/VoltAgent/awesome-claude-code-subagents - 100+ specialized subagents

**Swarm relevance**: **HIGH** (Claude Flow specifically)

**Notable**:
- **Superpowers**: Skills library built on Anthropic's Agent Skills specification — structured workflows for design, implementation, TDD, code review
- **Claude Flow**: "The leading agent orchestration platform for Claude" with multi-agent swarms, distributed swarm intelligence, RAG integration
- **MCP Protocol**: Model Context Protocol for tool/data integration
- **Production systems**: Agents as tools/handoffs pattern
- **112 specialized agents** in multi-agent systems (from some Claude Code setups)

---

## Orchestration & Coordination Tools

### OpenAI Swarm → OpenAI Agents SDK
- **Swarm**: https://github.com/openai/swarm (Educational, now superseded)
- **Agents SDK**: https://openai.github.io/openai-agents-python/
- **License**: Not specified
- **Last updated**: **Swarm deprecated** (replaced by Agents SDK)

**What it is**: Educational framework for lightweight multi-agent orchestration → Production-ready Agents SDK with primitives for agent coordination.

**Swarm relevance**: **HIGH** (new SDK actively developed)

**Key files for swarm orchestration**:
- Agent handoffs (agents as tools)
- Guardrails (input/output validation)
- Agents SDK primitives

**Notable**:
- **Swarm deprecated**: OpenAI recommends migrating to Agents SDK for production
- **Production-ready**: Agents SDK actively maintained
- **Lightweight**: Few abstractions, easy to use
- **Agents as tools**: Agents can delegate to other agents
- **Guardrails**: Built-in validation of agent inputs/outputs
- **Cognition's critique**: "Don't Build Multi-Agents" blog post critiques Swarm (but refers to old educational version)

---

### CCPM (Claude Code Project Manager)
- **URL**: https://github.com/automazeio/ccpm
- **Stars**: Not specified
- **License**: Not specified
- **Last updated**: Active

**What it is**: "Project management system for Claude Code using GitHub Issues and Git worktrees for parallel agent execution."

**Swarm relevance**: **HIGH** (parallel agent orchestration)

**Key files for swarm orchestration**:
- `ccpm/agents/` - Agent definitions
- `COMMANDS.md` - Command reference
- `/pm:issue-analyze` - Parallelization analysis
- `/pm:epic-start` - Launch swarm
- `/pm:epic-merge` - Merge results

**Notable**:
- **Parallel development**: Multiple agents work simultaneously with `parallel: true` flag
- **Git worktrees**: Each agent in isolated worktree (conflict-free)
- **GitHub Issues as source of truth**: Comments provide history
- **Real-time collaboration**: Human developers see AI progress through issue comments
- **State management**: Each epic maintains own context in `.claude/context/`
- **By developers who ship**: Built at Automaze

---

### The Org (elizaOS)
- **URL**: https://github.com/elizaOS/the-org
- **Stars**: Not specified
- **License**: MIT
- **Last updated**: Active

**What it is**: "Agents for organizations." Multi-agent system for organizational functions using elizaOS framework.

**Swarm relevance**: **HIGH** (production multi-agent system)

**Key files for swarm orchestration**:
- Specialized agent configurations (community management, dev relations, project coordination, social media)
- Multi-platform integration (Discord, Telegram, Twitter)
- Persistent memory & state (SQL plugin)

**Notable**:
- **Organizational structure**: Specialized agents for different company functions
- **Real deployment**: Production multi-agent system template
- **ElizaOS Worlds/Rooms**: Each agent has own context, can signal others
- **Load testing suite**: Tools to evaluate agent scalability
- **Dynamic onboarding**: Flexible agent setup and customization
- **Template for orgs**: Can be forked for your own organization's AI agents

---

### Windsurf Community Projects
- **windsurfinabox**: https://github.com/pfcoperez/windsurfinabox (Docker headless Cascade agent)
- **windsurf-demo**: https://github.com/Exafunction/windsurf-demo (Official demo app)
- **cascade-memory-bank**: https://github.com/GreatScottyMac/cascade-memory-bank (Memory system)
- **awesome-windsurf**: https://github.com/ichoosetoaccept/awesome-windsurf (Resource hub)

**Swarm relevance**: **MEDIUM** (parallel agents, not true swarm coordination)

**Notable**:
- **Git worktrees**: Up to 5-20 parallel Cascade agents in isolated worktrees
- **No inter-agent communication**: Agents work independently, human merges manually
- **Incident.io case study**: 4-5 parallel agents accelerating feature development
- **SWE-1.5 model**: Free for 3 months, 950 tokens/sec (6x faster than Haiku)
- **Arena Mode**: Compare multiple models on same task (not task distribution)
- **Human orchestration**: Developer is coordinator, not autonomous swarm

---

## Research & Academic Projects

### SWE-bench
- **URL**: https://github.com/SWE-bench/SWE-bench
- **Paper**: https://arxiv.org/html/2501.06781v1
- **Leaderboard**: https://scale.com/leaderboard/swe_bench_pro_public
- **License**: Open source

**What it is**: Benchmark for evaluating AI systems on real-world GitHub issues. Dataset of 2,294 issues from popular Python repositories.

**Swarm relevance**: **LOW** (benchmark, not framework)

**Notable**:
- **Industry standard**: Used by Devin, OpenHands, SWE-agent, and others
- **SWE-bench Pro**: 1,865 tasks across 41 professional repositories
- **Public vs private**: Significant difficulty gap (23.3% vs 14.9-17.8%)
- **Current best**: GPT-5, Claude Opus 4.1 at ~23% resolution
- **Academic paper**: Accepted at major conferences
- **Leaderboards**: Multiple tracking sites (Scale, swe-rebench.com)

---

### Devin's Published Research
- **SWE-bench results**: https://github.com/CognitionAI/devin-swebench-results (MIT License)
- **"Don't Build Multi-Agents"**: https://cognition.ai/blog/dont-build-multi-agents

**What it is**: Cognition's evaluation harness, methodology, and architectural philosophy on multi-agent systems.

**Swarm relevance**: **HIGH** (critical analysis of swarm architectures)

**Notable**:
- **13.86% SWE-bench**: 79 of 570 issues resolved (previous best: 4.8%)
- **Critique of naive multi-agents**: Context isolation causes failures (Flappy Bird example)
- **Two fundamental principles**: (1) Share full agent traces, not just messages (2) Actions carry implicit decisions
- **NO empirical data**: "180 architectures" and "70% degradation" NOT found in published materials
- **Anthropic counter-argument**: Multi-agent beat single-agent by 90.2% (released day after Cognition's post)
- **Devin 2.0 supports multi-agent dispatch**: Despite philosophical opposition to naive implementations

---

### MetaGPT Academic Paper
- **Paper**: https://arxiv.org/html/2308.00352v6
- **Title**: "MetaGPT: Meta Programming for a Multi-Agent Collaborative Framework"

**What it is**: Academic research on meta-programming for multi-agent LLM collaboration.

**Swarm relevance**: **HIGH** (foundational research)

**Notable**:
- **SOPs as prompts**: Standardized Operating Procedures encoded into agent prompts
- **Assembly line paradigm**: Role-based task distribution
- **Human workflow integration**: Incorporates efficient human procedures
- **Verification loops**: Agents verify intermediate results
- **Academic influence**: Cited by many subsequent multi-agent frameworks

---

### ChatDev Academic Paper
- **Paper**: https://arxiv.org/abs/2307.07924
- **ACL**: https://aclanthology.org/2024.acl-long.810/
- **Title**: "ChatDev: Communicative Agents for Software Development"

**What it is**: Research on chat-powered multi-agent software development.

**Swarm relevance**: **HIGH** (organizational structure research)

**Notable**:
- **Chat chain**: Guides what agents communicate
- **Communicative dehallucination**: Guides how agents communicate
- **Functional seminars**: Specialized meetings for design, coding, testing, documentation
- **Organizational metaphor**: CEO, CTO, Programmer, Tester roles
- **ACL 2024**: Accepted at major NLP conference

---

### Eliza Academic Paper
- **Paper**: https://arxiv.org/html/2501.06781v1
- **Title**: "Eliza: A Web3 friendly AI Agent Operating System"

**What it is**: Academic paper on elizaOS architecture and Web3 integration.

**Swarm relevance**: **HIGH** (production system analysis)

**Notable**:
- **GAIA benchmark**: Self-consistency voting with swarms of agents
- **Blockchain integration**: Multi-agent trading, DeFi, token operations
- **Agent-to-agent economy**: Machine-to-machine payments via PayAI
- **Production deployment**: Analysis of real-world multi-agent systems
- **Academic validation**: First major academic paper on Web3 agent OS

---

## Community Resources & Tooling

### Awesome Lists

**e2b-dev/awesome-ai-agents**
- **URL**: https://github.com/e2b-dev/awesome-ai-agents
- **License**: Not specified
- **What it is**: Comprehensive list of AI autonomous agents

**e2b-dev/awesome-ai-sdks**
- **URL**: https://github.com/e2b-dev/awesome-ai-sdks
- **What it is**: SDKs, frameworks, libraries for creating/monitoring/debugging AI agents

**e2b-dev/awesome-devins**
- **URL**: https://github.com/e2b-dev/awesome-devins
- **What it is**: Awesome Devin-inspired AI agents

**kyrolabs/awesome-agents**
- **URL**: https://github.com/kyrolabs/awesome-agents
- **What it is**: Awesome list of AI Agents

**von-development/awesome-LangGraph**
- **URL**: https://github.com/von-development/awesome-LangGraph
- **What it is**: Index of LangChain + LangGraph ecosystem

**The-Swarm-Corporation/Awesome-Swarms-List**
- **URL**: https://github.com/The-Swarm-Corporation/Awesome-Swarms-List
- **What it is**: Applications, tools, resources for Swarms framework

**Swarm relevance**: **MEDIUM** (discovery resources, not orchestration code)

**Notable**: Essential for discovering new agents, frameworks, and tools. Maintained by companies building agent infrastructure (E2B, Swarms, etc.).

---

### Plugin Ecosystems

**elizaos-plugins (organization)**
- **URL**: https://github.com/elizaos-plugins
- **Registry**: https://github.com/elizaos-plugins/registry
- **Count**: 200+ plugins (90+ official)

**Key plugins**:
- `@elizaos/plugin-bootstrap` - Core messaging (MANDATORY)
- `@elizaos/plugin-solana` / `plugin-solana-v2` - Blockchain operations
- `@elizaos/plugin-discord`, `plugin-telegram`, `plugin-twitter` - Social platforms
- `@elizaos/plugin-llama` - Local LLMs
- `@elizaos/plugin-echochambers` - Multi-agent chat rooms

**Swarm relevance**: **HIGH** (extensibility layer for multi-agent systems)

**Notable**: Largest plugin ecosystem for multi-agent framework. Enables cross-platform, cross-blockchain, multi-LLM agent orchestration.

---

### MCP (Model Context Protocol) Servers

**Official**:
- **Protocol**: https://www.anthropic.com/news/model-context-protocol (Anthropic, November 2024)
- **Docs**: https://docs.anthropic.com/en/docs/mcp
- **GitHub**: https://github.com/modelcontextprotocol

**Community Servers**:
- `github/github-mcp-server` - GitHub integration (Docker: ghcr.io/github/github-mcp-server)
- `deephaven/deephaven-mcp` - Database queries
- `cuongdev/mcp-codepipeline-server` - AWS CodePipeline

**Swarm relevance**: **MEDIUM** (shared tools, not agent communication)

**Notable**:
- **Open standard**: De-facto for connecting agents to tools/data
- **SDKs for all languages**: Broad adoption
- **Thousands of servers**: Large community ecosystem
- **Devin integration**: MCP Marketplace (one-click enable)
- **Windsurf integration**: `~/.codeium/windsurf/mcp_config.json`
- **ElizaOS integration**: Plugin-based MCP support
- **Does NOT provide inter-agent messaging**: Only shared external resources

---

### Character & Prompt Tools

**elizaOS/characterfile**
- **URL**: https://github.com/elizaOS/characterfile
- **Schema**: `schema/character.schema.json`
- **What it is**: Simple file format for character data (personality, knowledge, style)

**Tools**:
- `tweets2character` - Generate from Twitter archives
- `folder2knowledge` - Convert documents to knowledge
- `chats2character` - Process WhatsApp conversations
- `web2folder` - Capture web pages

**itsmetamike/eliza-agent-weaver**
- **URL**: https://github.com/itsmetamike/eliza-agent-weaver
- **What it is**: Develop multiple character files, connect narratives of multiple agents

**Swarm relevance**: **MEDIUM** (agent personality/knowledge management)

**Notable**: Critical for creating diverse agent personalities in multi-agent systems. Weaver tool specifically for connecting agent narratives.

---

### Memory & State Management

**elizaOS/agentmemory**
- **URL**: https://github.com/elizaOS/agentmemory
- **What it is**: Easy-to-use agent memory, powered by ChromaDB and Postgres
- **Features**: Document search, knowledge graphing, DBScan clustering

**Glacier VectorDB**
- **Integration**: elizaOS database adapter
- **What it is**: Verifiable vector storage and management

**Swarm relevance**: **MEDIUM** (shared memory for multi-agent systems)

**Notable**: Essential for multi-agent systems to share knowledge. DBScan clustering enables semantic grouping for efficient retrieval.

---

### Trust & Reputation Systems

**elizaOS/trust_scoreboard**
- **URL**: https://github.com/elizaOS/trust_scoreboard
- **What it is**: Trust scoring system for agents
- **Status**: Repository exists, implementation details not fully documented

**Swarm relevance**: **MEDIUM** (agent reputation in marketplaces)

**Notable**: Critical for agent-to-agent economy. Needed for agents to evaluate trustworthiness of other agents they hire.

---

## Analysis & Recommendations

### High Relevance for Swarm Orchestration

#### Tier 1: Production-Ready Swarm Frameworks
1. **elizaOS** (17.5k stars) - Most complete swarm system with agent economy, self-consistency voting, Worlds/Rooms architecture. Real deployments in production.
2. **CrewAI** (40.9k stars) - 60% of Fortune 500 using it. Proven at scale. Role-playing agents with collaborative intelligence.
3. **LangGraph** (24.4k stars) - Graph-based coordination with swarm and supervisor patterns. Trusted by major enterprises.
4. **MetaGPT** (31.4k stars) - Assembly line paradigm with SOPs. Academic credibility + practical use.

#### Tier 2: Emerging Swarm Systems
5. **Agency Swarm** (3.9k stars) - Built on OpenAI Agents SDK. Structured orchestration layer.
6. **Swarms (kyegomez)** (5.6k stars) - Enterprise-grade with multiple orchestration patterns.
7. **CCPM** - Parallel agent orchestration with git worktrees. Practical implementation for coding agents.
8. **ChatDev** (15k stars) - Virtual software company. Specialized functional seminars.

### Medium Relevance: Single-Agent Platforms & Tools

9. **OpenHands** (65k stars) - Largest community, but single-agent. Could be extended to multi-agent.
10. **aider** (40.1k stars) - Exploring multi-agent but not yet implemented.
11. **AgentGPT** (35.7k stars) - Browser-based autonomy, but single-agent focus.
12. **Claude Flow** - Multi-agent swarms for Claude specifically. New but promising.

### Historical/Deprecated But Influential

13. **AutoGen** (50.4k stars) - Maintenance mode. Microsoft Agent Framework is successor (Q1 2026).
14. **OpenAI Swarm** - Deprecated. Replaced by Agents SDK.

### Low Relevance: Research/Benchmark Tools

15. **SWE-agent**, **SWE-bench** - Benchmark standard, not orchestration framework.
16. **gpt-engineer** (54.7k stars) - Influential but single-agent.
17. **Devika**, **Devon**, **smol-ai** - Single-agent alternatives.

---

### Key Architectural Patterns Discovered

#### Pattern 1: Worlds & Rooms (elizaOS)
- Each agent has isolated context
- Agents can "signal" others across rooms
- Enables delegation, consensus, load-balancing
- **Production-proven** in The Org

#### Pattern 2: Swarm Voting (elizaOS)
- Multiple homogeneous agents
- Self-consistency via majority voting
- Used in GAIA benchmark evaluations
- **Academic validation**

#### Pattern 3: Hierarchical Supervisor (LangGraph, Agency Swarm)
- Central supervisor agent
- Specialized worker agents
- Supervisor delegates tasks
- **Enterprise-trusted** (Klarna, Replit)

#### Pattern 4: Graph-Based Coordination (LangGraph)
- Agents as nodes
- Connections as edges
- State managed by graph
- **Flexible control flows**

#### Pattern 5: Assembly Line (MetaGPT)
- Diverse roles (PM, Architect, Engineer, QA)
- SOPs encoded into prompts
- Sequential handoffs
- **Human workflow integration**

#### Pattern 6: Git Worktrees (Windsurf, CCPM)
- Parallel agents in isolated branches
- No inter-agent communication
- Human merges results
- **Conflict-free parallelism** (but NOT true swarm)

#### Pattern 7: Agent Economy (elizaOS)
- Agents hire other agents
- Machine-to-machine payments (PayAI)
- Agent Hub marketplace
- **Economic coordination** layer

---

### What's Missing in Open Source

#### Critical Gaps:
1. **No true swarm consensus algorithms** - Voting exists (elizaOS) but not Byzantine fault tolerance, quorum protocols
2. **No standardized inter-agent message format** - Each framework uses own protocol
3. **No agent discovery protocol** - Agents can't find each other across frameworks
4. **Limited cross-framework interoperability** - elizaOS agents can't talk to CrewAI agents
5. **No distributed state management** - Shared memory within framework only
6. **No agent reputation standard** - Each marketplace builds own trust system

#### Proprietary Advantages (Devin, Windsurf):
1. **Context compression** - Hard to get right (Cognition's admission)
2. **Planning agent systems** - Dual-model architectures (long-term + short-term)
3. **Production scaling** - Enterprise infrastructure (Cognition: $500/mo → $20/mo via optimization)
4. **Quality guardrails** - SOC 2 audit logging, policy enforcement

---

### Recommendations for Hatchery

#### If Building Swarm Orchestration:
1. **Study elizaOS** - Most complete open-source swarm with production deployments
   - Worlds/Rooms architecture
   - Self-consistency voting
   - Agent-to-agent economy
   - 200+ plugin ecosystem

2. **Adopt LangGraph patterns** - Graph-based coordination is flexible and proven
   - Supervisor pattern for hierarchical coordination
   - Swarm pattern for dynamic handoffs
   - State management primitives

3. **Leverage MCP protocol** - Standard for tool/data integration
   - Anthropic-backed open standard
   - Large ecosystem of servers
   - Avoid reinventing tool integration

4. **Design for interoperability** - None of the frameworks talk to each other
   - Standardized message format
   - Agent discovery protocol
   - Cross-framework bridges

#### If Building Coding Agents:
1. **OpenHands** - Largest community, 65k stars, solves >50% GitHub issues
2. **SWE-agent** - Academic credibility, state-of-the-art benchmark performance
3. **CCPM** - Practical parallel agent pattern with git worktrees

#### If Building Enterprise Systems:
1. **CrewAI** - 60% Fortune 500 using it, production-proven
2. **LangGraph** - Trusted by Klarna, Replit, Elastic
3. **MetaGPT** - SOPs for workflow standardization

#### If Exploring Research:
1. **Cognition's critique** - "Don't Build Multi-Agents" raises valid context isolation concerns
2. **Anthropic's counter** - Multi-agent beat single-agent by 90.2%, but requires careful design
3. **When multi-agent works**: Truly independent tasks, specialized domains, research/analysis
4. **When it fails**: Shared implicit decisions, mid-task coordination, conflicting actions

---

### Final Verdict: Top 3 Repos to Study

#### 1. elizaOS/eliza (17.5k stars)
**Why**: Most complete swarm architecture in open source. Production deployments, agent economy, 200+ plugins, academic paper, real multi-agent coordination primitives.

**Study these files**:
- `packages/plugin-bootstrap/` - Unified message bus
- `packages/core/src/` - Worlds/Rooms architecture
- `elizaOS/the-org` - Production multi-agent system
- `elizaOS/agentmemory` - Shared memory with clustering

---

#### 2. langchain-ai/langgraph (24.4k stars)
**Why**: Graph-based coordination is most flexible. Swarm and supervisor patterns both supported. Enterprise-trusted. Clean abstractions.

**Study these files**:
- `examples/multi_agent/multi-agent-collaboration.ipynb` - Collaboration patterns
- `langchain-ai/langgraph-swarm-py` - Swarm architecture
- `langchain-ai/langgraph-supervisor-py` - Hierarchical coordination

---

#### 3. FoundationAgents/MetaGPT (31.4k stars)
**Why**: Assembly line paradigm is closest to human organizational structures. SOPs as prompts is elegant. Academic validation.

**Study these files**:
- `docs/guide/tutorials/multi_agent_101.md` - Multi-agent tutorial
- SOPs implementation - Prompt engineering for roles
- Assembly line coordination - Role-based task distribution

---

## Sources

### Main Research Sources
- [Windsurf Wave 13 Research Files](local)
- [Devin 2.0 Research Files](local)
- [elizaOS Research Files](local)

### GitHub Repositories
- [elizaOS/eliza](https://github.com/elizaOS/eliza)
- [langchain-ai/langgraph](https://github.com/langchain-ai/langgraph)
- [FoundationAgents/MetaGPT](https://github.com/FoundationAgents/MetaGPT)
- [crewAIInc/crewAI](https://github.com/crewAIInc/crewAI)
- [microsoft/autogen](https://github.com/microsoft/autogen)
- [OpenHands/OpenHands](https://github.com/OpenHands/OpenHands)
- [SWE-agent/SWE-agent](https://github.com/SWE-agent/SWE-agent)
- [OpenBMB/ChatDev](https://github.com/OpenBMB/ChatDev)
- [kyegomez/swarms](https://github.com/kyegomez/swarms)
- [VRSEN/agency-swarm](https://github.com/VRSEN/agency-swarm)
- [AntonOsika/gpt-engineer](https://github.com/AntonOsika/gpt-engineer)
- [stitionai/devika](https://github.com/stitionai/devika)
- [Aider-AI/aider](https://github.com/Aider-AI/aider)
- [reworkd/AgentGPT](https://github.com/reworkd/AgentGPT)
- [openai/swarm](https://github.com/openai/swarm)
- [smol-ai/developer](https://github.com/smol-ai/developer)
- [automazeio/ccpm](https://github.com/automazeio/ccpm)
- [elizaOS/the-org](https://github.com/elizaOS/the-org)
- [e2b-dev/awesome-ai-agents](https://github.com/e2b-dev/awesome-ai-agents)

### Web Resources
- [APIdog - OpenHands Overview](https://apidog.com/blog/openhands-the-open-source-devin-ai-alternative/)
- [KDnuggets - OpenHands](https://www.kdnuggets.com/openhands-open-source-ai-software-developer)
- [Modal - Open AI Agents](https://modal.com/blog/open-ai-agents)
- [LangGraph Official](https://www.langchain.com/langgraph)
- [CrewAI Official](https://www.crewai.com/)
- [Swarms.ai](https://www.swarms.ai/)
- [OpenAI Agents SDK](https://openai.github.io/openai-agents-python/)
- [Model Context Protocol - Anthropic](https://www.anthropic.com/news/model-context-protocol)
- [Cognition AI - Don't Build Multi-Agents](https://cognition.ai/blog/dont-build-multi-agents)
- [arXiv - MetaGPT Paper](https://arxiv.org/html/2308.00352v6)
- [arXiv - ChatDev Paper](https://arxiv.org/abs/2307.07924)
- [arXiv - Eliza Paper](https://arxiv.org/html/2501.06781v1)

---

**Report Generated**: 2026-02-08
**Research Scope**: Open source swarm orchestration, multi-agent frameworks, coding agents
**Files Analyzed**: Windsurf Wave 13, Devin 2.0, elizaOS research + 40+ GitHub repos
**Total Stars Analyzed**: 500,000+ across all projects
