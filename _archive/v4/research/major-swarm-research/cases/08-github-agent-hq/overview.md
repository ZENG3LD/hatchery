# GitHub Agent HQ / Mission Control: Platform Overview

## 1. Overview & Scale

### Platform Summary

**GitHub Agent HQ** is a unified orchestration platform for AI coding agents launched at GitHub Universe 2025 (October 28, 2025) and expanded with multi-vendor support on February 4, 2026. It provides "governed, platform-level orchestration for specialized AI agents" across the full software development lifecycle.

**Mission Control** is the command center interface within Agent HQ, providing "a consistent interface across GitHub, VS Code, mobile, and CLI that lets you direct, monitor, and manage every AI-driven task."

### Multi-Agent Support

Agent HQ supports multiple AI vendors simultaneously:

- **GitHub Copilot** (first-party agent)
- **Anthropic Claude** / Claude Code (added Feb 2026)
- **OpenAI Codex** (added Feb 2026)
- **Google Labs Jules** (integration announced)
- **Cognition** (partnership announced)
- **xAI** (partnership announced)

All agents are accessible through existing paid Copilot subscriptions (Pro+ and Enterprise plans). As of Feb 2026, the platform is in **public preview**.

### Specialized Agent Types

GitHub Copilot CLI includes **four built-in specialized agents** that can run in parallel:

| Agent | Purpose | Description |
|-------|---------|-------------|
| **Explore** | Codebase analysis | "Fast codebase analysis. Ask questions about your code without cluttering your main context." |
| **Task** | Command execution | "Runs commands like tests and builds. Brief summaries on success, full output on failure." |
| **Plan** | Implementation planning | "Generates implementation strategies by examining code dependencies and structure." |
| **Code-review** | Quality validation | "Analyzes changes with focus on only surfacing genuine issues," minimizing noise. |

**Key Quote**: "Copilot can now run multiple agents simultaneously rather than sequentially. What this means in practice: when developers invoke a complex task like debugging authentication failures, they no longer wait for one agent to finish exploring code patterns before another tests credentials, then a third reviews security implications. All three execute concurrently, transforming what might take 90 seconds of sequential agent handoffs into 30 seconds of parallel analysis."

### Scale & Adoption

According to internal GitHub usage data:
- "About 400 employees using it across 300+ repositories and nearly 1,000 merged pull requests" (as of discussions in late 2025)
- Analyzed "2,500+ repositories" to develop best practices for agent configuration files

## 2. Architecture

### Multi-Vendor Agent Coexistence

Agent HQ's core architectural innovation is enabling multiple AI models to work within GitHub's existing security perimeter:

**Quote**: "Agents from multiple vendors can now operate within GitHub's security perimeter, using the same identity controls, branch permissions and audit logging that enterprises already trust for human developers. Agent HQ compartmentalizes access at the branch level and wraps all agent activity in enterprise-grade governance controls."

**Key Design Principle**: "Rather than forcing developers into disconnected tools, it makes agents 'native to the GitHub flow' by integrating them into existing workflows built on Git, pull requests, and issues."

### Agent Execution Model

**Claude Integration**: "Claude can pick up issues, create branches, commit code, and respond to pull requests, working alongside your team like any other collaborator."

**Jules Integration**: Integrates as "a native assignee, streamlining manual steps"

**Backward Compatibility**: "The platform maintains backward compatibility—developers continue using familiar Git primitives, existing compute infrastructure (GitHub Actions or self-hosted runners), and their preferred development environments."

### Platform Access Points

Agents can be invoked from multiple surfaces:

- **GitHub.com** - Web interface with dedicated agents tab
- **GitHub Mobile** - iOS/Android apps
- **Visual Studio Code** - IDE integration
- **JetBrains IDEs** - IntelliJ, PyCharm, etc. (public preview)
- **Eclipse** - (public preview)
- **Xcode** - (public preview)
- **GitHub CLI** - Terminal interface
- **Slack, Linear, Jira, Teams, Azure Boards, Raycast** - Third-party integrations

### Subagent Execution

**Parallel Subagent System**: "Subagents can now run simultaneously when tasks are independent. Previously, multiple `runSubagent` calls executed sequentially; this update 'dramatically reduces wait times for research and code review operations.'"

**Automatic Delegation**: "The system allows Copilot to delegate to these agents automatically and can execute multiple agents simultaneously."

**Custom Agent Integration**: Developers can create custom agents with specialized capabilities using agent profile files.

## 3. Communication

### Cross-Agent Coordination

**Task Assignment**: "Mission Control brings orchestration into one unified interface, letting you run multiple Copilot agents across repositories, monitor their progress, steer them mid-run, and review their work."

**Multi-Agent Comparison**: "Agent HQ also lets you compare how different agents approach the same problem. You can assign multiple agents to a task, and see how Copilot, Claude, and Codex reason about tradeoffs and arrive at different solutions."

**Quote**: "Running agents in parallel, you get competing approaches and edge cases before code hardens."

### Session Logs

**Real-Time Monitoring**: "You can follow agent progress in real time or review completed sessions later, with detailed logs showing what the agent did and why."

**Detailed Access**: "Once the agent starts working, you can click any agent session to open the session log and follow its progress and thought process in real time."

**Transparency**: Users can "dive into Copilot's session logs in GitHub or Visual Studio Code to understand how it approached your task." These logs reveal "Copilot's internal monologue and the tools it used to understand your repository, make changes and validate its work."

**CLI Streaming**: Users can employ the `--follow` option to "stream live logs as the agent works"

### Real-Time Steering

**Mid-Run Intervention**: "Assign tasks to Copilot across repos, pick a custom agent, watch real-time session logs, steer mid-run (pause, refine, or restart), and jump straight into the resulting pull requests—all in one place."

**Active Oversight**: "Users can actively steer sessions by providing additional prompts while work is ongoing, or terminate sessions entirely using the 'Stop session' button in the log viewer."

**Drift Detection Signals** (from Mission Control guide):
- Failing tests, integrations, or dependency fetches
- Unexpected file creation outside scope
- Scope expansion beyond original requirements
- Intent misinterpretation evidenced in logs
- Circular/repetitive failed approaches

**Steering Methodology**: "Intervention requires specificity: explaining *why* redirection is necessary and *how* to proceed. Early intervention (within minutes) prevents hours of ineffective work."

### Session Persistence

Sessions are accessible across platforms:
- Dedicated agents tab at github.com/copilot/agents
- GitHub CLI commands (`gh agent-task list` and `gh agent-task view`)
- Native IDE integrations
- Raycast extension (macOS)
- GitHub Mobile application

**Metrics Tracked**: "Token usage, session count, and session length"

## 4. Git & Code Integration

### Pull Request Workflows

**Direct Integration**: "Direct integration with pull request workflows" enables agents to "jump straight into the resulting pull requests"

**Standard Review Process**: "Agent-generated changes follow standard review processes—developers review agent work the same way they would review teammate contributions. Changes remain attached to repository pull requests rather than existing as standalone outputs."

### Branch Management

**Branch Controls**: "New capabilities include branch controls for agent-created code, identity management for access control, and one-click merge conflict resolution."

**Protection Rules**: "Branch protection rules apply specifically to agent-created branches. The default policy: agents can only push to branches they created. They can't touch your main branch or team branches."

### Repository Context

**Native Git Integration**: The platform leverages existing Git infrastructure rather than replacing it. Agents work with:
- Issues and issue assignment
- Branch creation and management
- Commits with full attribution
- Pull request creation and comments
- Code review workflows

**Isolated Development Environment**: "The agent operates with its own development environment where it can run automated tests and linters, to validate its changes before it pushes."

### Agent Identity

**Identity Controls**: "Using the same identity controls, branch permissions and audit logging that enterprises already trust for human developers."

**Attribution**: Agents commit code with clear attribution showing which AI system created the changes.

## 5. What Worked & What Failed

### What Worked

**Multi-Vendor Strategy Benefits**:

1. **Choice and Competition**: "Pick your agent: Use Claude and Codex on Agent HQ" - giving developers options rather than vendor lock-in
2. **Specialized Strengths**: Different agents excel at different tasks; parallel execution enables "competing approaches and edge cases"
3. **Existing Infrastructure**: "Continue using familiar Git primitives, existing compute infrastructure (GitHub Actions or self-hosted runners)"
4. **Unified Governance**: Single control plane for all agents regardless of vendor

**Parallel Execution Success**: The shift from sequential to parallel agent execution showed dramatic improvements - "90 seconds of sequential agent handoffs into 30 seconds of parallel analysis"

**Custom Agent Configuration**: The agents.md and AGENTS.md file approach successfully reduced repetitive instruction overhead. Analysis of 2,500+ repositories showed clear patterns for effective configuration.

**Progressive Features**:
- Plan Mode: "Catch misunderstandings early before code is written"
- Code Quality Review: Automated maintainability checks before human review
- Session Transparency: Real-time logs showing agent reasoning

### What Failed / Limitations

**Limitations Acknowledged**:

1. **Not Production-Ready**: Many features marked "public preview" or "technical preview" indicating maturity concerns

2. **Early-Stage Controls**: "Early-stage issue flagging through code review capabilities" suggests validation mechanisms still developing

3. **Scope Management Challenges**: The extensive guidance on "drift detection" and "steering methodology" indicates agents frequently:
   - Expand scope beyond requirements
   - Misinterpret intent
   - Get stuck in circular/repetitive failed approaches
   - Create files outside intended scope

4. **Context Management**: Despite improvements, token limits remain a constraint:
   - Auto-compaction at 95% token capacity indicates frequent limit hitting
   - "Without cluttering your main context" (Explore agent) suggests context pollution is a real problem
   - Recommendation to keep SKILL.md under 500 lines/5000 tokens

5. **Platform Fragmentation**: Different feature availability across platforms:
   - JetBrains, Eclipse, Xcode support still in preview
   - CLI support "expected soon" for some features
   - Feature parity issues between VS Code and other IDEs

**Important Disclaimer from Anthropic Skills**: "These skills are provided for demonstration and educational purposes only. While some capabilities may be available in Claude, implementations and behaviors may differ. Always test thoroughly before relying on critical tasks."

### Key Success Factor

**Quote from best practices analysis**: "The best agent files grow through iteration, not upfront planning. Start minimal with one specific task, test performance, then add complexity based on actual agent mistakes rather than anticipated needs."

This suggests the platform requires significant human oversight and iteration, contradicting fully autonomous operation.

## 6. Open Source & Artifacts

### VS Code Extensions

**Primary Extension**: GitHub Copilot extension for VS Code integrates Agent HQ features
- Supports custom agents defined in `.github/agents/`
- Agent Skills framework enabled by default
- MCP server integration
- Plan Mode interface
- Session log viewer

**Installation**: Proprietary extension available through VS Code marketplace (not open source)

### Agent Skills Framework

**Open Standard**: Published December 18, 2025 at https://agentskills.io

**Adopters**: "Microsoft, OpenAI, Atlassian, Figma, Cursor, and GitHub have already adopted the standard"

**License**:
- Specification: Open standard (CC-BY-4.0 for documentation)
- Code: Apache 2.0

**Public Repositories**:

1. **agentskills/agentskills** - Specification and documentation
   - URL: https://github.com/agentskills/agentskills
   - Contains: Specification docs, tutorials, reference SDK

2. **anthropics/skills** - Reference implementations
   - URL: https://github.com/anthropics/skills
   - 65.4k stars, 6.5k forks
   - Contains: Example skills, templates, document processing implementations
   - Skills categories: Creative, Development, Enterprise, Documents (DOCX, PDF, PPTX, XLSX)
   - Note: Document skills are "source-available" (not fully open source)

3. **microsoft/skills** - Skills, MCP servers, agents.md templates
   - URL: https://github.com/microsoft/skills
   - 130+ skills for Azure SDK development
   - Categories: Foundry & AI (23), Data & Storage (16), Messaging & Events (10), Entra & Security (11), Monitoring
   - Languages: Python (41), .NET (28), TypeScript (24), Java (25), Rust (7), Core (5)

4. **github/awesome-copilot** - Community collection
   - URL: https://github.com/github/awesome-copilot
   - Contains: AGENTS.md examples, prompt templates, instruction files
   - Organized structure: agents/, prompts/, instructions/, skills/ directories

### SKILL.md Format

**Standard File Structure**:

```yaml
---
name: skill-name
description: Description of what skill does and when to use it
license: Apache-2.0  # optional
metadata:            # optional
  author: example-org
  version: "1.0"
---

# Skill Instructions

Step-by-step instructions, examples, and guidelines.
```

**Directory Structure**:
```
.github/skills/skill-name/  # or .claude/skills/ for legacy
├── SKILL.md                # Required
├── scripts/                # Optional executables
├── references/             # Optional detailed docs
└── assets/                 # Optional templates/data
```

**Progressive Disclosure**: Three-level loading system:
1. Metadata (~100 tokens): name + description loaded at startup
2. Instructions (<5000 tokens): Full SKILL.md loaded when activated
3. Resources (on-demand): Supporting files loaded only when referenced

**Validation Tool**: `skills-ref validate ./my-skill`

### GitHub Copilot SDK

**Repository**: https://github.com/github/copilot-sdk

**Status**: Technical Preview (January 14, 2026)

**License**: MIT

**Supported Languages**:
- Python (`pip install github-copilot-sdk`)
- TypeScript/Node.js (`npm install @github/copilot-sdk`)
- Go (`go get github.com/github/copilot-sdk/go`)
- .NET (`dotnet add package GitHub.Copilot.SDK`)

**Community SDKs**: Java, Rust, C++, Clojure (not officially maintained)

**Architecture**:
```
Application → SDK Client → JSON-RPC → Copilot CLI (server mode)
```

**Capabilities**:
- Agent runtime with planning, tool invocation, file editing
- BYOK support (OpenAI, Azure AI Foundry, Anthropic)
- Custom agents, skills, and tools
- Automatic CLI process lifecycle management

**Key Features**:
- Default tool configuration: all first-party tools enabled
- Per-prompt billing counted toward premium request quota
- Requires GitHub Copilot subscription (unless BYOK)

### MCP (Model Context Protocol) Integration

**GitHub MCP Registry**: https://github.com/mcp/github/github-mcp-server

Launched as "the fastest way to discover MCP servers" with:
- Self-publishing to OSS MCP Community Registry
- Automatic sync to GitHub MCP Registry
- One-click installation from VS Code Extensions view
- Enterprise registry configuration support

**Out-of-Box MCP Servers**:
- **GitHub MCP** (`github/*`) - Read-only repository tools
- **Playwright MCP** (`playwright/*`) - Browser automation, localhost-only

**Third-Party Integration**: Notion, Stripe, Figma, Sentry, Azure DevOps

**Registry Specification**: v0.1 MCP registry specification with standardized endpoint routing

### Configuration File Formats

**AGENTS.md** - Custom agent definitions
```yaml
---
name: agent-name
description: 'Agent purpose and capabilities'
target: vscode  # or github-copilot or both
tools: ['read', 'edit', 'search']  # or ["*"] for all
infer: true  # Auto-select based on context
---

Custom instructions defining agent behavior (max 30,000 characters)
```

**agents.md** (lowercase) - Repository-level instructions
- Analyzed across 2,500+ repositories
- Provides persona and pre-written context
- Reduces repetitive instruction overhead

**copilot-instructions.md** - General coding standards
- Located at `.github/copilot-instructions.md`
- Now applies to non-coding tasks (architecture, explanations)

### Open Source Ecosystem

**Community Resources**:

- **awesome-agent-skills** (heilcheng/awesome-agent-skills) - Curated skill list
- **Agent-Skills-for-Context-Engineering** (muratcankoylan) - Context management patterns
- **copilot-orchestra** (ShepAlderson) - Workflow coordination patterns
- **adg-parallels** (adamerso) - "AI Delegation Grid: scalable multi-agent orchestration for Copilot and LLMs inside VS Code"

**Documentation Sites**:
- https://agentskills.io - Official Agent Skills specification
- https://docs.github.com/en/copilot - GitHub Copilot documentation
- https://code.visualstudio.com/docs/copilot - VS Code Copilot docs

### Open Source Status Summary

| Component | Open Source? | License | Repository |
|-----------|--------------|---------|------------|
| Agent Skills Specification | Yes | CC-BY-4.0 | agentskills/agentskills |
| Agent Skills SDK | Yes | Apache 2.0 | agentskills/agentskills |
| Anthropic Skills Examples | Yes | Apache 2.0 | anthropics/skills |
| Anthropic Document Skills | Source-available | Proprietary | anthropics/skills |
| Microsoft Skills | Yes | Apache 2.0 | microsoft/skills |
| GitHub Copilot SDK | Yes | MIT | github/copilot-sdk |
| GitHub Copilot Extension | No | Proprietary | VS Code Marketplace |
| Agent HQ Platform | No | Proprietary | GitHub.com |
| Mission Control UI | No | Proprietary | GitHub.com |

## Sources

- [How to orchestrate agents using mission control - The GitHub Blog](https://github.blog/ai-and-ml/github-copilot/how-to-orchestrate-agents-using-mission-control/)
- [GitHub Agent HQ: Claude Codex Multi-Agent Platform - WinBuzzer](https://winbuzzer.com/2026/02/05/github-agent-hq-claude-codex-multi-agent-platform-xcxwbn/)
- [GitHub enables coding agents - Help Net Security](https://www.helpnetsecurity.com/2026/02/05/github-enables-coding-agents/)
- [GitHub Copilot CLI: Enhanced agents, context management - GitHub Changelog](https://github.blog/changelog/2026-01-14-github-copilot-cli-enhanced-agents-context-management-and-new-ways-to-install/)
- [What's new in VS Code Copilot January 2026 - alexop.dev](https://alexop.dev/posts/whats-new-vscode-copilot-january-2026/)
- [Introducing Agent HQ - The GitHub Blog](https://github.blog/news-insights/company-news/welcome-home-agents/)
- [Pick your agent: Use Claude and Codex on Agent HQ - The GitHub Blog](https://github.blog/news-insights/company-news/pick-your-agent-use-claude-and-codex-on-agent-hq/)
- [Creating custom agents - GitHub Docs](https://docs.github.com/en/copilot/how-tos/use-copilot-agents/coding-agent/create-custom-agents)
- [About Agent Skills - GitHub Docs](https://docs.github.com/en/copilot/concepts/agents/about-agent-skills)
- [Use Agent Skills in VS Code](https://code.visualstudio.com/docs/copilot/customization/agent-skills)
- [GitHub Copilot SDK Repository](https://github.com/github/copilot-sdk)
- [Agent Skills Specification](https://agentskills.io/specification)
- [Anthropic Skills Repository](https://github.com/anthropics/skills)
- [Microsoft Skills Repository](https://github.com/microsoft/skills)
- [GitHub Awesome Copilot Repository](https://github.com/github/awesome-copilot)
- [GitHub MCP Registry](https://github.blog/ai-and-ml/github-copilot/meet-the-github-mcp-registry-the-fastest-way-to-discover-mcp-servers/)
- [How to write a great agents.md - The GitHub Blog](https://github.blog/ai-and-ml/github-copilot/how-to-write-a-great-agents-md-lessons-from-over-2500-repositories/)
- [Tracking GitHub Copilot sessions - GitHub Docs](https://docs.github.com/en/copilot/how-tos/use-copilot-agents/coding-agent/track-copilot-sessions)
- [GitHub Copilot CLI: Plan before you build - GitHub Changelog](https://github.blog/changelog/2026-01-21-github-copilot-cli-plan-before-you-build-steer-as-you-go/)
