# Claude Code Teams/Swarm Infrastructure — Complete Dissection

**Source**: Official Claude Code repository + documentation (2026-02-08)
**Purpose**: Understand the architecture to guide Rust reimplementation in Hatchery

---

## 1. TeammateTool — 13 Core Operations

### File Path
- Not open-source — TeammateTool is built into the Claude Code binary
- Exposed through JSON-based tool interface (MCP-style)
- Feature-gated via `I9()` and `qFB()` validation functions

### Operations Breakdown

#### Team Lifecycle (3 operations)

**1. spawnTeam**
```typescript
Teammate({
  operation: "spawnTeam",
  team_name: "feature-auth",
  description: "Implementing OAuth2"
})
```
- Creates `~/.claude/teams/{team_name}/` directory
- Initializes `config.json` with leader metadata
- Designates caller as team leader
- Returns team ID

**2. discoverTeams**
```typescript
Teammate({ operation: "discoverTeams" })
```
- Scans `~/.claude/teams/` for available teams
- Returns list excluding teams you're already in
- Used for joining existing teams

**3. cleanup**
```typescript
Teammate({ operation: "cleanup" })
```
- Removes `~/.claude/teams/{team}/` and `~/.claude/tasks/{team}/`
- **CRITICAL**: Fails if any teammates are still active
- **MUST** be called by leader only (teammates have incomplete team context)

#### Membership Management (4 operations)

**4. requestJoin**
```typescript
Teammate({
  operation: "requestJoin",
  team_name: "feature-auth",
  proposed_name: "helper",
  capabilities: "Code review capability"
})
```
- Sends join request to team leader's inbox
- Queued as `join_request` message type

**5. approveJoin** (Leader only)
```typescript
Teammate({
  operation: "approveJoin",
  target_agent_id: "helper",
  request_id: "join-123"
})
```
- Adds member to `config.json` members array
- Creates inbox file for new member

**6. rejectJoin** (Leader only)
```typescript
Teammate({
  operation: "rejectJoin",
  target_agent_id: "helper",
  request_id: "join-123",
  reason: "Team at capacity"
})
```
- Sends rejection message to requester

#### Communication (2 operations)

**7. write** — Direct Message
```typescript
Teammate({
  operation: "write",
  target_agent_id: "security-reviewer",
  value: "Prioritize auth module review"
})
```
- **CRITICAL PATTERN**: Teammates have NO visible text output
- All communication MUST use `write` operation
- Writes JSON message to `~/.claude/teams/{team}/inboxes/{target}.json`

**8. broadcast** — Group Message
```typescript
Teammate({
  operation: "broadcast",
  name: "team-lead",
  value: "Status check: Report progress"
})
```
- **WARNING**: Sends N messages for N teammates (expensive)
- Token cost scales linearly with team size
- Reserve for critical announcements only

#### Graceful Shutdown (4 operations)

**9. requestShutdown** (Leader only)
```typescript
Teammate({
  operation: "requestShutdown",
  target_agent_id: "security-reviewer",
  reason: "Tasks complete"
})
```
- Sends `shutdown_request` to teammate's inbox

**10. approveShutdown** (Teammate only)
```typescript
Teammate({
  operation: "approveShutdown",
  request_id: "shutdown-123"
})
```
- Sends confirmation and terminates the Claude Code process

**11. rejectShutdown** (Teammate only)
```typescript
Teammate({
  operation: "rejectShutdown",
  request_id: "shutdown-123",
  reason: "Still on task #3"
})
```
- Refuses shutdown with explanation

#### Plan Approval (2 operations)

**12. approvePlan** (Leader only)
```typescript
Teammate({
  operation: "approvePlan",
  target_agent_id: "architect",
  request_id: "plan-456"
})
```
- Allows teammate to exit plan-only mode
- Teammate can now execute tool calls

**13. rejectPlan** (Leader only)
```typescript
Teammate({
  operation: "rejectPlan",
  target_agent_id: "architect",
  request_id: "plan-456",
  feedback: "Add error handling and rate limiting"
})
```
- Sends feedback, teammate stays in plan mode
- Teammate revises and resubmits

---

## 2. Task System — DAG Management

### File Path
- Task storage: `~/.claude/tasks/{team_name}/{task_id}.json`
- Task tool operations exposed as separate tools

### Operations

**TaskCreate**
```typescript
TaskCreate({
  subject: "Review authentication",
  description: "Review auth module for vulnerabilities",
  activeForm: "Reviewing auth module..."
})
```
- Returns `task_id` (auto-incremented)
- Initial status: `"pending"`

**TaskList**
```typescript
TaskList()
```
- Returns all tasks with `{ id, subject, status, owner, blockedBy }`

**TaskGet**
```typescript
TaskGet({ taskId: "2" })
```
- Returns full task JSON

**TaskUpdate**
```typescript
// Claim task
TaskUpdate({ taskId: "2", owner: "security-reviewer" })

// Change status
TaskUpdate({ taskId: "2", status: "in_progress" })
TaskUpdate({ taskId: "2", status: "completed" })

// Add dependency
TaskUpdate({ taskId: "3", addBlockedBy: ["1"] })

// Remove dependency
TaskUpdate({ taskId: "3", removeBlockedBy: ["1"] })
```

**TaskStop**
```typescript
TaskStop({ taskId: "2" })
```
- Stops task execution (for background tasks)

### Task JSON Schema
```json
{
  "id": "2",
  "subject": "Review auth module",
  "description": "Security audit of authentication code",
  "activeForm": "Reviewing...",
  "status": "in_progress",
  "owner": "security-reviewer",
  "blockedBy": ["1"],
  "created": "2026-02-08T12:00:00Z",
  "updated": "2026-02-08T12:05:00Z"
}
```

### Task DAG Mechanics

**Auto-Unblocking**:
```typescript
// Setup pipeline
TaskCreate({ subject: "Research" })      // #1
TaskCreate({ subject: "Implement" })     // #2
TaskCreate({ subject: "Test" })          // #3

TaskUpdate({ taskId: "2", addBlockedBy: ["1"] })
TaskUpdate({ taskId: "3", addBlockedBy: ["2"] })

// When #1 completes → #2 auto-unblocks
// When #2 completes → #3 auto-unblocks
```

**File Locking for Claims**:
- Uses file system locks to prevent race conditions
- Multiple teammates can claim tasks simultaneously
- First one to acquire lock wins
- Implementation details not exposed (binary)

**Cycle Detection**:
- Prevents circular dependencies (A → B → A)
- Enforced at `TaskUpdate` time
- Exact algorithm not documented

---

## 3. Agent Spawning — Two Modes

### Subagent (One-Off, No Team)
```typescript
Task({
  subagent_type: "Explore",
  description: "Find auth files",
  prompt: "Locate all authentication files",
  model: "haiku",
  run_in_background: false  // Synchronous by default
})
```
- Spawns ephemeral Claude instance
- Returns result directly to caller
- No team membership
- **Async mode**: `run_in_background: true` (returns TaskOutput later)

### Teammate (Persistent, Team Member)
```typescript
// Step 1: Create team
Teammate({ operation: "spawnTeam", team_name: "my-project" })

// Step 2: Spawn teammate as background task
Task({
  team_name: "my-project",
  name: "security-reviewer",
  subagent_type: "security-sentinel",
  prompt: "Review auth code. Send findings to team-lead.",
  run_in_background: true  // MUST be async for teammates
})
```
- Joins team automatically
- Appears in `config.json` members array
- Communicates via inbox
- Can claim tasks from shared task list

### Built-In Agent Types

**Core**:
- `Bash`: Git operations, system tasks
- `Explore`: Read-only codebase exploration (uses Haiku)
- `Plan`: Architecture design
- `general-purpose`: Full tool access

**Meta**:
- `claude-code-guide`: Questions about Claude Code itself
- `statusline-setup`: Configure status display

**Plugin Types** (from code-review, feature-dev, etc.):
- `security-sentinel`, `performance-oracle`, `architecture-strategist`
- `code-simplicity-reviewer`, `kieran-rails-reviewer`
- `best-practices-researcher`, `framework-docs-researcher`
- `git-history-analyzer`, `bug-reproduction-validator`

---

## 4. Mailbox/Inbox System

### File Structure
```
~/.claude/teams/{team_name}/
├── config.json
└── inboxes/
    ├── team-lead.json
    ├── worker-1.json
    └── worker-2.json
```

### config.json Schema
```json
{
  "teamName": "feature-auth",
  "leaderId": "abc123",
  "created": "2026-02-08T12:00:00Z",
  "members": [
    {
      "agentId": "abc123",
      "name": "team-lead",
      "agentType": "Leader",
      "joined": "2026-02-08T12:00:00Z"
    },
    {
      "agentId": "def456",
      "name": "security-reviewer",
      "agentType": "security-sentinel",
      "joined": "2026-02-08T12:05:00Z"
    }
  ]
}
```

### Inbox Message Types

**Standard Message**:
```json
{
  "from": "team-lead",
  "text": "Prioritize auth module",
  "timestamp": "2026-01-25T23:38:32Z",
  "read": false
}
```

**Shutdown Request**:
```json
{
  "type": "shutdown_request",
  "requestId": "shutdown-abc123",
  "from": "team-lead",
  "reason": "All tasks complete"
}
```

**Task Completed Notification**:
```json
{
  "type": "task_completed",
  "from": "worker-1",
  "taskId": "2",
  "taskSubject": "Review module"
}
```

**Plan Approval Request**:
```json
{
  "type": "plan_approval_request",
  "from": "architect",
  "requestId": "plan-xyz789",
  "planContent": "# Implementation Plan\n..."
}
```

**Join Request**:
```json
{
  "type": "join_request",
  "proposedName": "helper",
  "requestId": "join-abc123",
  "capabilities": "Testing and review"
}
```

### Delivery Mechanism

**Push Model** (not polling):
- Messages delivered automatically to recipients
- Leader receives idle notifications when teammates stop
- No active polling loop required
- Implementation: File system watching + event emitters (binary, not exposed)

**Message Ordering**:
- Timestamped for chronological ordering
- `read` flag tracks processing state

---

## 5. Context Compression — /compact

### Trigger Thresholds

**Auto-Compact**:
- Triggers at **75-92% context usage** (varies by model)
- Reserves ~20% for the compaction process itself
- Leaves 25% free for reasoning after compaction

**Manual**:
- User runs `/compact` command
- Can include custom instructions

### Implementation

**API-Level** (for Agent SDK):
```typescript
// Requires beta header
headers: {
  "compact-2026-01-12": "enabled"
}
```
- Detects when input tokens exceed threshold
- Generates summary of conversation
- Creates compaction block in conversation
- Continues with compacted context

**Claude Code Internal**:
- Not exposed in source (binary implementation)
- PreCompact hook fires before compaction
- Hook receives `{ trigger: "auto" | "manual", custom_instructions: string }`

### What Gets Compressed

**Compressed**:
- Old conversation turns
- Tool call results (summarized)
- Redundant context

**Protected** (not compressed):
- CLAUDE.md files
- MCP server context
- Skill definitions
- Recent turns (last N messages)

### Compaction Strategies

VSCode extension compacts at **~25% remaining** (75% usage).
Official recommendation: **65-70%** for best results.

**Threshold Trade-offs**:
- 65-70%: More context preserved, proactive
- 75-92%: Maximizes context window usage
- Reserve 20-25% for reasoning + compaction overhead

---

## 6. Team Config Structure

### Directory Layout
```
~/.claude/
├── teams/{team_name}/
│   ├── config.json
│   └── inboxes/{agent_id}.json
├── tasks/{team_name}/
│   └── {task_id}.json
├── projects/{project_hash}/
│   └── {session_id}.jsonl  # Transcript
└── settings.json
```

### Environment Variables

**Agent Identity**:
- `CLAUDE_CODE_TEAM_NAME`: Team identifier
- `CLAUDE_CODE_AGENT_ID`: Individual agent identifier
- `CLAUDE_CODE_AGENT_TYPE`: Role (Leader, Swarm, Pipeline, Council, Watchdog)

**Project Context**:
- `CLAUDE_PROJECT_DIR`: Project root directory
- `CLAUDE_PLUGIN_ROOT`: Plugin directory (for plugin scripts)
- `CLAUDE_ENV_FILE`: Path for persisting env vars (SessionStart hooks only)

**Feature Flags**:
- `CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS`: Enable teams (disabled by default)
- `CLAUDE_CODE_REMOTE`: Set to "true" in web environments

### settings.json Schema (Hooks)
```json
{
  "env": {
    "CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS": "1"
  },
  "teammateMode": "auto" | "in-process" | "tmux",
  "disableAllHooks": false,
  "allowManagedHooksOnly": false,
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Bash",
        "hooks": [
          {
            "type": "command",
            "command": ".claude/hooks/block-rm.sh",
            "timeout": 600,
            "async": false
          }
        ]
      }
    ]
  }
}
```

---

## 7. Hooks System — Lifecycle Integration

### Hook Events (14 total)

**Session Lifecycle**:
- `SessionStart`: Session begins or resumes (matcher: `startup|resume|clear|compact`)
- `SessionEnd`: Session terminates (matcher: `clear|logout|prompt_input_exit|bypass_permissions_disabled|other`)
- `PreCompact`: Before compaction (matcher: `manual|auto`)

**Agentic Loop**:
- `UserPromptSubmit`: Before processing user prompt (no matcher)
- `PreToolUse`: Before tool execution (matcher: tool name)
- `PermissionRequest`: When permission dialog appears (matcher: tool name)
- `PostToolUse`: After tool succeeds (matcher: tool name)
- `PostToolUseFailure`: After tool fails (matcher: tool name)
- `Stop`: Agent finished responding (no matcher)

**Subagents**:
- `SubagentStart`: Subagent spawned (matcher: agent type)
- `SubagentStop`: Subagent finished (matcher: agent type)

**Teams**:
- `TeammateIdle`: Teammate about to go idle (no matcher)
- `TaskCompleted`: Task being marked complete (no matcher)

**Notifications**:
- `Notification`: Notification sent (matcher: `permission_prompt|idle_prompt|auth_success|elicitation_dialog`)

### Hook Types

**Command Hooks**:
```json
{
  "type": "command",
  "command": ".claude/hooks/script.sh",
  "timeout": 600,
  "async": false,
  "statusMessage": "Running validation..."
}
```
- Receives JSON input via stdin
- Returns JSON output via stdout
- Exit codes: 0 (allow), 2 (block), other (non-blocking error)

**Prompt Hooks** (LLM-based):
```json
{
  "type": "prompt",
  "prompt": "Evaluate: $ARGUMENTS. Check if tasks complete.",
  "model": "haiku",
  "timeout": 30
}
```
- Sends input + prompt to Claude model
- Returns `{ "ok": true|false, "reason": "..." }`

**Agent Hooks** (Multi-turn):
```json
{
  "type": "agent",
  "prompt": "Verify tests pass. $ARGUMENTS",
  "model": "haiku",
  "timeout": 60
}
```
- Spawns subagent with tool access (Read, Grep, Glob)
- Up to 50 turns
- Returns `{ "ok": true|false, "reason": "..." }`

### Hook Input/Output Schemas

**Common Input** (all hooks):
```json
{
  "session_id": "abc123",
  "transcript_path": "/path/to/transcript.jsonl",
  "cwd": "/project/dir",
  "permission_mode": "default|plan|acceptEdits|dontAsk|bypassPermissions",
  "hook_event_name": "PreToolUse"
}
```

**PreToolUse Input**:
```json
{
  "tool_name": "Bash",
  "tool_input": { "command": "npm test" },
  "tool_use_id": "toolu_01ABC123..."
}
```

**TeammateIdle Input**:
```json
{
  "teammate_name": "researcher",
  "team_name": "my-project"
}
```

**TaskCompleted Input**:
```json
{
  "task_id": "task-001",
  "task_subject": "Implement user auth",
  "task_description": "Add login endpoints",
  "teammate_name": "implementer",
  "team_name": "my-project"
}
```

### Exit Code Behaviors

| Hook Event         | Can Block? | Exit 2 Behavior                                      |
|--------------------|------------|-----------------------------------------------------|
| PreToolUse         | Yes        | Blocks tool call                                    |
| PermissionRequest  | Yes        | Denies permission                                   |
| UserPromptSubmit   | Yes        | Blocks prompt, erases it                            |
| Stop               | Yes        | Prevents stopping, continues conversation           |
| SubagentStop       | Yes        | Prevents subagent from stopping                     |
| TeammateIdle       | Yes        | Prevents idle, teammate continues working           |
| TaskCompleted      | Yes        | Prevents task completion                            |
| PostToolUse        | No         | Shows stderr to Claude (tool already ran)           |
| PostToolUseFailure | No         | Shows stderr to Claude                              |
| Notification       | No         | Shows stderr to user only                           |
| SubagentStart      | No         | Shows stderr to user only                           |
| SessionStart       | No         | Shows stderr to user only                           |
| SessionEnd         | No         | Shows stderr to user only                           |
| PreCompact         | No         | Shows stderr to user only                           |

### Decision Control Patterns

**Top-level `decision`** (UserPromptSubmit, PostToolUse, Stop):
```json
{
  "decision": "block",
  "reason": "Test suite must pass"
}
```

**PreToolUse** (`hookSpecificOutput`):
```json
{
  "hookSpecificOutput": {
    "hookEventName": "PreToolUse",
    "permissionDecision": "allow|deny|ask",
    "permissionDecisionReason": "Reason...",
    "updatedInput": { "command": "npm run lint" },
    "additionalContext": "Current env: production"
  }
}
```

**PermissionRequest** (`hookSpecificOutput`):
```json
{
  "hookSpecificOutput": {
    "hookEventName": "PermissionRequest",
    "decision": {
      "behavior": "allow|deny",
      "updatedInput": { "command": "npm run lint" },
      "updatedPermissions": [...],
      "message": "Denied: too risky",
      "interrupt": true
    }
  }
}
```

**TeammateIdle / TaskCompleted**:
- Exit code 2 only (no JSON decision)
- stderr message fed back to model as feedback

---

## 8. SendMessage Tool — Legacy/Alternative

**NOTE**: SendMessage appears in some documentation but is NOT the same as TeammateTool.

SendMessage was an earlier/alternative pattern. Modern teams use **TeammateTool write/broadcast**.

If encountered:
```typescript
SendMessage({
  message: "text",
  broadcast: false,
  shutdown_request: false,
  shutdown_response: "approve|reject",
  plan_approval: "approve|reject"
})
```

**Recommendation**: Ignore SendMessage, focus on TeammateTool operations.

---

## Architecture Summary

### Team Lead Session
```
[Claude Code Instance — Leader]
├── TeammateTool (13 operations)
├── Task system (TaskCreate, TaskUpdate, TaskList, TaskGet, TaskStop)
├── Inbox monitor (receives messages from teammates)
├── Team config manager (manages members)
└── Hooks (TeammateIdle, TaskCompleted)
```

### Teammate Session
```
[Claude Code Instance — Teammate]
├── TeammateTool (limited: write, broadcast, requestShutdown responses)
├── Task system (TaskUpdate for claiming, TaskList for discovery)
├── Inbox monitor (receives messages from leader)
├── NO visible text output (must use TeammateTool write)
└── Hooks (all normal hooks apply)
```

### Data Flow

**Task Assignment**:
1. Leader creates tasks via TaskCreate
2. Tasks appear in `~/.claude/tasks/{team}/`
3. Teammates poll TaskList
4. Teammate claims via TaskUpdate (file lock prevents race)
5. Teammate works on task
6. Teammate marks complete via TaskUpdate (triggers TaskCompleted hook)
7. Blocked tasks auto-unblock

**Message Delivery**:
1. Sender calls TeammateTool write/broadcast
2. JSON message written to `~/.claude/teams/{team}/inboxes/{recipient}.json`
3. Recipient's inbox monitor delivers message (push, not poll)
4. Message appears in recipient's context

**Graceful Shutdown**:
1. Leader sends requestShutdown
2. Teammate receives shutdown_request in inbox
3. Teammate can approveShutdown or rejectShutdown
4. If approved, teammate exits gracefully
5. Leader calls cleanup after all teammates shut down

---

## Key Insights for Rust Reimplementation

### 1. File-Based Coordination
- All state persisted in `~/.claude/` (teams, tasks, inboxes)
- File locking for task claims (use `fs2` crate in Rust)
- JSON schema for messages/config (use `serde_json`)

### 2. Push-Based Messaging
- Not polling — file system watching (use `notify` crate)
- Event-driven architecture
- Async message delivery

### 3. Hook Integration Points
- 14 hook events covering full lifecycle
- 3 hook types (command, prompt, agent)
- Exit code + JSON output for control
- Async hooks for long-running operations

### 4. Context Management
- Auto-compact at 75-92% usage
- Protect CLAUDE.md, MCP, skills from compression
- Reserve 20-25% for reasoning

### 5. Team Patterns
- **Leader**: Orchestration only (in delegate mode)
- **Swarm**: Self-organizing task claims
- **Pipeline**: Sequential dependencies with auto-unblock
- **Council**: Debate/consensus (multiple agents on same task)
- **Watchdog**: Quality gates via hooks

### 6. Token Cost Model
- Each teammate is a separate Claude instance
- Broadcast scales linearly (N messages for N teammates)
- Subagents cheaper than teammates (ephemeral, no team overhead)

### 7. Security Model
- All agents inherit leader's permissions
- Hooks run with full user permissions (dangerous)
- Feature-gated (experimental, disabled by default)

---

## Critical Gaps (Not Exposed)

### Binary Implementation Details
- **File locking algorithm**: Exact mechanism for task claim atomicity
- **Inbox delivery**: File watching vs. polling internals
- **Cycle detection**: DAG validation algorithm
- **Token accounting**: Cost tracking per teammate
- **Heartbeat/timeout**: 5min timeout mentioned but not specified
- **Feature gates**: `I9()` and `qFB()` validation logic

### Missing Specs
- Maximum team size
- Maximum task count
- Inbox message size limits
- Compaction algorithm internals
- Error recovery (crashed teammate, orphaned locks)

### Undocumented Patterns
- Nested teams (explicitly forbidden)
- Cross-team communication
- Team persistence/resumption
- Migration between team structures

---

## Sources

- [Orchestrate teams of Claude Code sessions](https://code.claude.com/docs/en/agent-teams)
- [Claude Code Swarm Orchestration Skill](https://gist.github.com/kieranklaassen/4f2aba89594a4aea4ad64d753984b2ea)
- [Claude Code's Hidden Multi-Agent System](https://paddo.dev/blog/claude-code-hidden-swarm/)
- [Hooks reference - Claude Code Docs](https://code.claude.com/docs/en/hooks)
- [Introducing Claude Opus 4.6](https://www.anthropic.com/news/claude-opus-4-6)
- [Anthropic introduces Claude Opus 4.6 with Agent Teams](https://www.heise.de/en/news/Anthropic-introduces-Claude-Opus-4-6-with-Agent-Teams-11167248.html)
- [AddyOsmani.com - Claude Code Swarms](https://addyosmani.com/blog/claude-code-agent-teams/)
- [Claude Code Multi-Agent Orchestration System](https://gist.github.com/kieranklaassen/d2b35569be2c7f1412c64861a219d51f)
- [How Claude Code Got Better by Protecting More Context](https://hyperdev.matsuoka.com/p/how-claude-code-got-better-by-protecting)
- [Claude Code Compaction | Steve Kinney](https://stevekinney.com/courses/ai-development/claude-code-compaction)

---

**END OF DISSECTION**
