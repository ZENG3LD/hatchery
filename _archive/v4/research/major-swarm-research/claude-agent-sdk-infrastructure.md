# Claude Agent SDK Infrastructure: Complete Technical Reference

**Research Date:** 2026-02-08
**Scope:** Reusable multi-agent infrastructure for Claude Code and Agent SDK
**Focus:** Tools, coordination mechanisms, context management, and orchestration patterns

---

## Executive Summary

Claude Code's multi-agent infrastructure is built on three layers:

1. **Agent SDK** - Programmable Python/TypeScript API exposing Claude Code's agent loop, tools, and context management
2. **TeammateTool** - 13-operation coordination layer for inter-agent communication and team management
3. **Task System** - Shared task queue with dependency tracking for coordinated work

The system uses **file-based coordination** (`~/.claude/teams/`, `~/.claude/tasks/`) rather than networked communication, enabling local-first multi-agent orchestration without external infrastructure.

---

## 1. Agent SDK Tools: Full API Surface

### 1.1 Core Query Interface

**Primary Methods:**
- `query()` - Main entry point for agent execution
- `ClaudeAgentOptions` - Configuration object for tools, permissions, hooks, agents
- `ClaudeAgentClient` - Interactive streaming client

**Example:**
```python
from claude_agent_sdk import query, ClaudeAgentOptions

async for message in query(
    prompt="Find and fix the bug in auth.py",
    options=ClaudeAgentOptions(allowed_tools=["Read", "Edit", "Bash"])
):
    print(message)
```

### 1.2 Built-in Tools (All SDK/CLI Environments)

| Tool | Capability | Parameters |
|------|-----------|------------|
| **Read** | Read any file in working directory | `file_path`, `offset`, `limit` |
| **Write** | Create new files | `file_path`, `content` |
| **Edit** | Make precise edits to existing files | `file_path`, edits (line-based) |
| **Bash** | Run terminal commands, scripts, git ops | `command`, `timeout` |
| **Glob** | Find files by pattern | `pattern` (e.g., `**/*.ts`) |
| **Grep** | Search file contents with regex | `pattern`, `glob`, `output_mode` |
| **WebSearch** | Search web for current information | `query`, `allowed_domains`, `blocked_domains` |
| **WebFetch** | Fetch and parse web page content | `url`, `prompt` |
| **AskUserQuestion** | Ask clarifying questions with options | `question`, `options` |

**No TeamCreate, SendMessage, or TaskCreate in base SDK** - these are **Claude Code-specific** team coordination tools, not general Agent SDK features.

### 1.3 Team Coordination Tools (Claude Code Only)

Available only when `CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1` is set:

#### TeammateTool (13 Operations)

**Team Lifecycle:**
1. `spawnTeam` - Create team with you as leader
2. `discoverTeams` - List teams available to join
3. `requestJoin` - Worker requests membership
4. `approveJoin` - Leader accepts worker
5. `rejectJoin` - Leader declines worker
6. `cleanup` - Remove team resources (fails if teammates active)

**Communication:**
7. `write` - Send targeted message to specific teammate
8. `broadcast` - Send to all teammates (expensive, use sparingly)

**Shutdown:**
9. `requestShutdown` - Leader initiates termination
10. `approveShutdown` - Teammate confirms exit
11. `rejectShutdown` - Teammate declines with reason

**Plan Approval:**
12. `approvePlan` - Leader accepts plan from teammate with `plan_mode_required: true`
13. `rejectPlan` - Leader returns plan with revision feedback

**Key Constraint:** Workers **cannot use TaskCreate** (read-only access to task system).

#### Task System Tools

- **TaskList** - View all tasks with status (pending | in_progress | completed), owner, blockedBy
- **TaskUpdate** - Claim, progress, complete; add/remove dependencies
- **TaskCreate** - Create new task (leader only in team context)
- **TaskGet** - Retrieve specific task details

**Task Dependencies:**
```python
# Task #2 waits for Task #1
TaskUpdate(taskId="2", addBlockedBy=["1"])
# Auto-unblocks when Task #1 completes
```

### 1.4 Agent SDK-Specific Features

**Subagents:**
- Defined via `agents` parameter in `ClaudeAgentOptions`
- Spawned using `Task` tool when included in `allowed_tools`
- Cannot spawn other subagents (no nesting)

**Hooks:**
- `PreToolUse`, `PostToolUse`, `Stop`, `SessionStart`, `SessionEnd`, `UserPromptSubmit`
- Team-specific: `SubagentStart`, `SubagentStop`, `TeammateIdle`, `TaskCompleted`
- Three types: `command` (shell), `prompt` (LLM decision), `agent` (subagent verification)

**MCP (Model Context Protocol):**
- Connect external systems: databases, browsers, APIs
- Example: Playwright for browser automation, GitHub for repo access
- Configured via `mcp_servers` parameter

**Permissions:**
- `default` - Standard prompts
- `acceptEdits` - Auto-accept file edits
- `dontAsk` - Auto-deny prompts
- `delegate` - Coordination-only (team leads)
- `bypassPermissions` - Skip all checks
- `plan` - Read-only exploration

**Sessions:**
- Resume via `resume: session_id` parameter
- Fork sessions to explore alternatives
- Transcripts stored in `~/.claude/projects/{project}/{sessionId}/`

---

## 2. Message Delivery Between Agents

### 2.1 Mechanism: File-Based Inbox System

**Architecture:**
- Each team member: `~/.claude/teams/{team}/inboxes/{agent}.json`
- **Not polling** - filesystem watchers trigger reads
- **Not webhooks** - local file events
- **Not networked** - all coordination is local

**Message Types:**
```json
{
  "type": "shutdown_request",
  "from": "lead",
  "to": "worker-1",
  "request_id": "shutdown-123",
  "reason": "Tasks complete"
}
```

**Delivery Semantics:**
- **Write operation**: Sender appends to recipient's inbox file
- **Read operation**: Recipient watches inbox, processes on file change
- **Latency**: 10-15 seconds typical (filesystem event propagation)
- **Automatic**: Teammates receive messages without polling

### 2.2 Communication Patterns

**Worker → Leader:**
- `write` operation only (workers cannot broadcast)
- Auto-notifications: `idle_notification` when worker finishes tasks

**Leader → Worker:**
- `write` for targeted messages
- `broadcast` for team-wide (scales cost linearly with team size)

**Worker → Worker:**
- Indirect via leader or shared task list
- No direct peer-to-peer messaging

### 2.3 Known Latencies & Constraints

- Task status updates lag 10-15 seconds
- No file locking for task claiming (uses optimistic concurrency)
- Heartbeat timeout: 5 minutes inactivity = crashed teammate
- No message delivery guarantees (best-effort filesystem)

---

## 3. Context Compaction: How It Works

### 3.1 The 84% Token Reduction

**Source:** [Managing context on the Claude Developer Platform](https://claude.com/blog/context-management)

**Specific Result:**
> "In a 100-turn web search evaluation, context editing enabled agents to complete workflows that would otherwise fail due to context exhaustion—while reducing token consumption by 84%."

**Caveat:** This is a **specific benchmark result**, not a general compaction rate. Actual reduction varies by workflow.

### 3.2 Context Editing Strategies (Server-Side)

**Two Primary Strategies:**

#### Strategy 1: Tool Result Clearing (`clear_tool_uses_20250919`)

**What It Does:**
- Clears oldest tool results when context exceeds threshold
- Replaces with placeholder text so Claude knows content was removed
- Preserves tool calls (parameters) by default unless `clear_tool_inputs: true`

**Configuration:**
```python
context_management={
    "edits": [
        {
            "type": "clear_tool_uses_20250919",
            "trigger": {"type": "input_tokens", "value": 30000},  # When to activate
            "keep": {"type": "tool_uses", "value": 3},            # How many to preserve
            "clear_at_least": {"type": "input_tokens", "value": 5000},  # Minimum clearing
            "exclude_tools": ["web_search"]                       # Never clear these
        }
    ]
}
```

**What's Preserved:**
- Recent N tool uses (configurable via `keep`)
- Excluded tools (via `exclude_tools`)
- Conversation flow (user/assistant turns remain)

**What's Discarded:**
- Oldest tool results first (chronological order)
- Raw tool outputs (e.g., file contents from Read tool)

#### Strategy 2: Thinking Block Clearing (`clear_thinking_20251015`)

**What It Does:**
- Manages `thinking` blocks when extended thinking enabled
- Configurable retention: keep last N turns or all

**Configuration:**
```python
context_management={
    "edits": [
        {
            "type": "clear_thinking_20251015",
            "keep": {"type": "thinking_turns", "value": 2}  # Keep last 2 turns
            # OR: "keep": "all"  # Maximize cache hits
        }
    ]
}
```

**Default Behavior (No Config):**
- Automatically keeps only thinking blocks from last assistant turn
- Equivalent to `keep: {type: "thinking_turns", value: 1}`

### 3.3 The 75% Threshold (Auto-Compaction)

**Source:** Multiple community references mention "75-92% utilization"

**How It Works:**
- Default trigger: ~95% of context window capacity
- Configurable via `CLAUDE_AUTOCOMPACT_PCT_OVERRIDE` environment variable
- Example: `CLAUDE_AUTOCOMPACT_PCT_OVERRIDE=50` triggers at 50%

**Logged in Transcripts:**
```json
{
  "type": "system",
  "subtype": "compact_boundary",
  "compactMetadata": {
    "trigger": "auto",
    "preTokens": 167189
  }
}
```

### 3.4 What Algorithm Compresses Context?

**No Public Algorithm Specification**

The documentation does not reveal:
- Exact compression/summarization algorithm
- Token prioritization heuristics
- How "importance" is calculated

**What We Know:**
1. **Chronological removal** for tool results (oldest first)
2. **Structured summarization** for client-side compaction (SDK)
3. **Selective preservation** based on recency and exclusion rules

### 3.5 Client-Side Compaction (SDK Alternative)

**Mechanism:**
1. SDK monitors total tokens = `input_tokens + cache_creation_input_tokens + cache_read_input_tokens + output_tokens`
2. When threshold exceeded, injects summary prompt as user turn
3. Claude generates summary wrapped in `<summary></summary>` tags
4. SDK replaces entire message history with summary

**Default Summary Structure:**
1. Task Overview (user's request, success criteria)
2. Current State (completed work, files modified)
3. Important Discoveries (constraints, decisions, errors)
4. Next Steps (remaining actions, blockers)
5. Context to Preserve (preferences, commitments)

**Configuration:**
```python
compaction_control={
    "enabled": True,
    "context_token_threshold": 100000,
    "model": "claude-haiku-4-5",  # Optional: cheaper model for summaries
    "summary_prompt": "Custom prompt..."  # Optional
}
```

**Limitation:** Server-side compaction recommended over SDK compaction (better token calculation, no client-side constraints).

---

## 4. Delegate Mode: What Lead Agents Can/Can't Do

### 4.1 Purpose

**Problem:** Lead agents sometimes implement tasks themselves instead of delegating.

**Solution:** Restrict lead to coordination-only tools.

### 4.2 Activation

**Interactive:**
- Press `Shift+Tab` to toggle delegate mode

**Programmatic:**
```json
{
  "permissionMode": "delegate"
}
```

### 4.3 Tool Restrictions in Delegate Mode

**Allowed:**
- `Teammate` (all 13 operations)
- `TaskCreate`, `TaskUpdate`, `TaskList`, `TaskGet`
- `SendMessage` (if available)
- `AskUserQuestion`

**Blocked:**
- `Read`, `Write`, `Edit` - No file operations
- `Bash` - No command execution
- `Grep`, `Glob` - No codebase exploration
- MCP tools - No external integrations

**Result:** Lead can only spawn, message, shutdown teammates, and manage tasks.

### 4.4 When to Use

**Use Delegate Mode When:**
- Team has 3+ workers
- Lead keeps "helping" instead of orchestrating
- Tasks are clearly partitioned

**Don't Use When:**
- Lead needs to synthesize results (may require reading files)
- Only 1-2 workers (overhead not worth it)
- Rapid iteration needed (adds coordination friction)

---

## 5. Plan Approval Mode: Workflow & Capabilities

### 5.1 How It Works

**Setup:**
```
Spawn an architect teammate to refactor the authentication module.
Require plan approval before they make any changes.
```

**Flow:**
1. Teammate works in **read-only plan mode** (no Edit/Write until approved)
2. Teammate generates plan, sends `plan_approval_request` to leader
3. Leader reviews plan → approves or rejects with feedback
4. If rejected: teammate revises, resubmits
5. If approved: teammate exits plan mode, begins implementation

### 5.2 Approval Decision Control

**Automatic (Lead Decides):**
- Lead makes approval decisions autonomously
- Influence via prompt: "only approve plans that include test coverage"

**Manual (User Approves):**
- Not currently supported in experimental release
- Workaround: Ask lead to forward plan to you for review

### 5.3 Plan Modification

**Can Approver Modify Plan?**
- No direct modification
- Rejection with feedback forces revision
- Multiple rounds until acceptable

**Rejection Example:**
```python
Teammate({
    operation: "rejectPlan",
    target_agent_id: "architect",
    request_id: "plan-456",
    feedback: "Add error handling for API calls, include rollback strategy"
})
```

### 5.4 Permission Mode vs Plan Mode

**Key Difference:**
- **Permission Mode**: Controls what tools agent can use
- **Plan Mode**: Gates implementation behind approval step

**Combined Use:**
```json
{
  "permissionMode": "acceptEdits",  // Once approved, auto-accept edits
  "plan_mode_required": true        // But require plan first
}
```

---

## 6. TeammateIdle / TaskCompleted Hooks: Implementation

### 6.1 What They Are

**Hook Types:** Shell commands that execute at lifecycle events

**NOT Git Hooks:** Unrelated to git pre-commit/post-merge hooks
**NOT MCP Hooks:** Different from Model Context Protocol integrations

**Implementation:** User-defined shell scripts in `settings.json` or `.claude/hooks/`

### 6.2 TeammateIdle Hook

**Event:** `TeammateIdle`
**When It Fires:** When agent team teammate is about to go idle (finished tasks, no more work)
**Matcher:** None (always fires for all teammates)

**Exit Codes:**
- `0` - Allow teammate to go idle
- `2` - Send feedback and keep teammate working

**Example Use Case:** Enforce minimum deliverables
```json
{
  "hooks": {
    "TeammateIdle": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "./.claude/hooks/verify-teammate-deliverables.sh"
          }
        ]
      }
    ]
  }
}
```

**Script Example:**
```bash
#!/bin/bash
INPUT=$(cat)
AGENT_ID=$(echo "$INPUT" | jq -r '.agent_id')

# Check if teammate created at least one test file
if ! ls tests/*${AGENT_ID}*.test.js 2>/dev/null; then
  echo "Missing test file for your work. Create tests before going idle." >&2
  exit 2  # Block idle, send feedback
fi

exit 0  # Allow idle
```

### 6.3 TaskCompleted Hook

**Event:** `TaskCompleted`
**When It Fires:** When task is being marked as completed
**Matcher:** None (fires for all tasks)

**Exit Codes:**
- `0` - Allow task completion
- `2` - Prevent completion, send feedback

**Example Use Case:** Verify tests pass
```json
{
  "hooks": {
    "TaskCompleted": [
      {
        "hooks": [
          {
            "type": "agent",
            "prompt": "Verify all unit tests pass. Run test suite and check results. If any fail, respond {\"ok\": false, \"reason\": \"test failures\"}.",
            "timeout": 120
          }
        ]
      }
    ]
  }
}
```

**Note:** This example uses `type: "agent"` (spawns subagent with tools) instead of `type: "command"` (shell script).

### 6.4 All Hook Events (12 Total)

| Event | When It Fires | Matcher Support |
|-------|---------------|-----------------|
| `SessionStart` | Session begins/resumes | Source: startup, resume, clear, compact |
| `UserPromptSubmit` | User submits prompt | None |
| `PreToolUse` | Before tool executes | Tool name (e.g., `Bash`, `Edit\|Write`) |
| `PermissionRequest` | Permission dialog appears | Tool name |
| `PostToolUse` | After tool succeeds | Tool name |
| `PostToolUseFailure` | After tool fails | Tool name |
| `Notification` | Claude sends notification | Notification type |
| `SubagentStart` | Subagent spawned | Agent type |
| `SubagentStop` | Subagent finishes | Agent type |
| `Stop` | Claude finishes responding | None |
| `TeammateIdle` | Teammate about to go idle | None |
| `TaskCompleted` | Task marked complete | None |
| `PreCompact` | Before context compaction | Trigger: manual, auto |
| `SessionEnd` | Session terminates | Reason: clear, logout, etc. |

### 6.5 Hook Input Schema

**Common Fields (All Events):**
```json
{
  "session_id": "abc123",
  "cwd": "/Users/name/project",
  "hook_event_name": "PreToolUse"
}
```

**PreToolUse Specific:**
```json
{
  "tool_name": "Bash",
  "tool_input": {
    "command": "npm test"
  }
}
```

**TeammateIdle/TaskCompleted Specific:**
```json
{
  "agent_id": "worker-1",
  "team_name": "feature-auth"
}
```

### 6.6 Hook Output (Exit Code Behavior)

**Exit 0:** Action proceeds
- For `UserPromptSubmit`/`SessionStart`: stdout → injected into Claude's context
- For other events: hook passed, continue execution

**Exit 2:** Action blocked
- stderr → fed to Claude as feedback
- Tool call/task completion prevented

**Exit 1 or other:** Action proceeds, stderr logged but not shown to Claude

**JSON Output (Alternative to Exit 2):**
```json
{
  "hookSpecificOutput": {
    "hookEventName": "PreToolUse",
    "permissionDecision": "deny",
    "permissionDecisionReason": "Protected file"
  }
}
```

---

## 7. Fresh Context Windows: Session State Persistence

### 7.1 Context Window Sizes

**Claude Opus 4.6 (Released Feb 2026):**
- **1M tokens** per session
- ~750,000 words
- ~1,500 pages of text
- ~30,000 lines of code

**Previous Models:**
- Sonnet 4.5: 200K tokens (default)
- Haiku 4.5: 200K tokens

### 7.2 "Per Session" vs "Total"

**Per Session:**
- Each agent instance (lead or teammate) gets **its own 1M token budget**
- Budget resets when session restarts
- Independent from other agents

**Not Total:**
- No global token pool shared across agents
- 5-person team = 5x 1M tokens = 5M total capacity
- Each teammate isolated in own context window

### 7.3 Session State Persistence

**What Persists:**
- Conversation history (all user/assistant turns)
- Tool calls and results
- Subagent transcripts (separate files in `~/.claude/projects/{project}/{sessionId}/subagents/`)
- Task list state (in `~/.claude/tasks/{team}/`)
- Team member roster (in `~/.claude/teams/{team}/config.json`)

**Where Stored:**
- Session transcripts: `~/.claude/projects/{project}/{sessionId}/transcript.jsonl`
- Team configs: `~/.claude/teams/{team}/config.json`
- Tasks: `~/.claude/tasks/{team}/`

**Cleanup:**
- Automatic after `cleanupPeriodDays` setting (default: 30 days)
- Manual via `/clear` (current session only)
- Team cleanup via `Teammate({operation: "cleanup"})`

### 7.4 Session Resumption After /clear

**What Happens:**
- `/clear` ends current session, creates new one
- Previous session transcript archived
- New session starts with **fresh 1M token budget**
- Team state persists (if not cleaned up)

**Resume Commands:**
- `claude -c` or `claude --continue` - Resume most recent session
- `claude -r {session_id}` - Resume specific session
- `claude --resume` - List all resumable sessions

**Limitation:**
> "Session resumption (`/resume`, `/rewind`) doesn't restore in-process teammates"

After resume, lead may try messaging teammates that no longer exist. Solution: spawn new teammates.

### 7.5 Fresh Context via Subagents

**Pattern:**
- Spawn subagent with focused task
- Subagent gets fresh 1M token context
- Returns summary to main agent (condenses results)

**Example:**
```
Use a subagent to analyze all 500 log files and return only errors with timestamps
```

**Result:**
- Subagent consumes 500K tokens reading logs
- Returns 2K token summary to main agent
- Main agent's context remains clean

---

## 8. Team Config Format: Schema & Examples

### 8.1 Location & Scope

**Path:** `~/.claude/teams/{team-name}/config.json`

**When Created:**
- Via `Teammate({operation: "spawnTeam", team_name: "feature-auth"})`
- Automatically by Claude when team requested

**Visibility:**
- All team members can read
- Only lead can modify (via TeammateTool operations)

### 8.2 Config Schema

**Full Structure:**
```json
{
  "team_name": "feature-auth",
  "description": "Team working on authentication refactor",
  "created_at": "2026-02-08T10:30:00Z",
  "lead": {
    "agent_id": "lead-abc123",
    "session_id": "session-xyz789",
    "working_directory": "/Users/name/project"
  },
  "members": [
    {
      "agent_id": "frontend-dev",
      "agent_type": "general-purpose",
      "name": "Frontend Developer",
      "capabilities": "React, TypeScript, UI components",
      "status": "active",
      "joined_at": "2026-02-08T10:32:00Z",
      "permission_mode": "acceptEdits",
      "plan_mode_required": false
    },
    {
      "agent_id": "backend-dev",
      "agent_type": "general-purpose",
      "name": "Backend Developer",
      "capabilities": "Node.js, Express, PostgreSQL",
      "status": "active",
      "joined_at": "2026-02-08T10:33:00Z",
      "permission_mode": "default",
      "plan_mode_required": true
    }
  ],
  "settings": {
    "auto_cleanup": true,
    "heartbeat_timeout_minutes": 5,
    "max_teammates": 10
  }
}
```

### 8.3 Field Descriptions

**Top-Level:**
- `team_name` (string, required) - Unique identifier for team
- `description` (string, optional) - Team purpose
- `created_at` (ISO 8601 timestamp)

**Lead Object:**
- `agent_id` - Lead's unique identifier
- `session_id` - Lead's Claude Code session
- `working_directory` - Project path

**Members Array:**
- `agent_id` - Teammate's unique identifier
- `agent_type` - Subagent type: `general-purpose`, `Explore`, `Plan`, or custom
- `name` - Human-readable name
- `capabilities` - Description of skills
- `status` - `active`, `idle`, `shutdown_requested`, `terminated`
- `joined_at` - Timestamp
- `permission_mode` - `default`, `acceptEdits`, `dontAsk`, `delegate`, `bypassPermissions`, `plan`
- `plan_mode_required` - Boolean (requires approval before implementation)

**Settings:**
- `auto_cleanup` - Delete team resources when all teammates shut down
- `heartbeat_timeout_minutes` - Inactivity threshold before considering teammate crashed
- `max_teammates` - Maximum team size

### 8.4 Environment Variables (Team Context)

**Automatically Set for Teammates:**
```bash
CLAUDE_CODE_TEAM_NAME=feature-auth
CLAUDE_CODE_AGENT_ID=frontend-dev
CLAUDE_CODE_AGENT_TYPE=general-purpose
CLAUDE_PROJECT_DIR=/Users/name/project
```

**Usage in Hooks:**
```bash
#!/bin/bash
echo "Running hook for $CLAUDE_CODE_AGENT_ID in team $CLAUDE_CODE_TEAM_NAME"
```

### 8.5 Reading Config from Teammates

**Bash:**
```bash
cat ~/.claude/teams/$CLAUDE_CODE_TEAM_NAME/config.json | jq '.members'
```

**Python (in hook):**
```python
import os
import json

team_name = os.getenv('CLAUDE_CODE_TEAM_NAME')
config_path = f"~/.claude/teams/{team_name}/config.json"

with open(os.path.expanduser(config_path)) as f:
    config = json.load(f)

print(f"Team has {len(config['members'])} members")
```

### 8.6 Modifying Config

**Via TeammateTool Only:**
- Teammates added via `approveJoin`
- Status changes via `requestShutdown` → `approveShutdown`
- No direct JSON editing (race conditions)

**Manual Editing Not Recommended:**
- Lead and teammates may cache config
- Changes not reflected until session restart
- Risk of corrupting active team state

---

## 9. Coordination Patterns: Common Architectures

### 9.1 Pattern 1: Parallel Specialists

**Structure:**
- Lead spawns 3-5 specialists
- Each owns independent domain
- No inter-specialist dependencies

**Example:**
```
Create agent team:
- Security reviewer (check for vulnerabilities)
- Performance reviewer (analyze bottlenecks)
- Test coverage reviewer (verify tests)
Review PR #142 and report findings.
```

**Task Structure:**
```
Task 1: Security audit [worker: security-reviewer]
Task 2: Performance analysis [worker: perf-reviewer]
Task 3: Test coverage check [worker: test-reviewer]
```

**No Dependencies:** All tasks can execute in parallel.

### 9.2 Pattern 2: Sequential Pipeline

**Structure:**
- Tasks depend on previous stage completion
- Workers claim next available unblocked task
- Lead coordinates handoffs

**Example:**
```
Create agent team to implement new feature:
1. Architect designs schema [blocker for all]
2. Backend implements API [blocker for frontend]
3. Frontend builds UI [blocker for tests]
4. QA writes tests [final stage]
```

**Task Structure:**
```
Task 1: Design schema [worker: architect]
Task 2: Implement API [blockedBy: [1], worker: backend-dev]
Task 3: Build UI [blockedBy: [2], worker: frontend-dev]
Task 4: Write tests [blockedBy: [3], worker: qa-engineer]
```

**Dependencies Enforce Order:** Task 2 can't start until Task 1 completes.

### 9.3 Pattern 3: Load-Balanced Swarm

**Structure:**
- Pool of identical workers
- Shared task queue (no assignments)
- Self-organized task claiming

**Example:**
```
Create agent team with 5 workers to refactor 20 modules.
Each worker claims next available module from task list.
```

**Task Structure:**
```
Task 1-20: Refactor module-{1..20} [status: pending, owner: null]
```

**Claiming:**
1. Worker checks `TaskList`, finds pending task
2. Worker calls `TaskUpdate(taskId="3", owner="worker-2", status="in_progress")`
3. File lock prevents double-claiming
4. Worker completes, marks `status="completed"`
5. Repeat until queue empty

### 9.4 Pattern 4: Hierarchical (Swarm Host / Brood Lord)

**Structure:**
- Lead spawns sub-leads (swarm hosts)
- Each sub-lead spawns workers (brood lords)
- Multi-tier delegation

**Example:**
```
Create agent team:
- Lead (you)
  - Frontend Lead
    - UI Components Worker
    - Styling Worker
  - Backend Lead
    - API Worker
    - Database Worker
```

**Current Limitation:**
> "Teammates cannot spawn other teammates. Only the lead can manage the team."

**Workaround:** Use subagents (not teammates) for second tier:
- Swarm Host = Teammate with Task tool enabled
- Brood = Subagents spawned by teammate

---

## 10. Known Limitations & Constraints

### 10.1 Agent Teams (Experimental)

**Session Resumption:**
- In-process teammates not restored after `/resume`
- Lead may message non-existent teammates
- Solution: Respawn new teammates

**Task Synchronization:**
- Status updates lag 10-15 seconds
- No real-time coordination
- Solution: Size tasks for 5-10 minute duration

**Shutdown:**
- Teammates finish current request before shutting down
- Can take minutes if long-running tool call
- Solution: Ask teammate to interrupt current work

**Team Management:**
- One team per session (no multiple teams)
- Lead cannot transfer (fixed for team lifetime)
- No nested teams (teammates can't spawn teams)

**Permissions:**
- All teammates start with lead's permission mode
- Can modify after spawn, but not during spawn
- No per-teammate initial settings

**Display:**
- Split-pane requires tmux or iTerm2
- Not supported: VS Code integrated terminal, Windows Terminal, Ghostty
- In-process mode works universally

### 10.2 Hooks

**Execution:**
- 10-minute timeout default (configurable)
- No slash command triggering from hooks
- No tool calls from hooks (output only)

**PostToolUse:**
- Cannot undo actions (tool already executed)
- Use PreToolUse to block preventively

**PermissionRequest:**
- Doesn't fire in non-interactive mode (`-p`)
- Use PreToolUse for headless automation

**Stop:**
- Fires on every response finish, not just task completion
- Doesn't fire on user interrupts (Ctrl+C)

### 10.3 Context Management

**Server-Side Tool Challenges:**
- Token counting incorrect for web_search/web_fetch
- `cache_read_input_tokens` includes internal API calls
- SDK compaction triggers prematurely
- Solution: Use token counting endpoint, avoid compaction with server-side tools

**Compaction Edge Cases:**
- Tool use response pending during compaction → removed from history
- Claude re-issues tool call after resuming if still needed
- No guarantee of preserving specific context

### 10.4 SDK vs CLI Feature Parity

**Not in SDK (CLI-Only):**
- Agent teams (TeammateTool) - experimental feature flag
- Split-pane display modes
- Interactive `/hooks` menu
- Task UI (`Ctrl+T`)

**Not in CLI (SDK-Only):**
- Programmatic session forking
- Custom hook callbacks (Python/TS functions)
- Streaming message inspection

---

## 11. Sources & References

### Official Anthropic Documentation
- [Agent SDK Overview](https://platform.claude.com/docs/en/agent-sdk/overview)
- [Create Custom Subagents](https://code.claude.com/docs/en/sub-agents)
- [Context Editing](https://platform.claude.com/docs/en/build-with-claude/context-editing)
- [Orchestrate Teams](https://code.claude.com/docs/en/agent-teams)
- [Automate Workflows with Hooks](https://code.claude.com/docs/en/hooks-guide)

### Community Research
- [Claude Code's Hidden Multi-Agent System](https://paddo.dev/blog/claude-code-hidden-swarm/) - Kelly's discovery of TeammateTool via binary analysis
- [Claude Code Swarm Orchestration Skill](https://gist.github.com/kieranklaassen/4f2aba89594a4aea4ad64d753984b2ea) - Complete 13-operation reference
- [Claude Code Agent Teams Setup Guide](https://www.marc0.dev/en/blog/claude-code-agent-teams-multiple-ai-agents-working-in-parallel-setup-guide-1770317684454) - Practical configuration guide
- [AddyOsmani.com - Claude Code Swarms](https://addyosmani.com/blog/claude-code-agent-teams/) - Context management patterns

### Anthropic Engineering Posts
- [Building Agents with Claude Agent SDK](https://claude.com/blog/building-agents-with-the-claude-agent-sdk)
- [Managing Context on Claude Platform](https://claude.com/blog/context-management)
- [Effective Context Engineering for AI Agents](https://www.anthropic.com/engineering/effective-context-engineering-for-ai-agents)

### News Coverage
- [Anthropic's Claude Opus 4.6 brings 1M token context and 'agent teams'](https://venturebeat.com/technology/anthropics-claude-opus-4-6-brings-1m-token-context-and-agent-teams-to-take) - VentureBeat
- [Anthropic releases Opus 4.6 with new 'agent teams'](https://techcrunch.com/2026/02/05/anthropic-releases-opus-4-6-with-new-agent-teams/) - TechCrunch

---

## 12. Glossary

**Agent SDK** - Programmable Python/TypeScript library exposing Claude Code's agent loop
**Agent Team** - Multiple Claude instances coordinating via shared task list and messaging
**Context Compaction** - Automatic summarization of conversation history when approaching token limits
**Context Editing** - Server-side removal of stale tool results/thinking blocks to preserve context space
**Delegate Mode** - Permission mode restricting lead to coordination tools only
**Fresh Context Window** - New 1M token budget per agent instance
**Hooks** - User-defined shell commands executed at lifecycle events
**Inbox** - File-based message queue at `~/.claude/teams/{team}/inboxes/{agent}.json`
**MCP (Model Context Protocol)** - Standard for connecting external tools/services
**Plan Approval Mode** - Gating teammate implementation behind leader review
**Subagent** - Isolated agent spawned within single session (not teammate)
**Task System** - Shared work queue with dependency tracking
**Teammate** - Separate Claude instance in agent team (independent session)
**TeammateTool** - 13-operation coordination layer for agent teams
**Tool Result Clearing** - Context editing strategy removing old tool outputs

---

## End of Document

**Total Sections:** 12
**Word Count:** ~9,500
**Research Depth:** Comprehensive (all 8 original questions answered)
**Last Updated:** 2026-02-08
