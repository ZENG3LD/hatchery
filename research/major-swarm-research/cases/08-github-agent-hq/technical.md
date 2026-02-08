# GitHub Agent HQ / Mission Control: Technical Specification

## 1. Prompts & Prompting Strategy

### Custom Agent Prompting

**Agent Profile Format** (YAML + Markdown):

```yaml
---
name: agent-identifier
description: 'Required: explains agent purpose and capabilities'
target: vscode  # or github-copilot or both (optional)
tools: ['read', 'edit', 'search']  # or ["*"] for all tools
infer: true  # Whether Copilot auto-selects based on context
model: gpt-4.1  # Strongly recommended
mcp-servers:  # Organization/enterprise level only
  custom-mcp:
    type: 'local'
    command: 'command-name'
    args: ['--arg1', '--arg2']
    tools: ["*"]
    env:
      VAR_NAME: $COPILOT_MCP_VAR_VALUE
metadata:  # Optional annotations
  author: org-name
  version: "1.0"
---

# Custom Instructions (max 30,000 characters)

Agent behavior, expertise, constraints, and guidelines.
```

**File Locations**:
- Repository level: `.github/agents/CUSTOM-AGENT-NAME.md`
- Organization/Enterprise level: `.github-private/agents/CUSTOM-AGENT-NAME.md`

**Configuration Hierarchy** (lowest to highest precedence):
```
Enterprise → Organization → Repository
```

### Tool Configuration Specification

**Tool Aliases** (case-insensitive):

| Alias | Tools | Capability |
|-------|-------|------------|
| `execute` | shell, bash, powershell | Execute commands |
| `read` | Read, NotebookRead | View file contents |
| `edit` | Edit, MultiEdit, Write, NotebookEdit | Modify files |
| `search` | Grep, Glob | Search files/text |
| `agent` | custom-agent, Task | Invoke other custom agents |
| `web` | WebSearch, WebFetch | Fetch URLs and search (not in coding agent) |
| `todo` | TodoWrite | Manage task lists (not in coding agent) |

**Tool Selection Patterns**:
```yaml
tools: ["*"]                    # All available tools (default)
tools: ["read", "edit"]         # Specific tools only
tools: []                       # Disable all tools
tools: ["github/*"]             # All GitHub MCP server tools
tools: ["custom-mcp/tool-1"]    # Specific MCP tool
```

**Fine-Grained Access Control**: "Custom agents can restrict tool availability through configuration. Create agent definitions at `.github/agents/[name].md` specifying allowed tools via a `tools` array, preventing unintended modifications."

### SKILL.md Prompting Format

**Complete Schema** (from agentskills.io specification):

```yaml
---
name: skill-name  # Required, 1-64 chars, lowercase + hyphens only
description: >    # Required, 1-1024 chars
  Extracts text and tables from PDF files, fills PDF forms,
  and merges multiple PDFs. Use when working with PDF documents
  or when the user mentions PDFs, forms, or document extraction.
license: Apache-2.0  # Optional
compatibility: >     # Optional, 1-500 chars
  Designed for Claude Code (or similar products)
metadata:            # Optional key-value map
  author: example-org
  version: "1.0"
allowed-tools: Bash(git:*) Bash(jq:*) Read  # Experimental
---

# Skill Instructions

Step-by-step instructions, examples, edge cases.
Agents load entire file when activated.
Keep under 500 lines / 5000 tokens recommended.
```

**Validation Rules**:

| Field | Constraint |
|-------|------------|
| `name` | Must match parent directory, no consecutive `--`, no leading/trailing `-` |
| `description` | Must include both what skill does AND when to use it |
| `allowed-tools` | Space-delimited list, experimental feature |

**Directory Structure**:
```
.github/skills/skill-name/  # or .claude/skills/ for backward compatibility
├── SKILL.md                # Required
├── scripts/                # Optional executables
│   └── extract.py
├── references/             # Optional detailed docs
│   ├── REFERENCE.md
│   └── finance.md
└── assets/                 # Optional templates/data
    └── template.json
```

**File Referencing**: Use relative paths from skill root:
```markdown
See [the reference guide](references/REFERENCE.md) for details.

Run the extraction script:
scripts/extract.py
```

**Validation Command**: `skills-ref validate ./my-skill`

### Effective Prompting Strategies

**From 2,500+ Repository Analysis**:

**What Works**:

1. **Persona-First**: "Expert technical writer fluent in Markdown" > "helpful coding assistant"

2. **Commands Early**: Put specific commands with flags at top, not just tool names
   ```markdown
   ## Commands
   - `npm test -- --coverage`
   - `pytest -v --tb=short`
   - `prettier --write "**/*.{js,ts}"`
   ```

3. **Real Code Examples**: Code snippets > lengthy explanations

4. **Three-Tier Boundaries**:
   ```markdown
   ## Boundaries
   ✅ Always: Write to `docs/` only, run `markdownlint`, validate links
   ⚠️ Ask First: Changes to API contracts, database schemas
   🚫 Never: Remove failing tests, modify `main` branch, commit secrets
   ```

5. **Six Essential Areas**:
   - Commands (with flags)
   - Testing (specific frameworks + commands)
   - Project structure (directory layout)
   - Code style (linters, formatters)
   - Git workflow (branching strategy)
   - Boundaries (what to never touch)

**What Doesn't Work**:

- Generic personas without specific job definitions
- Vague requests like "Fix the authentication bug" without context
- Lengthy explanations without executable examples
- Upfront planning all constraints (instead: iterate based on actual mistakes)

**Strong vs Weak Prompts**:

| Weak | Strong |
|------|--------|
| "Fix the authentication bug" | "Fix OAuth token refresh in `auth.ts`. Reference the pattern in `admin-auth.ts`. Test with `npm run test:auth`" |
| "Helpful coding assistant" | "Backend API developer. Write TypeScript REST endpoints using Express. Follow patterns in `src/api/`. Never modify database schemas without approval." |

**Key Insight**: "The best agent files grow through iteration, not upfront planning. Start minimal with one specific task, test performance, then add complexity based on actual agent mistakes rather than anticipated needs."

### Plan Mode Prompting

**Interactive Planning Process**:

1. Agent analyzes request
2. **Clarifying Questions**: "Copilot uses the new `ask_user` tool to prompt you with follow-up questions, confirm assumptions about feature scope, and get your input on design decisions."
3. Generates structured implementation plan
4. User reviews plan in dedicated panel
5. User approves and agent begins implementation

**Benefits**:
- "Catch misunderstandings early before code is written"
- "Make informed decisions about implementation approach"
- "Stay in control of complex multistep tasks"

**Activation**: Press `Shift + Tab` to cycle in/out of plan mode (CLI)

### agents.md Best Practices

**Core Success Patterns** (from GitHub's analysis):

1. **Specific Tech Stack**: "Python 3.11, FastAPI 0.104, PostgreSQL 15" > "Python web app"

2. **Clear Job Definition**: Define WHO the agent is before WHAT it does

3. **Executable Commands**: Include actual tools agents can run with relevant flags

**Example Agent Definitions**:

**Docs Agent**:
```markdown
You are an expert technical writer.

Tasks:
- Read code from `src/`
- Generate API documentation in Markdown
- Validate with `markdownlint`
- Write only to `docs/` directory

Never modify source code.
```

**Test Agent**:
```markdown
You write unit tests using pytest.

Tasks:
- Write tests in `tests/` folder
- Run `pytest -v` to validate
- Follow patterns in existing tests

Never remove failing tests. Ask before changing fixtures.
```

**Lint Agent**:
```markdown
You fix code style using prettier.

Command: `prettier --write "**/*.{js,ts}"`

Only modify formatting—never logic.
```

## 2. Memory & Context Management

### Context Management Enhancements

**Auto-Compaction** (January 2026):
- "System automatically compresses history when reaching 95% token capacity"
- Prevents context overflow without user intervention

**Manual Controls**:
- `/compact` command: Manual context compression
- `/context` command: Displays detailed token usage breakdown

**Session Resumption**:
- `--resume` flag: "Cycle through local and remote Copilot coding agent sessions via TAB"

### Progressive Disclosure Loading System

**Three-Level Architecture** (Agent Skills):

| Level | Token Budget | Content | When Loaded |
|-------|--------------|---------|-------------|
| 1. Discovery | ~100 tokens | `name` + `description` | Startup (all skills) |
| 2. Instructions | <5000 tokens | Full `SKILL.md` body | When skill activated |
| 3. Resources | Variable | `scripts/`, `references/`, `assets/` | On-demand reference |

**Design Rationale**: "You can install many skills, and Copilot will load only what's relevant for each task."

**Optimization Guidelines**:
- Keep main `SKILL.md` under 500 lines
- Move detailed reference material to separate files
- Keep individual reference files focused
- Avoid deeply nested reference chains (max 1 level from SKILL.md)

**Quote**: "Note that the agent will load this entire file once it's decided to activate a skill. Consider splitting longer SKILL.md content into referenced files."

### Agent-Specific Context Isolation

**Explore Agent**: "Fast codebase analysis. Ask questions about your code **without cluttering your main context**." (emphasis added)

This indicates separate context windows for different agents to prevent pollution of the main conversation context.

### Session Persistence

**Information Captured**:
- Agent reasoning and decision-making process
- Tools invoked and their outputs
- File modifications
- Test results and validation steps
- Token usage metrics
- Session duration and status

**Access Points**:
- GitHub.com agents tab: `github.com/copilot/agents`
- CLI: `gh agent-task list`, `gh agent-task view`
- IDE integrations (VS Code, JetBrains, Eclipse, Xcode)
- Mobile apps
- Raycast extension

**Quote**: "Session logs serve as reasoning artifacts for improving future prompts. The system maintains execution transparency, showing decision-making processes that inform subsequent orchestration decisions."

### Custom Instructions Hierarchy

**Scope Levels** (narrow to broad):

1. **File-specific**: `*.instructions.md` with `applyTo` glob patterns
2. **Directory-specific**: `.github/instructions/*.instructions.md`
3. **Repository-wide**: `.github/copilot-instructions.md`
4. **Agent Skills**: `.github/skills/*/SKILL.md` (task-specific, on-demand)
5. **Custom Agents**: `.github/agents/*.md` (specialized personas)

**Universal Application**: "`.github/copilot-instructions.md` now applies to non-coding tasks like architecture explanations" (January 2026 update)

### Context vs Skills vs Instructions

**From VS Code documentation**:

| Feature | Purpose | Activation | Portability |
|---------|---------|------------|-------------|
| **Agent Skills** | Specialized capabilities and workflows | Task-specific, on-demand loading | Cross-tool (VS Code, CLI, coding agent) |
| **Custom Instructions** | Coding standards and guidelines | Always applied or glob-pattern based | VS Code and GitHub.com only |

**Key Distinction**: Skills enable "specialized capabilities" with bundled resources (scripts, templates). Instructions provide "coding standards and guidelines" as pure text.

## 3. Task Distribution & Scheduling

### Agent Routing Mechanism

**Automatic Selection** (with `infer: true`):

Copilot evaluates custom agent descriptions against user prompts and automatically selects the most relevant agent. "When Copilot chooses to use a skill, the SKILL.md file will be injected in the agent's context, giving the agent access to your instructions."

**Manual Selection**:

Users can explicitly invoke agents:
- Via Mission Control interface: "pick a custom agent"
- CLI: Invoke specific specialized agents (Explore, Task, Plan, Code-review)
- IDE: Select agent from available options

**Skill Discovery Protocol**:

1. All skill metadata loaded at startup (~100 tokens per skill)
2. User submits prompt
3. Agent evaluates prompt against skill descriptions
4. Matching skills activate (SKILL.md body loaded)
5. Agent can reference supporting files on-demand

### Parallel Agent Execution

**Concurrent Execution Model**:

**Quote**: "Copilot can now run multiple agents simultaneously rather than sequentially. What this means in practice: when developers invoke a complex task like debugging authentication failures, they no longer wait for one agent to finish exploring code patterns before another tests credentials, then a third reviews security implications. All three execute concurrently, transforming what might take 90 seconds of sequential agent handoffs into 30 seconds of parallel analysis."

**Subagent Parallelization** (January 2026):

"Subagents can now run simultaneously when tasks are independent. Previously, multiple `runSubagent` calls executed sequentially; this update 'dramatically reduces wait times for research and code review operations.'"

**Optimal Parallel Workflows** (from Mission Control guide):

✅ **Good for Parallelization**:
- Research and analysis tasks
- Documentation generation
- Security reviews
- Work in separate modules/components

❌ **Keep Sequential**:
- Tasks with dependencies
- Work requiring assumption validation
- Changes to same files (merge conflicts)

### Task Partitioning Strategy

**Quote**: "Effective parallelization requires avoiding merge conflicts. Optimal parallel workflows include: Research and analysis tasks, Documentation generation, Security reviews, Work in separate modules/components. Tasks with dependencies or requiring assumption validation should remain sequential."

**Orchestration Pattern**:

```
User → Mission Control → Task Assignment → Multiple Agents (Parallel)
                              ↓
                    Separate Branches/Files
                              ↓
                      Pull Requests → Review
```

### Specialized Agent Delegation

**Built-in Agent Roles**:

| Agent | Delegation Use Case | Output |
|-------|---------------------|--------|
| **Explore** | Codebase questions, architecture understanding | Brief summaries without cluttering main context |
| **Task** | Running tests, builds, CI commands | "Brief summaries on success, full output on failure" |
| **Plan** | Implementation roadmaps, dependency analysis | Structured step-by-step implementation plans |
| **Code-review** | Change validation, quality checks | "Only surfacing genuine issues," minimizing noise |

**Automatic Delegation**: "The system allows Copilot to delegate to these agents automatically and can execute multiple agents simultaneously."

### Multi-Agent Comparison

**Competitive Execution**:

"Agent HQ also lets you compare how different agents approach the same problem. You can assign multiple agents to a task, and see how Copilot, Claude, and Codex reason about tradeoffs and arrive at different solutions."

**Benefits**: "Running agents in parallel, you get competing approaches and edge cases before code hardens."

**Use Cases**:
- Architecture decisions with multiple valid approaches
- Performance optimization strategies
- Security review from different model perspectives

## 4. Validation & Quality Control

### Code Review Agent

**Capabilities**:

- **Selective Issue Detection**: "Focuses on surfacing genuine issues rather than nitpicking style preferences"
- **Pre-human Review**: "GitHub has integrated a code review step directly into the Copilot's workflow, allowing Copilot to address initial problems before a developer ever sees the code"
- **Real-time Analysis**: Evaluates changes as agent works

**Integration Point**: Automated step in agent workflow before PR creation

### GitHub Code Quality

**Status**: Public preview

**Capabilities**:

"GitHub Code Quality (in public preview) extends Copilot's security checks to evaluate the maintainability and reliability impact of changed code, helping ensure 'LGTM' reflects long-term code health."

**Assessment Scope**:
- **Maintainability**: Code structure, complexity, readability
- **Reliability**: Error handling, edge cases, robustness
- **Security**: Existing security checks + new maintainability-focused checks

**Workflow Integration**: Repository-wide analysis as automated review step

### Multi-Layer Validation Architecture

**From Mission Control Guide**:

**Layer 1: Session Logs**
- Reveal reasoning before implementation
- Enable early detection of intent misinterpretation

**Layer 2: File Change Analysis**
- Focus on unexpected modifications
- Critical code path review
- Scope boundary validation

**Layer 3: Test Suite Verification**
- Unit tests
- Integration tests (e.g., Playwright)
- CI/CD pipeline checks

**Layer 4: Agent-Powered Self-Critique**

"The guide suggests leveraging agents for self-critique: requesting edge case identification, test coverage gaps, and failing test analysis. This treats agents as junior developers explaining their reasoning."

### Validation Before Push

**Isolated Development Environment**:

"The agent operates with its own development environment where it can run automated tests and linters, to validate its changes before it pushes."

**Pre-Push Checks**:
- Automated test execution
- Linter validation
- Build verification
- Code quality assessment

### PR Validation Workflow

**Standard Review Process**:

"Agent-generated changes follow standard review processes—developers review agent work the same way they would review teammate contributions. Changes remain attached to repository pull requests rather than existing as standalone outputs."

**Review Integration**:
- Session logs accessible from PR interface
- Agent reasoning visible to reviewers
- Standard approval/request changes workflow
- Merge conflict resolution: "one-click merge conflict resolution"

### Metrics Dashboard

**Status**: Public preview

**Capabilities**:

"The Copilot metrics dashboard (in public preview) can track usage and impact across your entire organization, providing clear traceability for agent-generated work."

**Tracked Metrics**:
- Token usage
- Session count
- Session length
- Adoption across organization
- Agent impact on development workflows

## 5. Mailbox / Inbox Implementation

### Session-Based Messaging

**Agent-to-User Communication**:

**Clarifying Questions** (Plan Mode):
- Agent uses `ask_user` tool to prompt with follow-up questions
- User responds inline
- Agent incorporates responses into plan

**Status Updates**:
- Real-time session logs show agent progress
- Notifications on session completion
- Mobile app notifications

**Error Reporting**:
- Task agent: "brief summaries on success, **full output on failure**"
- Detailed error logs in session viewer

### Cross-Agent Communication

**NOT DISCLOSED**: Direct agent-to-agent messaging protocol not documented in available sources.

**Inferred Patterns**:

**Subagent Delegation**:
- Parent agent invokes subagent via `runSubagent` calls
- Subagent returns results to parent
- Results incorporated into parent's context

**Shared Context**:
- Multiple agents working on same task may share session context
- Session logs visible across agent invocations
- Results from parallel agents accessible for comparison

### User Steering Interface

**Mid-Run Intervention**:

"Users can actively steer sessions by providing additional prompts while work is ongoing, or terminate sessions entirely using the 'Stop session' button in the log viewer."

**Steering Capabilities**:
- Pause agent execution
- Provide clarifying context
- Refine requirements
- Redirect approach
- Restart with new strategy

**Communication Methods**:
- Inline prompts in session log
- Stop/resume controls
- CLI: `--follow` for streaming interaction

### Asynchronous Execution Model

**Default Behavior**:

"Agents run asynchronously by default. You can follow their progress in real time or review completed sessions later, with detailed logs showing what the agent did and why."

**Benefits**:
- Non-blocking: Continue other work while agent executes
- Multi-device access: Start on desktop, check on mobile
- Session persistence: Pick up where you left off

**Access Pattern**:
```
User submits task → Agent starts (async) → User navigates away
                         ↓
                  Agent works independently
                         ↓
                  Notification on completion
                         ↓
            User reviews results in session log
```

### Third-Party Integration Hooks

**Messaging Platforms**:
- Slack
- Microsoft Teams
- Jira
- Linear
- Azure Boards
- Raycast (macOS)

These integrations likely enable:
- Agent task assignment from external tools
- Notifications on agent completion
- Status updates in team channels

**Implementation Details**: NOT DISCLOSED in available sources

### Session Log as Communication Artifact

**Quote**: "Session logs serve as reasoning artifacts for improving future prompts. The system maintains execution transparency, showing decision-making processes that inform subsequent orchestration decisions."

**Communication Flow**:
```
Agent → Session Log → User Review → Improved Prompts → Next Agent Session
```

**Logged Information**:
- "Copilot's internal monologue"
- Tools invoked and outputs
- Reasoning for decisions
- File changes with rationale
- Test results and validation steps

## 6. Open Source Artifacts & Code

### Comprehensive Open Source Inventory

#### 1. Agent Skills Specification (OPEN STANDARD)

**Repository**: https://github.com/agentskills/agentskills

**License**: Apache 2.0 (code), CC-BY-4.0 (docs)

**Status**: Active, published Dec 18, 2025

**Contents**:
- Complete SKILL.md format specification
- Progressive disclosure loading mechanism
- Validation tools: `skills-ref` reference library
- Documentation and tutorials

**Adopters**: "Microsoft, OpenAI, Atlassian, Figma, Cursor, and GitHub"

**Specification Site**: https://agentskills.io/specification

**Key Quote**: "Anthropic builds specifications that solve genuine interoperability problems, releases them as open standards, and lets adoption create value that accrues to the ecosystem rather than to Anthropic alone."

**Full Specification** (from agentskills.io):

```yaml
# SKILL.md Frontmatter Schema
---
name: string              # Required, 1-64 chars, lowercase+hyphens
description: string       # Required, 1-1024 chars, what+when
license: string           # Optional, license name or file reference
compatibility: string     # Optional, 1-500 chars, environment requirements
metadata:                 # Optional, key-value map
  author: string
  version: string
  [arbitrary-key]: string
allowed-tools: string     # Optional, space-delimited, experimental
---

# Body: Markdown instructions (no restrictions)
# Recommended: <500 lines, <5000 tokens
# Include: step-by-step instructions, examples, edge cases
```

**Directory Structure Standard**:
```
skill-name/
├── SKILL.md              # Required
├── scripts/              # Optional executables (Python, Bash, JS)
├── references/           # Optional docs loaded on-demand
│   ├── REFERENCE.md
│   └── [domain].md
└── assets/               # Optional templates, images, data
    └── [resource-files]
```

**Progressive Disclosure Specification**:
1. **Metadata**: All skill `name` + `description` fields loaded at startup
2. **Instructions**: Full `SKILL.md` body loaded when skill matches task
3. **Resources**: Referenced files loaded only when agent accesses them

**Validation Tool**:
```bash
skills-ref validate ./skill-name
```

Checks:
- YAML frontmatter validity
- Naming conventions (lowercase-with-hyphens)
- Required field presence
- Character count limits
- Directory name matches `name` field

---

#### 2. Anthropic Skills Repository (REFERENCE IMPLEMENTATIONS)

**Repository**: https://github.com/anthropics/skills

**Stats**: 65.4k stars, 6.5k forks, 20 commits (main branch)

**License**:
- Example skills: Apache 2.0 (open source)
- Document skills (docx, pdf, pptx, xlsx): Source-available (NOT open source)

**Languages**: Python (91.3%), HTML (4.5%), Shell (2.5%), JavaScript (1.7%)

**Structure**:
```
anthropics/skills/
├── .claude-plugin/       # Claude Code plugin configuration
├── skills/               # Skill examples
│   ├── docx/             # Word document creation/editing
│   ├── pdf/              # PDF manipulation
│   ├── pptx/             # PowerPoint creation
│   └── xlsx/             # Excel spreadsheet creation
├── spec/                 # Agent Skills specification
├── template/             # Skill template starter
└── THIRD_PARTY_NOTICES.md
```

**Skill Categories**:
1. **Creative & Design** - Art, music, design applications
2. **Development & Technical** - Testing web apps, MCP server generation
3. **Enterprise & Communication** - Branding, communications, workflows
4. **Document Skills** - DOCX, PDF, PPTX, XLSX (production reference implementations)

**Template Example**:
```yaml
---
name: my-skill-name
description: A clear description of what this skill does and when to use it
---

# My Skill Name

[Add your instructions here that Claude will follow when this skill is active]

## Examples
- Example usage 1
- Example usage 2

## Guidelines
- Guideline 1
- Guideline 2
```

**Usage in Claude Code**:
```bash
/plugin marketplace add anthropics/skills
/plugin install document-skills@anthropic-agent-skills
/plugin install example-skills@anthropic-agent-skills
```

**Important Disclaimer**:
> "These skills are provided for demonstration and educational purposes only. While some capabilities may be available in Claude, implementations and behaviors may differ. Always test thoroughly before relying on critical tasks."

**Partner Integrations**:
- Notion published [Notion Skills for Claude](https://www.notion.so/notiondevs/Notion-Skills-for-Claude-28da4445d27180c7af1df7d8615723d0)

---

#### 3. Microsoft Skills Repository (PRODUCTION SDK PATTERNS)

**Repository**: https://github.com/microsoft/skills

**License**: Apache 2.0

**Scope**: "Skills, MCP servers, Custom Agents, Agents.md for SDKs to ground Coding Agents"

**Scale**: 130+ skills across 5 languages + core skills

**Structure**:
```
microsoft/skills/
├── .github/skills/       # Skill library
│   ├── [skill-name]/
│   │   ├── SKILL.md
│   │   └── skill.yml
│   ├── Core skills (5)
│   ├── Python skills (41) - suffix: -py
│   ├── .NET skills (28) - suffix: -dotnet
│   ├── TypeScript skills (24) - suffix: -ts
│   ├── Java skills (25) - suffix: -java
│   └── Rust skills (7) - suffix: -rust
├── .claude/              # Claude agent configuration
├── .opencode/            # OpenCode agent configuration
├── docs-site/            # Documentation site
└── tests/                # Evaluation harnesses
```

**Skill Categories**:

| Category | Count | Examples |
|----------|-------|----------|
| Foundry & AI | 23 | Agent frameworks, content safety, vision, translation |
| Data & Storage | 16 | Cosmos DB, Blob Storage, Data Lake, Tables |
| Messaging & Events | 10 | Event Grid, Event Hubs, Service Bus |
| Entra & Security | 11 | Identity, Key Vault, authentication |
| Monitoring | - | OpenTelemetry, Application Insights |
| Communication | - | Call automation, chat, SMS (Java-focused) |

**Design Philosophy**:

"Skills surface SDK patterns already in model weights through 'right activation context' rather than teaching new patterns."

**Context Management**:

"Documentation emphasizes loading 'only skills essential for current project' to prevent 'context rot: diluted attention, wasted tokens, conflated patterns.'"

**Multi-Agent Symlinks**:
```bash
ln -s ../.github/skills .opencode/skills
ln -s ../.github/skills .claude/skills
```

**Language Detection**: Automatic suffix-based discovery (e.g., `-py` → Python skills)

**MCP Integration**:
- **mcp-builder skill**: Generates MCP servers (Python FastMCP, Node/TypeScript, C#/.NET)
- Pre-configured MCP servers for documentation, GitHub, browser automation

**Agents.md Template**: Provides role-specific agent definitions:
- Backend developer agents
- Frontend developer agents
- Infrastructure agents
- Planner agents

---

#### 4. GitHub Copilot SDK (OFFICIAL AGENT RUNTIME)

**Repository**: https://github.com/github/copilot-sdk

**License**: MIT

**Status**: Technical Preview (Jan 14, 2026)

**Supported Languages**:

| Language | Installation | Package |
|----------|--------------|---------|
| Python | `pip install github-copilot-sdk` | github-copilot-sdk |
| TypeScript/Node.js | `npm install @github/copilot-sdk` | @github/copilot-sdk |
| Go | `go get github.com/github/copilot-sdk/go` | github.com/github/copilot-sdk/go |
| .NET | `dotnet add package GitHub.Copilot.SDK` | GitHub.Copilot.SDK |

**Community SDKs** (not officially maintained):
- Java
- Rust
- C++
- Clojure

**Repository Structure**:
```
github/copilot-sdk/
├── nodejs/               # TypeScript/JavaScript implementation
├── python/               # Python SDK
├── go/                   # Go implementation
├── dotnet/               # .NET/C# implementation
├── docs/                 # Comprehensive documentation
└── test/                 # Test suite across all languages
```

**Architecture**:

```
Application Code
       ↓
   SDK Client (Python/TS/Go/.NET)
       ↓
   JSON-RPC Protocol
       ↓
Copilot CLI (Server Mode)
       ↓
  LLM Backend (GitHub Copilot / BYOK)
```

**Core Capabilities**:
- Agent runtime with automatic planning
- Tool invocation orchestration
- File system operations
- Git repository management
- Web request handling
- Automatic CLI process lifecycle management

**Authentication Methods**:
```python
# GitHub signed-in user credentials (default)
# OAuth GitHub App tokens
# Environment variables
export COPILOT_GITHUB_TOKEN=ghp_xxx
export GH_TOKEN=ghp_xxx
export GITHUB_TOKEN=ghp_xxx

# BYOK (Bring Your Own Key)
# Supports: OpenAI, Azure AI Foundry, Anthropic
```

**Default Tool Configuration**:

By default, all first-party tools are enabled (equivalent to `--allow-all` flag):
- Shell execution (bash, powershell)
- File operations (Read, Write, Edit)
- Search (Grep, Glob)
- Git operations
- Web requests (WebFetch, WebSearch)

**Customization**:
```python
# Define custom agents
agent = CopilotAgent(
    name="my-agent",
    description="Specialized agent for X",
    tools=["read", "edit", "custom-tool"],
    skills=["skill-1", "skill-2"]
)

# Implement custom tools
@copilot.tool("custom-tool")
def my_custom_tool(param: str) -> str:
    # Implementation
    return result
```

**Billing**: Per-prompt billing counted toward premium request quota (unless BYOK)

**Requirements**: GitHub Copilot subscription (Pro/Enterprise) unless using BYOK

**Key Features**:
- Production-tested agent engine (same as Copilot CLI)
- Programmable orchestration
- Multi-language support
- BYOK for vendor flexibility
- Custom agents, skills, tools extensibility

**Documentation**: `docs/getting-started.md` in repository

---

#### 5. GitHub Awesome Copilot Collection (COMMUNITY PATTERNS)

**Repository**: https://github.com/github/awesome-copilot

**Structure**:
```
github/awesome-copilot/
├── agents/               # Custom agent definitions (*.agent.md)
├── prompts/              # Task-specific prompts (*.prompt.md)
├── instructions/         # Coding standards (*.instructions.md)
├── skills/               # Self-contained SKILL.md folders
├── docs/
│   └── README.skills.md
└── AGENTS.md             # Main documentation
```

**File Type Specifications**:

**Agent Files** (`*.agent.md`):
```yaml
---
description: 'Required, single-quoted'
tools: ['tool-a', 'tool-b']           # Recommended
model: 'gpt-4.1'                      # Strongly recommended
---

Agent instructions here
```

**Prompt Files** (`*.prompt.md`):
```yaml
---
agent: 'agent'                        # Required, single-quoted
description: 'Non-empty, single-quoted'
tools: ['tool-a', 'tool-b']           # Recommended
model: 'gpt-4.1'                      # Strongly recommended
---

Prompt content here
```

**Instruction Files** (`*.instructions.md`):
```yaml
---
description: 'Non-empty, single-quoted'
applyTo: '**.js, **.ts'               # File patterns
---

Coding standards and guidelines
```

**Skills** (`skills/*/SKILL.md`):
```yaml
---
name: skill-name                      # Lowercase with hyphens, max 64 chars
description: 'What and when'          # 10-1024 characters
---

Skill instructions + bundled assets (scripts, templates, data <5MB each)
```

**Development Workflow**:
```bash
# Setup
npm ci
npm run build

# Validation
npm run collection:validate
npm run skill:validate

# Pre-commit
bash scripts/fix-line-endings.sh
npm run build  # Auto-updates README
```

**Naming Convention**: Consistent lowercase-with-hyphens (e.g., `address-comments.agent.md`)

**Best Practices**:
- Run validators before PRs
- Normalize line endings to LF (Unix-style)
- Wrap description values in single quotes
- Include all required front matter fields
- README generation automated via `npm run build`

---

#### 6. GitHub MCP Registry & Servers

**Registry Announcement**: https://github.blog/ai-and-ml/github-copilot/meet-the-github-mcp-registry-the-fastest-way-to-discover-mcp-servers/

**Official MCP Servers**:

**GitHub MCP Server**:
- **Repository**: https://github.com/mcp/github/github-mcp-server
- **Status**: General Availability
- **Scope**: "Allows agents to connect with the rich context found in GitHub repositories, issues, and pull requests"
- **Access**: Read-only tools by default, scoped to source repository
- **Reference**: `github/*` or `github/<tool-name>`

**Playwright MCP Server**:
- **Scope**: Browser automation
- **Access**: All tools available, localhost-only
- **Reference**: `playwright/*` or `playwright/<tool-name>`

**Azure DevOps MCP**:
- **Repository**: https://github.com/mcp/microsoft/azure-devops-mcp
- **Scope**: Azure DevOps integration

**MCP Registry Specification**: v0.1 MCP registry specification

**Enterprise Registry Configuration**:

Organizations can configure custom MCP registries:
- Registry URL configuration
- Access control policies
- Determine which MCP servers developers can discover
- Support in VS Code and other IDEs

**MCP Server Installation** (VS Code):
1. Extensions view → Browse MCP Servers
2. Find desired server from list
3. Click Install next to each

**Third-Party MCP Servers**:
- Notion
- Stripe
- Figma
- Sentry
- Atlassian

**Self-Publishing**:

"Developers can self-publish MCP servers directly to the OSS MCP Community Registry, and once published, those servers automatically appear in the GitHub MCP Registry, creating a unified, scalable path for discovery."

---

#### 7. Community Open Source Projects

**copilot-orchestra** (ShepAlderson/copilot-orchestra):
- Workflow coordination pattern
- Complete AI development cycle: planning → implementation → review → commit
- Specialized subagent orchestration

**adg-parallels** (adamerso/adg-parallels):
- "AI Delegation Grid: scalable multi-agent orchestration for Copilot and LLMs inside VS Code"
- Parallel task execution
- Adapter patterns
- Automation workflows

**awesome-agent-skills** (heilcheng/awesome-agent-skills):
- "A curated list of skills, tools, tutorials, and capabilities for AI coding agents (Claude, Codex, Copilot, VS Code)"

**awesome-agent-skills** (skillmatic-ai/awesome-agent-skills):
- "The definitive resource for Agent Skills - modular capabilities revolutionizing AI agent architecture"

**Agent-Skills-for-Context-Engineering** (muratcankoylan):
- "A comprehensive collection of Agent Skills for context engineering, multi-agent architectures, and production agent systems"
- Context management patterns
- Multi-agent architecture patterns

**pi-subagents** (nicobailon/pi-subagents):
- "Pi extension for async subagent delegation with truncation, artifacts, and session sharing"

---

### Proprietary Components (NOT Open Source)

| Component | Status | Access |
|-----------|--------|--------|
| **Agent HQ Platform** | Proprietary | GitHub.com (Copilot Pro+/Enterprise) |
| **Mission Control UI** | Proprietary | GitHub.com, Mobile apps |
| **GitHub Copilot Extension** (VS Code) | Proprietary | VS Code Marketplace |
| **Copilot CLI Binary** | Proprietary | Via npm/homebrew installation |
| **Agent Runtime Core** | Proprietary | Accessed via SDK (MIT), not source-available |
| **Model Backends** | Proprietary | GitHub Copilot, Claude, Codex APIs |

---

### Installation & Setup Examples

**Agent Skills in VS Code**:
```bash
# Skills auto-discovered from:
.github/skills/          # Project skills (recommended)
.claude/skills/          # Legacy compatibility
~/.copilot/skills/       # Personal skills

# Configure additional locations:
# settings.json
{
  "chat.agentSkillsLocations": [
    ".github/skills",
    "~/shared-skills"
  ]
}
```

**Custom Agents in Repository**:
```bash
# Create agent profile
mkdir -p .github/agents
cat > .github/agents/docs-writer.md <<EOF
---
description: 'Expert technical writer specializing in API documentation'
tools: ['read', 'edit']
model: 'gpt-4.1'
---

You write clear, concise API documentation in Markdown.

## Tasks
- Read code from src/
- Generate API docs
- Validate with markdownlint
- Write only to docs/ directory

## Constraints
Never modify source code.
EOF
```

**GitHub Copilot SDK (Python)**:
```python
from github_copilot_sdk import CopilotAgent

# Create agent with custom skills
agent = CopilotAgent(
    name="my-agent",
    description="Custom agent for X",
    tools=["read", "edit", "search"],
    skills=[".github/skills/my-skill"]
)

# Invoke agent
result = await agent.run("Implement feature X")

# Access session logs
logs = agent.get_session_logs()
```

**MCP Server Configuration** (Organization level):
```yaml
---
name: org-agent
description: 'Agent with custom MCP server'
mcp-servers:
  stripe-mcp:
    type: 'local'
    command: 'npx'
    args: ['@stripe/mcp-server']
    tools: ["*"]
    env:
      STRIPE_API_KEY: ${{ secrets.COPILOT_MCP_STRIPE_KEY }}
tools: ['read', 'stripe-mcp/*']
---

Agent with Stripe API access for payment integration tasks.
```

---

### Summary: Open Source vs Proprietary

**Fully Open Source** (Apache 2.0 / MIT):
- ✅ Agent Skills specification (agentskills.io)
- ✅ GitHub Copilot SDK (github/copilot-sdk)
- ✅ Anthropic example skills (anthropics/skills, most skills)
- ✅ Microsoft skills library (microsoft/skills)
- ✅ GitHub MCP servers (github-mcp-server, others)
- ✅ Validation tools (skills-ref)

**Source-Available** (limited license):
- ⚠️ Anthropic document skills (docx, pdf, pptx, xlsx)

**Proprietary** (closed source):
- ❌ Agent HQ platform
- ❌ Mission Control UI
- ❌ VS Code Copilot extension
- ❌ Copilot CLI binary
- ❌ Agent runtime core (accessed via SDK, not source-available)
- ❌ Model backends (Copilot, Claude, Codex APIs)

**Key Insight**: GitHub/Anthropic/Microsoft released the **orchestration patterns and interfaces** as open source (Agent Skills, Copilot SDK) while keeping the **execution platform and models** proprietary. This enables ecosystem growth while maintaining commercial control.

## Sources

- [How to orchestrate agents using mission control - The GitHub Blog](https://github.blog/ai-and-ml/github-copilot/how-to-orchestrate-agents-using-mission-control/)
- [GitHub Copilot CLI: Enhanced agents, context management - GitHub Changelog](https://github.blog/changelog/2026-01-14-github-copilot-cli-enhanced-agents-context-management-and-new-ways-to-install/)
- [What's new in VS Code Copilot January 2026 - alexop.dev](https://alexop.dev/posts/whats-new-vscode-copilot-january-2026/)
- [Creating custom agents - GitHub Docs](https://docs.github.com/en/copilot/how-tos/use-copilot-agents/coding-agent/create-custom-agents)
- [About custom agents - GitHub Docs](https://docs.github.com/en/copilot/concepts/agents/coding-agent/about-custom-agents)
- [Custom agents configuration - GitHub Docs](https://docs.github.com/en/copilot/reference/custom-agents-configuration)
- [How to write a great agents.md - The GitHub Blog](https://github.blog/ai-and-ml/github-copilot/how-to-write-a-great-agents-md-lessons-from-over-2500-repositories/)
- [About Agent Skills - GitHub Docs](https://docs.github.com/en/copilot/concepts/agents/about-agent-skills)
- [Use Agent Skills in VS Code](https://code.visualstudio.com/docs/copilot/customization/agent-skills)
- [Agent Skills Specification](https://agentskills.io/specification)
- [GitHub Copilot SDK Repository](https://github.com/github/copilot-sdk)
- [Anthropic Skills Repository](https://github.com/anthropics/skills)
- [Microsoft Skills Repository](https://github.com/microsoft/skills)
- [GitHub Awesome Copilot Repository](https://github.com/github/awesome-copilot)
- [Tracking GitHub Copilot sessions - GitHub Docs](https://docs.github.com/en/copilot/how-tos/use-copilot-agents/coding-agent/track-copilot-sessions)
- [GitHub Copilot CLI: Plan before you build - GitHub Changelog](https://github.blog/changelog/2026-01-21-github-copilot-cli-plan-before-you-build-steer-as-you-go/)
- [Introducing Agent HQ - The GitHub Blog](https://github.blog/news-insights/company-news/welcome-home-agents/)
- [Pick your agent: Use Claude and Codex on Agent HQ - The GitHub Blog](https://github.blog/news-insights/company-news/pick-your-agent-use-claude-and-codex-on-agent-hq/)
- [Meet the GitHub MCP Registry - The GitHub Blog](https://github.blog/ai-and-ml/github-copilot/meet-the-github-mcp-registry-the-fastest-way-to-discover-mcp-servers/)
- [Build an agent into any app with the GitHub Copilot SDK - The GitHub Blog](https://github.blog/news-insights/company-news/build-an-agent-into-any-app-with-the-github-copilot-sdk/)
- [Agent Skills - Simon Willison's Blog](https://simonwillison.net/2025/Dec/19/agent-skills/)
- [Anthropic Opens Agent Skills Standard - Unite.AI](https://www.unite.ai/anthropic-opens-agent-skills-standard-continuing-its-pattern-of-building-industry-infrastructure/)
