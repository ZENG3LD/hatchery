# Claude Code Agent Teams: Implementation Internals & Community Patterns

**Research Date:** 2026-02-06
**Status:** Experimental Feature (requires `CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1`)
**GitHub Source:** https://github.com/anthropics/claude-code

---

## Executive Summary

Claude Code Agent Teams is an experimental multi-agent orchestration system that allows coordination of multiple independent Claude Code instances. One session acts as the **team lead**, spawning **teammates** that work in parallel with their own context windows, communicating via an **inbox-based mailbox system** and coordinating through a **shared task list**.

**Key Discovery:** The feature was initially discovered as a hidden, feature-flagged system embedded in Claude Code binaries before official launch with Opus 4.6.

---

## 1. Core Architecture

### 1.1 System Components

| Component | Role | Storage Location |
|-----------|------|------------------|
| **Team Lead** | Main Claude Code session that creates the team, spawns teammates, coordinates work | Current session |
| **Teammates** | Separate Claude Code instances, each with own context window | Spawned processes (tmux/in-process/iTerm2) |
| **Task List** | Shared work items with DAG-based dependency tracking | `~/.claude/tasks/{team-name}/N.json` |
| **Mailbox** | Asynchronous messaging system for inter-agent communication | `~/.claude/teams/{team-name}/inboxes/{agent}.json` |
| **Team Config** | Member registry, metadata | `~/.claude/teams/{team-name}/config.json` |

### 1.2 File System Structure

```
~/.claude/
├── teams/{team-name}/
│   ├── config.json              # Team members, metadata
│   └── inboxes/{agent-id}.json  # Per-agent message queues
└── tasks/{team-name}/
    └── N.json                   # Individual task files (numbered)
```

**Team Config Schema:**
```json
{
  "members": [
    {
      "name": "string",
      "agent_id": "string",
      "agent_type": "string"
    }
  ]
}
```

**Task File Schema:**
```json
{
  "subject": "string",
  "description": "string",
  "status": "pending|in_progress|completed",
  "owner": "agent_id",
  "blockedBy": ["task_id_1", "task_id_2"],
  "created": "timestamp",
  "updated": "timestamp"
}
```

### 1.3 Teammate Spawn Mechanisms

Three backend modes auto-detect based on environment:

| Mode | Visibility | Persistence | Platform | Performance |
|------|------------|-------------|----------|-------------|
| **in-process** | Invisible (Shift+Up/Down to cycle) | Dies with lead | All | Fastest |
| **tmux** | Separate panes | Survives lead exit | Unix/macOS | Medium |
| **iTerm2** | Split panes | Requires `it2` CLI + Python API | macOS only | Medium |

**Auto-Detection Logic:**
- Default: `"auto"` → Uses tmux if already inside tmux session, else in-process
- Override via `teammateMode` in settings.json or `--teammate-mode` CLI flag

**Spawn Command Example (tmux):**
```bash
tmux new-session -d -s claude-team-{uuid}
tmux send-keys "claude --team-name {name} --agent-id {id} --agent-type {type}" Enter
```

### 1.4 Environment Variables (Teammates Receive)

Auto-injected into teammate processes:

```bash
CLAUDE_CODE_TEAM_NAME={team-name}
CLAUDE_CODE_AGENT_ID={uuid}
CLAUDE_CODE_AGENT_NAME={human-readable-name}
CLAUDE_CODE_AGENT_TYPE={general-purpose|security-sentinel|etc}
CLAUDE_CODE_PLAN_MODE_REQUIRED={true|false}
```

---

## 2. Tool APIs

### 2.1 Teammate Tool (13 Operations)

**Team Lifecycle:**
```typescript
spawnTeam(name: string, config: TeamConfig)
discoverTeams(): Team[]
cleanup()  // ⚠️ Must run from lead, not teammates
```

**Membership:**
```typescript
requestJoin(team_name: string)
approveJoin(agent_id: string)
rejectJoin(agent_id: string, reason: string)
```

**Communication:**
```typescript
write(to: string, message: string)        // Direct message (1-to-1)
broadcast(message: string)                 // Team-wide (expensive: O(n) messages)
```

**Coordination:**
```typescript
approvePlan(agent_id: string)
rejectPlan(agent_id: string, feedback: string)
```

**Shutdown:**
```typescript
requestShutdown(agent_id: string)
approveShutdown()                          // Teammate accepts shutdown
rejectShutdown(reason: string)             // Teammate refuses
```

### 2.2 SendMessage Tool (Structured Messages)

Beyond simple text, supports typed message payloads:

```typescript
type MessageType =
  | "text"
  | "shutdown_request"
  | "shutdown_response"
  | "idle_notification"
  | "task_completed"
  | "plan_approval_request"
  | "join_request"
  | "permission_request"
```

**Message Schema:**
```json
{
  "timestamp": "ISO-8601",
  "from": "agent_id",
  "type": "MessageType",
  "payload": {},
  "read": false
}
```

**Automatic Delivery:**
- Messages delivered to recipients **without polling** (push-based)
- Idle teammates auto-notify lead when stopping
- No explicit polling API needed

### 2.3 Task Management Tools (Team-Aware)

**CRITICAL:** Distinct from `Task` tool (subagents)!

| Tool | Purpose | Availability |
|------|---------|--------------|
| `Task` | Spawn subagents (within single session) | All sessions |
| `TaskCreate` | Create team tasks | Agent Teams only |
| `TaskUpdate` | Modify task status/owner/dependencies | Agent Teams only |
| `TaskList` | Query shared task list | Agent Teams only |
| `TaskGet` | Retrieve task details | Agent Teams only |

**TaskCreate API:**
```typescript
TaskCreate(subject: string, description: string)
// Returns: task_id
```

**TaskUpdate API:**
```typescript
TaskUpdate(
  task_id: string,
  updates: {
    status?: "pending" | "in_progress" | "completed",
    owner?: string,
    addBlockedBy?: string[]  // Add dependency
  }
)
```

**TaskList API:**
```typescript
TaskList(filter?: {
  status?: "pending" | "in_progress" | "completed",
  owner?: string
})
// Returns: Task[]
```

**Automatic Dependency Resolution:**
- When Task A completes, all tasks with `blockedBy: ["A"]` auto-transition to `pending`
- No manual unblocking needed

### 2.4 Task Tool (Subagents) vs Agent Teams

| Feature | Task (Subagents) | Agent Teams (Teammate + TaskCreate) |
|---------|------------------|-------------------------------------|
| **Context** | Own window, results return to caller | Fully independent, persistent |
| **Communication** | Report to main agent only | Teammates message each other |
| **Coordination** | Main agent manages all | Self-coordination via task list |
| **Persistence** | Transcripts saved, resumable | Sessions persist, mailbox survives |
| **Nesting** | Cannot spawn subagents | Cannot spawn nested teams |
| **Best For** | Focused tasks, summary needed | Complex collaboration, parallel work |

---

## 3. Communication & Coordination Patterns

### 3.1 Mailbox System (Inbox-Based)

**Storage:** JSON files per agent at `~/.claude/teams/{team}/inboxes/{agent_id}.json`

**Message Lifecycle:**
1. Sender calls `Teammate.write(to, message)` or `SendMessage`
2. System appends message to `inboxes/{to}.json`
3. Recipient session polls inbox (automatic in Claude Code runtime)
4. Message marked `read: true` after processing

**Polling Mechanism:**
- **Not exposed to LLM** — runtime handles automatically
- Messages appear as system events in teammate context
- No explicit tool call needed to receive

### 3.2 Task Claiming (Race Condition Prevention)

**File Locking:**
```
1. Teammate reads task list
2. Finds pending task with no blockedBy
3. Attempts TaskUpdate(task_id, {status: "in_progress", owner: agent_id})
4. System uses file lock on task JSON
5. First writer wins, others get "already claimed" error
6. Retry loop: go to step 1
```

**Known Issue:** "Teammates sometimes forget to mark tasks as completed, blocking dependent work."

### 3.3 Orchestration Patterns (From Community)

**Pattern 1: Parallel Specialists**
```
Lead → spawns [SecurityReviewer, PerformanceReviewer, TestReviewer]
Each → works independently on same codebase
Each → reports findings via message
Lead → synthesizes results
```

**Pattern 2: Sequential Pipeline**
```
Task A (Research) → creates Task B (Plan) [blockedBy: A]
Task B → creates Task C (Implement) [blockedBy: B]
Task C → creates Task D (Test) [blockedBy: C]
Teammates claim tasks as dependencies resolve
```

**Pattern 3: Self-Organizing Swarm**
```python
while True:
    tasks = TaskList(status="pending", blockedBy=[])
    if not tasks:
        break
    task = tasks[0]
    TaskUpdate(task.id, {status: "in_progress", owner: self.agent_id})
    execute(task)
    TaskUpdate(task.id, {status: "completed"})
```

**Pattern 4: Plan Approval Workflow**
```
1. Teammate spawned with plan_mode_required: true
2. Teammate is read-only, drafts plan
3. Teammate sends plan_approval_request message
4. Lead calls approvePlan(agent_id) or rejectPlan(agent_id, feedback)
5. If approved, teammate exits plan mode, gains write access
6. If rejected, teammate revises plan (stays read-only)
```

---

## 4. Known Limitations & Bugs (2026)

### 4.1 Session Resumption
❌ **In-process teammates NOT restored with `/resume` or `/rewind`**
- After resume, lead may message non-existent teammates
- Workaround: Respawn teammates manually

### 4.2 File Conflicts
❌ **Two teammates editing same file → overwrites**
- File locking only applies to task claiming, not file editing
- **Best Practice:** Assign different file sets per teammate

### 4.3 Task Status Lag
⚠️ **Teammates forget to mark tasks completed**
- Blocks dependent tasks indefinitely
- Manual intervention: `TaskUpdate(task_id, {status: "completed"})`

### 4.4 Shutdown Latency
⚠️ **Graceful shutdown waits for current tool call to complete**
- Can take minutes if teammate is mid-execution
- No force-kill API

### 4.5 Architectural Constraints
- **One team per session** (lead can't manage multiple teams)
- **No nested teams** (teammates can't spawn teammates)
- **Lead is fixed** (can't promote teammate to lead)
- **Permissions set at spawn** (can't change per-teammate modes before spawn)

### 4.6 Context Window Issues (Historical)
- **Issue:** Agents loaded too much data, hit context limit, failed to compact
- **Status:** Improved in recent releases, but auto-compaction can still fail
- **Monitor:** Check `preTokens` in transcript files

### 4.7 Orphaned tmux Sessions
```bash
# Clean up manually
tmux ls
tmux kill-session -t claude-team-{uuid}
```

---

## 5. Community Implementations & Tools

### 5.1 Kieran Klaassen's Swarm Orchestration Skill

**Source:** https://gist.github.com/kieranklaassen/4f2aba89594a4aea4ad64d753984b2ea

**Key Contributions:**
- Comprehensive documentation of all 13 Teammate operations
- 5 orchestration patterns with code examples
- Detailed task dependency workflows
- Built-in agent type catalog (security-sentinel, performance-oracle, etc.)

**Compound Engineering Plugin Agents:**
- 42 Opus-tier agents (critical architecture, security, code review)
- 42 Inherit-tier agents (complex tasks, user-selected model)
- 51 Sonnet-tier agents (support tasks)
- 18 Haiku-tier agents (fast operational tasks)

### 5.2 Claude-Swarm (stevegeek)

**Source:** https://github.com/stevegeek/claude-swarm

**Architecture:**
- YAML config files define team structure
- MCP server-based communication
- Multi-directory support per instance
- Tree-like hierarchy (main instance → connected instances via MCP)

**Key Innovation:** Connected instances communicate through dynamically-generated `mcp__{instance}__task` tools

**Config Example:**
```yaml
version: 1
swarm:
  name: "FullStack Team"
  main: backend
  instances:
    backend:
      description: "Backend API developer"
      directory: ./server
      model: opus
      connections: [frontend, database]
      allowed_tools: [Read, Write, Edit, Bash]
    frontend:
      description: "React developer"
      directory: ./client
      model: sonnet
      connections: [backend]
```

### 5.3 Dream Team (drbscl)

**Source:** https://github.com/drbscl/dream-team

**5-Phase Pipeline:**
1. **Scope** → Analyze project structure, tech stack
2. **Team-Plan** → Map requirements to capabilities
3. **Assemble** → Discover plugins/agents/skills from registries
4. **Train** → Auto-generate custom agents for gaps
5. **Execute** → Orchestrate team

**Multi-Registry Sourcing:**
- Official Claude Code plugin marketplace
- VoltAgent/awesome-claude-code-subagents (GitHub)
- skills.sh registry

**Security Gates:**
- Assembly Review (validate external resources)
- Training Review (approve auto-generated agents)
- Execute Review (confirm team composition)

**Warning:** "DO NOT run with `--dangerously-skip-permissions` during assembly!" (risk of prompt injection)

### 5.4 Claude-Flow (ruvnet)

**Source:** https://github.com/ruvnet/claude-flow

**Features:**
- Enterprise-grade agent orchestration
- Distributed swarm intelligence
- RAG integration
- Native MCP protocol support
- 16 multi-agent workflow orchestrators

**Agent Coordination Examples:**
- Full-stack feature: backend-architect → database-architect → frontend-developer → test-automator → security-auditor
- Progressive disclosure architecture (3-tier knowledge loading)

### 5.5 MCP Agent Mail (Dicklesworthstone)

**Source:** https://github.com/Dicklesworthstone/mcp_agent_mail

**Purpose:** Gmail-like interface for agent communication
- Alternative mailbox implementation
- MCP server for inter-agent messaging
- Not Claude Code native, but compatible

---

## 6. Hooks & MCP Integration

### 6.1 Hook Events (Subagents, NOT Agent Teams)

**PreToolUse / PostToolUse:**
- Designed for **subagents** (single session, Task tool)
- NOT for **agent teams** (multiple sessions, Teammate tool)
- Can't intercept Teammate communication via hooks

**Known Issues:**
- `PreToolUse` approval blocking doesn't work (`approve: false` ignored)
- Hooks not triggering on WSL2 in some cases
- PostToolUse cannot modify tool output

**Best Use Case for Teams:** Validate Bash commands per teammate

```yaml
# In teammate agent definition
hooks:
  PreToolUse:
    - matcher: "Bash"
      hooks:
        - type: command
          command: "./scripts/validate-teammate-command.sh"
```

### 6.2 MCP Servers for Agent Coordination

**Claude Code as MCP Server:**
- `claude mcp serve` exposes file editing/command tools
- Other MCP clients (Claude Desktop, Cursor) can invoke Claude Code remotely
- NOT for inter-teammate communication (that's mailbox-based)

**Agent Coordination via MCP:**
- MCP servers give subagents external tool access
- Agent teams don't expose teammates as MCP servers
- No known MCP server specifically for agent team orchestration (yet)

**Integration Pattern:**
- Subagents can use MCP tools (if inherited from main session)
- Background subagents CANNOT use MCP tools (permission pre-approval limitation)
- Agent teams: each teammate loads MCP servers independently

---

## 7. Subagents vs Agent Teams (Deep Dive)

### 7.1 Task Tool (Subagents)

**Invocation:**
```typescript
Task(
  agent_type: "general-purpose" | "Explore" | "Plan" | "Bash" | custom,
  prompt: string,
  background?: boolean,
  thoroughness?: "quick" | "medium" | "very thorough"  // Explore only
)
```

**Built-in Types:**
- `Explore` (Haiku, read-only, codebase search)
- `Plan` (Inherit model, read-only, planning research)
- `general-purpose` (Inherit model, all tools, complex tasks)
- `Bash` (Inherit, separate context for commands)
- `statusline-setup` (Sonnet, /statusline config)
- `claude-code-guide` (Haiku, feature questions)

**Foreground vs Background:**
- **Foreground:** Blocks main session, permission prompts passed through, can ask clarifying questions
- **Background:** Concurrent, pre-approves permissions, auto-denies unknowns, NO MCP tools, NO AskUserQuestion

**Context Management:**
- Transcripts: `~/.claude/projects/{project}/{sessionId}/subagents/agent-{agentId}.jsonl`
- Resumption: Ask Claude to resume agent ID
- Auto-compaction: Triggers at ~95% capacity (override: `CLAUDE_AUTOCOMPACT_PCT_OVERRIDE`)

**Limitations:**
- **Cannot spawn subagents** (no nesting)
- Results return to caller → can inflate main context
- No inter-subagent communication

### 7.2 Agent Teams (Teammate Tool)

**Invocation:**
```typescript
// Lead spawns teammate
Teammate.spawnTeam(team_name, config)
// Then spawns individual teammates via natural language delegation
// (No direct spawn API in docs — orchestrated by lead)
```

**Communication:**
- Teammates message each other directly
- Shared task list enables self-coordination
- Mailbox survives session restarts (in ~/.claude/teams/)

**Context Management:**
- Each teammate: separate Claude Code session
- Context windows: independent, NOT shared
- Only summaries/findings exchanged via messages

**Token Cost:**
- **Significantly higher** than subagents
- Each teammate: full Opus/Sonnet context window
- Broadcast: O(n) messages

---

## 8. Best Practices (Synthesized from Docs + Community)

### 8.1 When to Use Agent Teams

✅ **Good Use Cases:**
- Research & review (multiple perspectives simultaneously)
- New modules/features (each teammate owns different files)
- Debugging with competing hypotheses
- Cross-layer coordination (frontend + backend + infra)

❌ **Poor Use Cases:**
- Sequential tasks with dependencies (use single session or subagents)
- Same-file edits (file conflicts)
- Simple tasks (overhead exceeds benefit)

### 8.2 Task Sizing

- **Too small:** Coordination overhead > value
- **Too large:** Long work without check-ins, risk of wasted effort
- **Just right:** Self-contained, clear deliverable (function, test file, review)
- **Rule of thumb:** 5-6 tasks per teammate

### 8.3 Prevent File Conflicts

**Strategy 1: File Ownership**
```
Teammate A: src/auth/*.rs
Teammate B: src/api/*.rs
Teammate C: tests/
```

**Strategy 2: Layer Separation**
```
Teammate A: Database schema + migrations
Teammate B: API endpoints
Teammate C: Frontend components
```

**Strategy 3: Explicit Task Constraints**
```
Task: "Implement JWT middleware in src/auth/jwt.rs. DO NOT modify other files."
```

### 8.4 Monitor & Steer

- Check task list progress: `Ctrl+T` (or ask lead "show task status")
- Redirect stuck teammates: Message directly with new approach
- Synthesize findings early: Don't let all teammates finish before review

### 8.5 Context Management

**Giving Context to Teammates:**
- Teammates load CLAUDE.md, MCP servers, skills automatically
- Include task-specific details in spawn prompt
- Use skills field to preload domain knowledge

**Preventing Context Bloat:**
- Have teammates return summaries, not full output
- Use background subagents for verbose operations
- Set auto-compact threshold lower if needed

### 8.6 Permission Management

- Pre-approve common operations in settings.json before spawning
- All teammates inherit lead's permission mode
- `bypassPermissions` at lead level applies to all teammates (can't override)

### 8.7 Graceful Shutdown

```
1. Ask lead: "Ask teammates to shut down"
2. Lead sends shutdown_request to each
3. Teammates approve/reject
4. Wait for all approvals
5. Lead runs cleanup()
```

**Warning:** Running cleanup with active teammates fails. Always shut down first.

---

## 9. Token Economics

### 9.1 Cost Scaling

| Scenario | Tokens (Approximate) | Model |
|----------|---------------------|-------|
| Single session (1 hour coding) | 50k-150k | Opus/Sonnet |
| 3-teammate team (parallel research) | 150k-450k | 3x independent contexts |
| Broadcast to 5 teammates | 5x message cost | Linear scaling |

### 9.2 Cost Optimization

**Use Haiku for teammates when possible:**
```yaml
---
name: fast-researcher
model: haiku
---
```

**Background subagents instead of teams for isolated tasks:**
- Subagent results summarized back to main context
- Agent teams: each teammate is full session

**Minimize broadcasts:**
- Prefer targeted `write(to, message)` over `broadcast()`
- Use task list for status updates instead of messages

---

## 10. Open Questions & Future Research

### 10.1 Unanswered from GitHub Source Search

❓ **Source code location:** Repository is closed-source; only binary releases available
❓ **Actual spawn implementation:** How does in-process mode work? (Likely Node.js worker threads)
❓ **Mailbox polling interval:** How often do teammates check inboxes?
❓ **File locking mechanism:** POSIX flock? Windows LockFileEx? JSON file-level locks?
❓ **Task ID generation:** Sequential integers per team?

### 10.2 Feature Requests from Community

- **Nested teams:** Teammates spawning sub-teams (currently forbidden)
- **Session resumption for in-process:** Currently only tmux survives restarts
- **Dynamic permission changes:** Set per-teammate permissions before spawn
- **Multi-team leads:** One lead managing multiple teams
- **Leader transfer:** Promote teammate to lead mid-session

### 10.3 Research Opportunities

- **Performance benchmarking:** Compare in-process vs tmux overhead
- **Race condition analysis:** Can multiple teammates safely edit different files in same directory?
- **Cost-benefit thresholds:** At what team size does coordination overhead dominate?
- **Alternative mailbox backends:** Could Redis/PostgreSQL replace JSON files for large teams?

---

## 11. Code Examples

### 11.1 Lead Orchestrating a 3-Agent Research Team

**User Prompt:**
```
Create an agent team to research authentication options for our API.
Spawn 3 teammates:
- OAuth specialist (research OAuth 2.0 + OIDC)
- JWT specialist (research JWT best practices)
- API key specialist (research API key management)

Have them each report findings in a markdown file.
```

**Expected Lead Actions:**
```typescript
// Step 1: Create team
Teammate.spawnTeam("auth-research", {
  members: [
    {name: "oauth-expert", type: "general-purpose"},
    {name: "jwt-expert", type: "general-purpose"},
    {name: "apikey-expert", type: "general-purpose"}
  ]
})

// Step 2: Create tasks
TaskCreate("Research OAuth 2.0 + OIDC", "Investigate OAuth flows, security considerations, library recommendations. Write findings to research/oauth.md")
TaskCreate("Research JWT best practices", "Cover signing algorithms, expiration, refresh tokens, storage. Write to research/jwt.md")
TaskCreate("Research API key management", "Rotation, scoping, rate limiting. Write to research/api-keys.md")

// Step 3: Spawn teammates (implicit via natural language)
// Step 4: Teammates self-claim tasks via TaskList + TaskUpdate
// Step 5: Teammates complete work, send idle_notification
// Step 6: Lead synthesizes findings
```

### 11.2 Sequential Pipeline with Dependencies

**User Prompt:**
```
Implement a new user registration endpoint.
Use a team with:
1. Database designer (schema + migration)
2. Backend developer (API endpoint, depends on schema)
3. Test writer (integration tests, depends on endpoint)
```

**Lead Actions:**
```typescript
Teammate.spawnTeam("registration-feature", {})

// Create tasks with dependencies
const schemaTask = TaskCreate(
  "Design user registration schema",
  "Create migration for users table with email, password_hash, created_at. File: migrations/003_users.sql"
)

const endpointTask = TaskCreate(
  "Implement /api/register endpoint",
  "POST handler with validation, password hashing, database insert. File: src/api/register.rs"
)
TaskUpdate(endpointTask, {addBlockedBy: [schemaTask]})  // Depends on schema

const testTask = TaskCreate(
  "Write integration tests",
  "Test successful registration, duplicate email, invalid input. File: tests/register_test.rs"
)
TaskUpdate(testTask, {addBlockedBy: [endpointTask]})  // Depends on endpoint

// Teammates claim as dependencies resolve
```

### 11.3 Plan Approval Workflow

**User Prompt:**
```
Spawn an architect teammate to refactor the authentication module.
Require plan approval before implementation.
```

**Lead Actions:**
```typescript
Teammate.spawnTeam("auth-refactor", {})
// Spawn teammate with plan_mode_required: true (via natural language)
// Teammate loads in read-only mode
```

**Teammate (in plan mode):**
```typescript
// Reads codebase
Read("src/auth/")
Grep("authentication", glob: "**/*.rs")

// Drafts plan
Write("refactor-plan.md", "## Proposal: Extract auth logic into separate crate...")

// Sends approval request
SendMessage({
  type: "plan_approval_request",
  payload: {plan: "See refactor-plan.md"}
})
```

**Lead:**
```typescript
// Reads plan
Read("refactor-plan.md")

// Decision
Teammate.approvePlan("architect-agent-id")
// OR
Teammate.rejectPlan("architect-agent-id", "Consider backward compatibility for existing tokens")
```

**Teammate (after approval):**
```typescript
// Exits plan mode, gains write access
Edit("src/auth/mod.rs", {...})
```

---

## 12. Comparison Matrix

### 12.1 Agent Coordination Approaches

| Feature | Subagents (Task) | Agent Teams (Teammate) | MCP Servers | Hooks |
|---------|------------------|------------------------|-------------|-------|
| **Cross-session** | No | Yes | Yes | No |
| **Inter-agent messaging** | No | Yes | Via tools | No |
| **Shared state** | Parent context | Task list + mailbox | External DB | Process memory |
| **Nesting** | No | No | N/A | N/A |
| **Resumption** | Yes (via agent ID) | Partial (tmux only) | N/A | N/A |
| **Token cost** | Medium (results summarized) | High (independent contexts) | Low (tool calls) | Low (script execution) |
| **Best for** | Focused tasks | Parallel collaboration | External integrations | Validation/hooks |

### 12.2 Community Tool Comparison

| Tool | Approach | Communication | Config | Platform |
|------|----------|---------------|--------|----------|
| **Native Agent Teams** | Built-in Teammate tool | Mailbox (JSON files) | Natural language | All |
| **claude-swarm** | MCP server tree | MCP tool calls | YAML | Unix/macOS |
| **Dream Team** | Plugin orchestration | Native tools | Interactive CLI | All |
| **claude-flow** | Enterprise framework | MCP + RAG | JSON/YAML | All |

---

## 13. References & Sources

### Official Documentation
- [Claude Code Agent Teams Docs](https://code.claude.com/docs/en/agent-teams)
- [Claude Code Subagents Docs](https://code.claude.com/docs/en/sub-agents)
- [Claude Code GitHub](https://github.com/anthropics/claude-code)

### Community Resources
- [Kieran Klaassen: Swarm Orchestration Skill](https://gist.github.com/kieranklaassen/4f2aba89594a4aea4ad64d753984b2ea)
- [Kieran Klaassen: Multi-Agent Orchestration System](https://gist.github.com/kieranklaassen/d2b35569be2c7f1412c64861a219d51f)
- [Claude Code Hidden Swarm (Paddo.dev)](https://paddo.dev/blog/claude-code-hidden-swarm/)
- [Support for Agent Teams (obra/superpowers)](https://github.com/obra/superpowers/issues/429)

### Implementation Repositories
- [stevegeek/claude-swarm](https://github.com/stevegeek/claude-swarm)
- [wshobson/agents](https://github.com/wshobson/agents)
- [drbscl/dream-team](https://github.com/drbscl/dream-team)
- [ruvnet/claude-flow](https://github.com/ruvnet/claude-flow)
- [Dicklesworthstone/mcp_agent_mail](https://github.com/Dicklesworthstone/mcp_agent_mail)

### Articles & Guides
- [Claude Code's New Hidden Feature: Swarms (Hacker News)](https://news.ycombinator.com/item?id=46743908)
- [AddyOsmani.com: Claude Code Swarms](https://addyosmani.com/blog/claude-code-agent-teams/)
- [ClaudeFa.st: Agent Teams Guide](https://claudefa.st/blog/guide/agents/agent-teams)
- [NxCode: Agent Teams Tutorial 2026](https://www.nxcode.io/resources/news/claude-agent-teams-parallel-ai-development-guide-2026)

### Issue Trackers
- [Bug: Agent Teams Crashes](https://github.com/anthropics/claude-code/issues/23435)
- [PreToolUse Hooks Not Blocking](https://github.com/anthropics/claude-code/issues/4362)
- [Feature: PostToolUse Modify Output](https://github.com/anthropics/claude-code/issues/18594)

---

## Appendix A: Agent Types (Built-in + Plugin)

### Built-in Types
- `general-purpose` (Inherit, all tools)
- `Explore` (Haiku, read-only)
- `Plan` (Inherit, read-only)
- `Bash` (Inherit, command execution)
- `statusline-setup` (Sonnet, TUI config)
- `claude-code-guide` (Haiku, help)

### Plugin Types (Compound Engineering)
**Tier 1: Opus 4.5 (Critical)**
- `security-sentinel`, `architecture-sage`, `code-reviewer-lead`, `performance-oracle`, `database-architect`, `api-designer`, etc.

**Tier 2: Inherit (Complex)**
- `auth-specialist`, `payment-integrator`, `notification-engineer`, etc.

**Tier 3: Sonnet 4.5 (Support)**
- `test-automator`, `doc-writer`, `config-manager`, etc.

**Tier 4: Haiku 4.5 (Fast Ops)**
- `log-parser`, `dependency-updater`, `format-checker`, etc.

(Full list: 153 total agents across all tiers)

---

## Appendix B: Troubleshooting Checklist

### Teammates Not Appearing
- [ ] Check `CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1` in settings
- [ ] Press Shift+Down to cycle through in-process teammates
- [ ] Verify task complexity (simple tasks may not trigger spawning)
- [ ] For tmux: `which tmux` confirms installation
- [ ] For iTerm2: Verify `it2` CLI + Python API enabled

### File Conflicts
- [ ] Review task assignments: each teammate owns different files?
- [ ] Check git status for unexpected merges
- [ ] Use `git diff --name-only` to see who modified what

### Task Status Issues
- [ ] Manually check task files: `cat ~/.claude/tasks/{team}/N.json`
- [ ] Update stuck tasks: Tell lead "Mark task N as completed"
- [ ] Check `blockedBy` arrays for resolved dependencies

### Performance Issues
- [ ] Check context window usage in transcripts (look for `preTokens`)
- [ ] Reduce teammate count if coordination overhead is high
- [ ] Switch high-volume teammates to Haiku model

### Orphaned Resources
- [ ] List teams: `ls ~/.claude/teams/`
- [ ] List tasks: `ls ~/.claude/tasks/`
- [ ] List tmux sessions: `tmux ls`
- [ ] Clean up manually: `rm -rf ~/.claude/teams/{team}` (after shutdown)

---

**End of Report**
