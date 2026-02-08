# Claude Code C Compiler Swarm: Deep Technical Architecture

**Research Date:** 2026-02-08
**Subject:** Anthropic's 16-Agent C Compiler Project - Complete Technical Architecture Analysis
**Status:** Comprehensive Deep Dive

---

## Executive Summary

Anthropic's research team (led by Nicholas Carlini) tasked 16 parallel Claude Opus 4.6 instances with building a production-grade C compiler in Rust from scratch. Over nearly 2,000 Claude Code sessions and $20,000 in API costs, the agent team produced a 100,000-line compiler capable of compiling the Linux 6.9 kernel across x86 (64-bit and 32-bit), ARM, and RISC-V architectures. The compiler achieved a 99% pass rate on the GCC Torture Test Suite and successfully compiled major C codebases including PostgreSQL, SQLite, QEMU, FFmpeg, and the classic game Doom.

**Key Innovation:** Git-based task coordination with file locking, GCC oracle-based parallel debugging, and zero human intervention during implementation.

---

## 1. TeammateTool: The 13 Operations

TeammateTool is the core orchestration layer of Claude Code's Swarm mode, providing structured multi-agent coordination through 13 distinct operations across three functional categories.

### 1.1 Team Lifecycle Management

#### 1. `spawnTeam`
**Purpose:** Creates a team with the caller designated as team leader.

**Parameters:**
- `team_name` (required)
- `description` (optional)

**Effects:**
- Creates `~/.claude/teams/{team-name}/config.json`
- Creates `~/.claude/tasks/{team-name}/` directory
- Caller becomes team leader automatically

**Usage:**
```
Create an agent team with 4 teammates to refactor these modules in parallel.
```

---

#### 2. `discoverTeams`
**Purpose:** Lists all available teams for joining (excludes current memberships).

**Parameters:** None

**Returns:** Team list with metadata including:
- Team name
- Lead agent ID
- Current member count
- Team status

---

#### 3. `cleanup`
**Purpose:** Removes team resources after shutdown.

**Parameters:** None

**Effects:**
- Deletes `~/.claude/teams/{team-name}/`
- Deletes `~/.claude/tasks/{team-name}/`

**Critical Constraint:** Will fail if teammates are still active. Must shut down all teammates first.

**Warning:** Always run cleanup from the team lead, not from teammates, to prevent inconsistent state.

---

### 1.2 Join/Leave Operations

#### 4. `requestJoin`
**Purpose:** Submit membership request to existing team.

**Parameters:**
- `team_name`
- `proposed_name`
- `capabilities` (description of skills/role)

**Effect:** Sends `join_request` message to team leader's inbox.

---

#### 5. `approveJoin`
**Purpose:** Leader accepts join request.

**Parameters:**
- `target_agent_id`
- `request_id`

**Effects:**
- Adds member to `config.json` members array
- Creates inbox file at `~/.claude/teams/{name}/inboxes/{agent}.json`

---

#### 6. `rejectJoin`
**Purpose:** Leader declines membership request.

**Parameters:**
- `target_agent_id`
- `request_id`
- `reason` (explanation for rejection)

**Effect:** Sends rejection notification message.

---

### 1.3 Coordination & Messaging

#### 7. `write`
**Purpose:** Send message to specific teammate.

**Parameters:**
- `operation: "write"`
- `target_agent_id`
- `value` (message text)

**Storage:** Written to `~/.claude/teams/{name}/inboxes/{agent}.json`

**Critical Note:** "Your text output is NOT visible to the team. You MUST use write" - standard output is isolated per agent, messaging requires explicit write() calls.

**Example:**
```
Send a message to the security-reviewer teammate asking them to focus on token handling.
```

---

#### 8. `broadcast`
**Purpose:** Send message to all teammates simultaneously.

**Parameters:**
- `operation: "broadcast"`
- `name`
- `value` (message text)

**Cost Warning:** Creates N separate messages for N teammates. Expensive at scale.

**Recommendation:** Use sparingly. Prefer targeted `write()` for one-to-one communication.

---

### 1.4 Plan Approval Workflow

#### 9. `approvePlan`
**Purpose:** Leader approves teammate plan (when `plan_mode_required=true`).

**Parameters:**
- `target_agent_id`
- `request_id`

**Trigger:** Responds to `plan_approval_request` message from teammate.

**Effect:** Teammate exits read-only plan mode and begins implementation.

---

#### 10. `rejectPlan`
**Purpose:** Leader provides feedback on rejected plan.

**Parameters:**
- `target_agent_id`
- `request_id`
- `feedback` (revision guidance)

**Effect:** Teammate remains in plan mode, revises based on feedback, resubmits.

**Use Case:** "Only approve plans that include test coverage" or "Reject plans that modify the database schema."

---

### 1.5 Graceful Shutdown

#### 11. `requestShutdown`
**Purpose:** Leader requests teammate termination.

**Parameters:**
- `target_agent_id`
- `reason`

**Message Format:** Sends `shutdown_request` JSON message.

---

#### 12. `approveShutdown`
**Purpose:** Teammate accepts termination request.

**Parameters:**
- `request_id`

**Effect:** Sends `shutdown_approved` confirmation, terminates process gracefully.

---

#### 13. `rejectShutdown`
**Purpose:** Teammate declines shutdown request.

**Parameters:**
- `request_id`
- `reason` (explanation for continuing)

**Use Case:** Teammate has active work in progress and requests extension.

---

## 2. JSON Inbox System Architecture

### 2.1 File System Structure

```
~/.claude/teams/{team-name}/
├── config.json              # Team metadata, members array
└── inboxes/
    ├── team-lead.json       # Leader's inbox
    └── {agent-name}.json    # One inbox file per teammate

~/.claude/tasks/{team-name}/
├── 1.json                   # Task with blockedBy[], blocks[] dependencies
├── 2.json
└── N.json
```

### 2.2 Message Schemas

| Message Type | Key Fields |
|--------------|-----------|
| **regular** | `from`, `text`, `timestamp`, `read` |
| **shutdown_request** | `type`, `requestId`, `from`, `reason`, `timestamp` |
| **shutdown_approved** | `type`, `requestId`, `from`, `paneId`, `backendType` |
| **idle_notification** | `type`, `from`, `completedTaskId`, `completedStatus` |
| **task_completed** | `type`, `from`, `taskId`, `taskSubject` |
| **plan_approval_request** | `type`, `from`, `requestId`, `planContent` |
| **join_request** | `type`, `proposedName`, `requestId`, `capabilities` |

### 2.3 Message Delivery Mechanism

**Push Model:** Messages are delivered automatically to recipients. The lead doesn't need to poll for updates.

**Automatic Notifications:**
- When teammates send messages → delivered automatically to recipients
- When teammates finish and stop → automatically notify the lead via `idle_notification`
- When tasks complete → `task_completed` messages sent to relevant agents

**Delivery Guarantee:** File-based inboxes provide durability. Messages persist across crashes and restarts.

**Pull Mechanism:** Agents read their `{agent-name}.json` inbox file to retrieve messages. While delivery is push-based (files written directly), consumption is pull-based (agents check their inbox).

---

## 3. File Locks & Atomic Task Claiming

### 3.1 Lock Format in `current_tasks/`

**Mechanism:** Agents create lock files in `current_tasks/` directory to claim tasks.

**Example:**
```
current_tasks/parse_if_statement.txt
current_tasks/codegen_function_definition.txt
current_tasks/fix_dash.txt
```

**Lock Content:** Simple text file containing agent ID and timestamp (implementation detail not publicly documented).

### 3.2 Race Condition Handling

**Git-Based Synchronization:** "If two agents try to claim the same task, Git's synchronization forces the second agent to pick a different one."

**How It Works:**
1. Agent A pulls latest from upstream
2. Agent A creates `current_tasks/task_X.txt`
3. Agent A commits and pushes
4. Agent B pulls latest (sees task_X already locked)
5. Agent B selects different task_Y
6. Agent B creates `current_tasks/task_Y.txt`
7. Agent B commits and pushes

**Optimistic Locking:** File-based locks with Git as the coordination layer. First push wins, conflicts force second agent to retry with different task.

**Production Implementation (from third-party systems):**
- **Lease-based locking:** Lock tied to task lease. Lease expires → lock auto-releases (no stale locks)
- **Lease versioning:** Optimistic leases (`lease_version`) enable concurrent lease attempts with only first succeeding
- **Path normalization:** `./file.py` equals `file.py` to prevent duplicate locks

### 3.3 Crash Handling

**Heartbeat Timeout:** 5 minutes. If a teammate crashes, they're automatically marked as inactive after timeout.

**Task Recovery:**
- Crashed agent's tasks remain in task list
- Other teammates can claim abandoned tasks after heartbeat timeout
- No stale locks: lease expiration releases locks automatically

**Orphaned Sessions:**
- Tmux sessions may persist after team ends
- Manual cleanup: `tmux ls && tmux kill-session -t <session-name>`

**PID Tracking (from production systems):**
- L1: PID tracking detects dead workers
- L2: Hot-swap to different model with dependency migration
- L3: Emergency orchestrator handoff with context checkpointing

**Invariant:** Failed tasks leave repository state identical to pre-execution. Orchestrator restarts deduplicate work automatically.

---

## 4. Task DAG: Dependency Management

### 4.1 DAG Structure

**Task Format (JSON):**
```json
{
  "taskId": "1",
  "subject": "Build API endpoint",
  "status": "pending",
  "blockedBy": ["2", "3"],
  "blocks": ["5", "6"],
  "assignee": null,
  "createdBy": "team-lead@compiler-team",
  "priority": "high"
}
```

**States:**
- `pending` - Not yet claimed
- `in_progress` - Claimed by agent
- `completed` - Finished successfully
- `failed` - Execution failed
- `poisoned` - Failed >N times, moved to dead-letter queue

### 4.2 Dependency Expression

**Directed Acyclic Graph (DAG):**
- Task 3 (Run Tests) cannot start until Task 1 (Build API) and Task 2 (Configure Auth) are complete
- Expressed via `blockedBy: ["1", "2"]` in Task 3
- Task 1 has `blocks: ["3"]` to track reverse dependencies

**Auto-Unblocking:** When a teammate completes a task that other tasks depend on, blocked tasks unblock automatically without manual intervention.

**API Example:**
```
TaskUpdate({ taskId: "3", addBlockedBy: ["1","2"] })
```

### 4.3 Persistence & Modification

**Storage:** `~/.claude/tasks/{team-name}/`

**Shared Access:** All agents read/write to same disk location via `CLAUDE_CODE_TASK_LIST_ID` environment variable pointing multiple instances at same task list.

**Cross-Session Coordination:**
```bash
export CLAUDE_CODE_TASK_LIST_ID=auth-system-v2
```

Multiple terminals → same `~/.claude/tasks/auth-system-v2/` → updates visible immediately to all sessions.

**Agent Modification:** Yes, agents can modify the DAG:
- Create new tasks
- Update dependencies
- Mark tasks complete/failed
- Claim available tasks

**Dynamic Review Loops (from production systems):**
- Bugs found → inject fix + re-review steps dynamically
- Bounded iterations → escalation to human
- High-risk tasks cannot bypass review gates

### 4.4 Task Claiming Algorithm

**Self-Claim:** After finishing a task, teammate picks next unassigned, unblocked task automatically.

**Leader Assign:** Lead can assign tasks explicitly to specific teammates.

**File Locking Integration:** Task claiming uses file locking to prevent race conditions when multiple teammates try to claim same task simultaneously.

**Claiming Protocol:**
1. Agent scans task list for `status=pending` with empty `blockedBy` array
2. Agent creates file lock
3. Agent updates task JSON: `status=in_progress`, `assignee=agent_id`
4. Git push (optimistic locking)
5. If push fails (conflict) → retry with different task

---

## 5. Git Merge Strategy: 16 Agents on One Repo

### 5.1 Repository Structure

**Monorepo:** Single shared Git repository for entire compiler project.

**Per-Agent Workspace:**
- Each agent runs in Docker container
- Repo mounted to `/upstream` (read-only reference)
- Agents maintain `/workspace` local copy (working directory)

**Example Structure:**
```
/upstream/          # Shared repo (mounted read-only)
/workspace/         # Agent's local clone
  src/
  current_tasks/    # Lock files
  tests/
  .git/
```

### 5.2 Merge Protocol

**Standard Git Flow:**
1. Agent pulls from `/upstream` (or central remote)
2. Agent works on task in `/workspace`
3. Agent commits changes locally
4. Agent pulls latest changes (merge)
5. Agent resolves merge conflicts (Claude resolves autonomously)
6. Agent pushes to upstream
7. Agent removes task lock file

**Conflict Frequency:** "Merge conflicts are frequent, but Claude is smart enough to figure that out."

**Conflict Resolution:** Agents resolve conflicts autonomously without human intervention. Claude understands Git conflict markers and semantic context.

### 5.3 Branch Naming & Strategy

**No explicit branching mentioned** in official documentation. Appears to use:
- Main/master branch
- Direct commits with pull-merge-push cycle
- File-based locking prevents duplicate work, not separate branches

**Alternative Patterns (from community):**

**Git Worktrees for Parallel Sessions:**
- Each worktree = separate directory with same Git history
- Isolation: changes in one worktree don't affect others
- Coordination: all worktrees share same Git history and remote
- Parallel sessions: multiple Claude instances in different worktrees

**Worktree Structure:**
```
~/.ccswitch/worktrees/{repo-name}/{session-name}/
main-project/
  feature-auth/
  bugfix-parser/
  refactor-codegen/
```

### 5.4 Who Merges?

**Each Agent Merges:** Every agent is responsible for:
- Pulling latest changes
- Merging upstream into local workspace
- Resolving conflicts
- Pushing merged result

**No Dedicated Merger:** No single agent or lead responsible for merging. Distributed responsibility.

**Merge Frequency:** Continuous. Every commit cycle includes pull-merge-push.

### 5.5 Lock File Coordination

**Lock Removal Protocol:**
1. Agent completes work
2. Agent commits code changes
3. Agent deletes `current_tasks/{task}.txt` lock file
4. Agent commits lock removal
5. Agent pushes (both code + lock removal in same push or separate)

**Prevents Stale Locks:** Git history tracks lock acquisition/release, preventing orphaned locks.

**Known Issues (from production):**
- Stale `.git/index.lock` files can persist 20+ seconds with no process holding them
- Caused by background git operations in Claude Code
- Blocks user git commands: `fatal: Unable to create 'LOCK_PATH': File exists`

---

## 6. Docker Isolation: Container Architecture

### 6.1 Container Contents

**Each Container Includes:**
- Full Claude Code CLI installation
- Git client
- Rust toolchain (for compiler project)
- Build tools (cargo, rustc)
- Test infrastructure
- Mounted repository access

**Container Purpose:** Complete isolation from host system while maintaining persistent credentials and workspace access.

### 6.2 Filesystem Mounting

**Mount Points:**
```
/upstream      # Shared Git repo (read-only or read-write)
/workspace     # Agent's local working directory
/tmp           # Temporary build artifacts
```

**Isolation Strategy:**
- Agents can only access directories explicitly mounted
- Personal files, system configs, sensitive data untouched
- When agent installs packages, modifies configs, or deletes files → host machine remains untouched

**Persistent Storage:**
- `~/.claude/teams/` - mounted for team coordination
- `~/.claude/tasks/` - mounted for task list access
- Git credentials - mounted for push/pull

### 6.3 Network Isolation

**Internet Access:** Compiler project ran **without internet access** per Anthropic blog post.

**Container Network:**
- Multi-container setups: services accessible via internal Docker network only
- Not accessible from host machine
- Isolation prevents unintended external API calls

**DNS/Service Discovery:**
- Containers can communicate via Docker internal DNS
- Service names resolve to container IPs

### 6.4 Tools & Access

**Each Agent Has:**
- Full shell access
- Bash/shell environment
- File system read/write (within mounted volumes)
- Git operations (clone, pull, push, merge)
- Compiler toolchain
- Test execution environment

**Each Agent Does NOT Have:**
- Internet access (per project constraints)
- Host filesystem access (beyond mounts)
- Access to other containers' workspaces directly
- Ability to modify host configuration

### 6.5 Backend Types

**Three Spawning Backends:**

| Backend | Description | Use Case |
|---------|-------------|----------|
| **in-process** | All teammates run inside main terminal | Any terminal, no setup |
| **tmux** | Each teammate in separate tmux pane | macOS, tmux users |
| **iterm2** | Each teammate in iTerm2 split pane | macOS iTerm2 users |

**Auto-Detection Logic:**
1. If running inside tmux → use tmux backend
2. If in iTerm2 with `it2` CLI installed → use iTerm2
3. If tmux available → use tmux in external session
4. Otherwise → fall back to in-process

**Override:**
```bash
export CLAUDE_CODE_SPAWN_BACKEND=in-process
# or
export CLAUDE_CODE_SPAWN_BACKEND=tmux
```

**Container Consideration:** Docker isolation works with all backends. Backend determines UI presentation, not isolation level.

---

## 7. Hierarchy: Lead Agent & Worker Coordination

### 7.1 Team Structure

**Flat Hierarchy with Role Specialization:**

**Team Lead:**
- Creates team via `spawnTeam`
- Spawns teammates
- Assigns tasks (or delegates to self-claiming)
- Synthesizes results
- Approves/rejects plans
- Manages shutdowns
- Fixed for team lifetime (cannot transfer leadership)

**Teammates:**
- Independent Claude Code instances
- Own context window (not inherited from lead)
- Own working directory
- Self-claim tasks or receive assignments
- Report completion via `idle_notification`
- Can message each other directly (peer-to-peer)
- Cannot spawn their own teams (no nesting)

**No Multi-Level Hierarchy:** One level only. Teammates cannot spawn sub-workers.

### 7.2 Specialized Roles

**Compiler Project Roles:**
- Core development agents (lexer, parser, codegen, etc.)
- Code deduplication agent
- Performance optimization agent
- Code quality/design critic
- Documentation maintainer

**Role Assignment:** Based on spawn prompt from lead. Roles are contextual, not hardcoded.

**Example:**
```
Spawn a security reviewer teammate with the prompt: "Review the authentication
module at src/auth/ for security vulnerabilities. Focus on token handling,
session management, and input validation."
```

### 7.3 Task Assignment Patterns

**1. Leader-Directed Coordination:**
- Lead creates tasks
- Lead assigns specific tasks to specific teammates
- Tight control over work distribution

**2. Parallel Swarm Execution:**
- Lead creates task backlog
- Teammates self-claim available tasks
- Maximum parallelization

**3. Pipeline Sequencing:**
- Lead creates tasks with dependencies
- Teammates claim tasks as dependencies complete
- Ordered execution with parallelism within stages

**4. Council Decision-Making:**
- Multiple teammates investigate same problem
- Each reports findings to lead
- Lead synthesizes consensus

**5. Watchdog Quality Monitoring:**
- Dedicated reviewer teammate
- Reviews work from other teammates
- Sends feedback via messaging

### 7.4 Can Workers Spawn Sub-Workers?

**No.** Current limitations:
- Teammates cannot spawn their own teams
- Teammates cannot spawn teammates
- Only the lead can manage the team
- No nested teams

**Rationale:** Prevents unbounded recursion and complexity. Single-level hierarchy is sufficient for current use cases.

### 7.5 Delegate Mode

**Purpose:** Force lead to coordinate only, prevent lead from implementing tasks itself.

**Activation:** Press Shift+Tab to cycle into delegate mode.

**Restricted Tools:**
- Spawning teammates
- Messaging teammates
- Shutting down teammates
- Managing tasks
- **Cannot:** Edit code, run tests, access file system directly

**Use Case:** When lead starts implementing instead of delegating, switch to delegate mode to enforce orchestration-only behavior.

---

## 8. GCC Oracle Strategy: Parallel Debugging

### 8.1 The Kernel Compilation Problem

**Challenge:** Compiling Linux kernel is one giant task. Every agent would:
1. Hit the same bug
2. Fix that bug
3. Overwrite each other's changes
4. No parallelization benefit

**Quote:** "When agents started to compile the Linux kernel, they got stuck. Unlike a test suite with hundreds of independent tests, compiling the Linux kernel is one giant task."

### 8.2 The Solution: Binary Split with GCC Oracle

**Strategy:** Use GCC as a "known-good compiler oracle" to isolate failures.

**Algorithm:**
1. **Random partition:** Randomly compile most kernel files using GCC
2. **Test subset:** Compile remaining files with Claude's C Compiler (ccc)
3. **Outcome A:** Kernel boots → problem NOT in ccc's subset → retry with different partition
4. **Outcome B:** Kernel breaks → problem IS in ccc's subset
5. **Refine:** Re-compile some of ccc's files with GCC to narrow down
6. **Isolate:** Binary search to specific file(s) causing failure
7. **Parallel debug:** Each agent fixes different files simultaneously

**Quote:** "The fix was to use GCC as an online known-good compiler oracle to compare against. A new test harness was written that randomly compiled most of the kernel using GCC, and only the remaining files with Claude's C Compiler."

### 8.3 What "Randomly Compiled Most Files" Means

**Technical Interpretation:**

**Random Sampling:**
- Kernel has thousands of `.c` files
- Test harness selects random subset (e.g., 90% of files)
- Subset compiled with GCC
- Remaining 10% compiled with ccc

**Example:**
```
Total files: 1000
GCC compiles: 900 (randomly selected)
CCC compiles: 100 (remaining)
```

**Binary Split Approach:**
- If kernel boots → CCC's 100 files are correct
- If kernel fails → bug in one of CCC's 100 files
- Refine: compile 50 with GCC, 50 with CCC
- Repeat until single file isolated

**Parallel Work Distribution:**
- Agent 1: tests partition A (files 1-100 with ccc)
- Agent 2: tests partition B (files 101-200 with ccc)
- Agent 3: tests partition C (files 201-300 with ccc)
- Each agent isolates different bugs in different files
- No overlapping work

### 8.4 Test Harness Implementation

**Custom Build System:**
- Modified kernel Makefile or build script
- Accept file list as input
- Compile listed files with GCC
- Compile unlisted files with ccc
- Link all object files into kernel

**Execution:**
```bash
# Pseudocode
gcc_files = random_sample(all_kernel_files, 0.9)
ccc_files = all_kernel_files - gcc_files

for file in gcc_files:
    compile_with_gcc(file)

for file in ccc_files:
    compile_with_ccc(file)

link_kernel()
boot_test()
```

**Feedback Loop:**
- Boot failure → identify faulty file(s)
- Agent fixes identified file
- Re-run test with that file compiled by ccc
- Verify fix
- Commit fix
- Move to next file

### 8.5 GCC Torture Test Suite

**Separate from Kernel Strategy:**

**Purpose:** Rigorous compiler test suite designed to stress-test compiler implementations.

**Structure:**
- Hundreds of independent tests
- Each tests specific C language features
- Edge cases, corner cases, undefined behavior
- Expected output provided

**Usage:**
- Agents run torture tests in parallel
- Each agent claims subset of failing tests
- Fix bugs to pass tests
- No oracle needed (expected output is ground truth)

**Result:** 99% pass rate achieved by Claude's compiler.

**Comparison:**
- **Torture tests:** Independent, parallelizable, ground truth provided
- **Kernel compilation:** Single giant task, required GCC oracle for parallelization

---

## 9. Team Config Schema

### 9.1 config.json Structure

**Location:** `~/.claude/teams/{team-name}/config.json`

**Schema:**
```json
{
  "name": "compiler-team",
  "leadAgentId": "coordinator@compiler-team",
  "members": [
    {
      "agentId": "lexer-agent@compiler-team",
      "name": "lexer-agent",
      "agentType": "specialist",
      "color": "#FF5733",
      "backendType": "tmux",
      "planModeRequired": false
    },
    {
      "agentId": "parser-agent@compiler-team",
      "name": "parser-agent",
      "agentType": "specialist",
      "color": "#33FF57",
      "backendType": "tmux",
      "planModeRequired": false
    },
    {
      "agentId": "security-reviewer@compiler-team",
      "name": "security-reviewer",
      "agentType": "reviewer",
      "color": "#3357FF",
      "backendType": "in-process",
      "planModeRequired": true
    }
  ]
}
```

**Field Descriptions:**

| Field | Type | Description |
|-------|------|-------------|
| `name` | string | Team identifier |
| `leadAgentId` | string | Agent ID of team lead (format: `name@team-name`) |
| `members` | array | List of all team members including lead |
| `members[].agentId` | string | Unique agent identifier |
| `members[].name` | string | Human-readable agent name |
| `members[].agentType` | string | Role/type (specialist, reviewer, general-purpose) |
| `members[].color` | string | Hex color for UI display |
| `members[].backendType` | string | Spawning backend (in-process, tmux, iterm2) |
| `members[].planModeRequired` | boolean | If true, agent must get plan approval before implementing |

### 9.2 Discovery & Access

**Teammates can read config.json** to discover other team members.

**API:**
```bash
cat ~/.claude/teams/compiler-team/config.json | jq '.members[] | {name, agentType, backendType}'
```

**Output:**
```json
{"name": "lexer-agent", "agentType": "specialist", "backendType": "tmux"}
{"name": "parser-agent", "agentType": "specialist", "backendType": "tmux"}
{"name": "security-reviewer", "agentType": "reviewer", "backendType": "in-process"}
```

---

## 10. Production Pain Points & Advanced Patterns

### 10.1 Context Compression Amnesia

**Problem:** Agents lose architectural context mid-task chain (write → test → refactor), requiring costly re-learning.

**Solution:** Persistent workers with session memory.
- Maintain 9 specialized worker types as long-lived processes
- Worker history injection: last N tasks of same type injected into prompt
- Pseudo-memory for stateless processes

**Invariant:** Workers complete write-test-refactor cycles without re-reading entire repositories after compaction.

### 10.2 Self-Review Blindness

**Problem:** Single LLM reviewing its own code misses identical blind spots repeatedly.

**Solution:** Cross-model adversarial validation.
- Use Claude + Kimi (or other models)
- Kimi runs adversarial review on design docs
- Found 12 issues across 4 severity levels in 900-line doc

**Enforcement:** `reviewer_id != assignee_id` enforced as hard ValueError at task creation.

### 10.3 Concurrent File Edit Conflicts

**Problem:** Multiple agents editing identical files cause overwrites.

**Solution:** File lock manager with lease integration.
- Lock tied to task lease
- Lease expires → lock auto-releases (no stale locks)
- Read-to-write upgrade without release gaps
- Path normalization (`./file.py` equals `file.py`)
- Resource budgets per worker type:
  - Explorer: read-only
  - Writer: limited files/lines
  - Reviewer: read-heavy
- Three-layer path security (whitelist, deny patterns, protected files)

**Invariant:** File modification without held lock triggers hard violation requiring human approval.

### 10.4 Lost Task Memory Across Sessions

**Problem:** No persistent backlog between sessions.

**Solution:** Auto-populated persistent backlog with intelligent routing.

**Auto-Population Sources:**
1. Intent parser from conversation: "надо поправить X" → backlog item
2. Worker output scanner: TODOs, critical issues, risk tables → backlog items

**Smart Loop:** Orchestrator checks backlog with adaptive intervals:
- No active tasks → picks highest priority item from backlog
- Creates task → executes
- Repeat

**Invariant:** Orchestrator ALWAYS picks backlog task when idle; user not involved in routine assignment.

### 10.5 Lost Architectural Knowledge

**Problem:** Decisions made in prior sessions get re-debated or contradicted.

**Solution:** Shared Knowledge Graph with MCP access.

**Persistent Entity Types:**
- Decisions (with context)
- Bug patterns (TTL-based)
- Preferences
- Insights
- Components
- Review notes

**Relations:** supersedes, depends_on, related_to

**Invariant:** Recorded architectural decisions available to all workers across all future sessions.

### 10.6 Autonomous Agent Token Waste

**Problem:** Agents performing repetitive low-value actions consume tokens inefficiently.

**Solution:** Self-measurement with circuit breakers and error intelligence.

**Mechanisms:**
- Budget limits on actions/worker launches per hour
- Boring filter skips low-interestingness LLM calls
- Circuit breaker after N consecutive failures
- Error classification: retryable vs fatal vs hidden
- Dead letter queue for poisoned tasks (>N retries)

**Evidence:** Meta-reasoner self-disabled after measuring 95% IDLE time across 737 calls.

**Invariant:** Autonomy cannot exceed budget; poisoned tasks don't retry infinitely.

---

## 11. Event Taxonomy (Production System)

**Categories:**

```
TASK: CREATED, LEASED, COMPLETED, FAILED, POISONED

WORKER: HEARTBEAT, HEARTBEAT_MISSED, COMPACTING

SAFETY: LOCK_ACQUIRED, LOCK_EXPIRED, TXN_COMMIT, TXN_ROLLBACK

WORKFLOW: PIPELINE_CREATED, PIPELINE_RESUMED, GATE_PASSED, GATE_FAILED, REVIEW_LOOP_INSERTED

AUTONOMY: BACKLOG_ITEM_CREATED, BACKLOG_AUTO_PICKED, META_DECISION_LOGGED, META_SELF_DISABLED
```

**Storage:** SQLite WAL with 14 tables and diagnostic views for:
- Dead-letter queue
- Available tasks
- Active locks
- Stale workers
- Stuck tasks

---

## 12. Limitations & Known Issues

### 12.1 Official Limitations

- **No session resumption with in-process teammates:** `/resume` and `/rewind` do not restore in-process teammates
- **Task status can lag:** Teammates sometimes fail to mark tasks as completed, blocking dependent tasks
- **Shutdown can be slow:** Teammates finish current request before shutting down
- **One team per session:** Lead can only manage one team at a time
- **No nested teams:** Teammates cannot spawn their own teams
- **Lead is fixed:** Cannot transfer leadership
- **Permissions set at spawn:** All teammates inherit lead's permission mode
- **Split panes require tmux or iTerm2:** Not supported in VS Code, Windows Terminal, Ghostty

### 12.2 Production Issues

- **File corruption on concurrent access:** No atomic writes to .claude.json
- **OAuth token race condition:** Stale tokens during concurrent refresh (fixed)
- **Stale git locks:** .git/index.lock persists 20+ seconds
- **Heartbeat failures:** 4GB VM insufficient for MCP servers + plugins
- **Session idle timeout:** Claude Desktop auto-quits after 5 minutes (300s SessionIdleManager)
- **MCP server hangs:** No timeout detection, can cause 16+ hour hangs

---

## 13. Key Takeaways for Swarm Implementation

### 13.1 Core Mechanisms

1. **Git-based coordination** is sufficient for task claiming and merge conflict resolution
2. **File-based locks** with lease expiration prevent stale locks
3. **Heartbeat timeout** (5 min) enables crash detection and recovery
4. **Optimistic locking** via Git push resolves race conditions
5. **DAG-based task dependencies** auto-unblock as work completes

### 13.2 Oracle Pattern

**For large monolithic tasks:**
- Use known-good oracle (GCC) to partition problem space
- Random sampling enables binary split debugging
- Parallel agents work on disjoint subsets
- Dramatically improves parallelization efficiency

**Applicable to:**
- Large test suites
- Monolithic build processes
- End-to-end integration tests

### 13.3 Hierarchy vs Flat

**Flat structure with roles > deep hierarchy:**
- Single lead coordinates
- Teammates self-claim from shared backlog
- Specialization via spawn prompts, not hardcoded types
- Peer-to-peer messaging for collaboration
- No sub-worker spawning (prevents unbounded complexity)

### 13.4 Context Management

**Each agent needs:**
- Own context window (not inherited)
- Access to shared knowledge (CLAUDE.md, Knowledge Graph)
- Task history (for continuity across sessions)
- Architectural decisions (to prevent re-debate)

**Do NOT:**
- Inherit conversation history (bloat)
- Share context windows (merge conflicts)
- Require manual synchronization (use filesystem)

### 13.5 Production-Grade Additions

**Beyond basic swarm:**
- Cross-model adversarial validation (prevent self-review blindness)
- Persistent backlog with auto-population
- Circuit breakers and budget limits
- Error classification (retryable vs fatal)
- Dead-letter queue for poisoned tasks
- Multi-threshold compaction with partner notification
- Checkpoint chains for crash recovery

---

## Sources

### Official Anthropic
- [Building a C compiler with a team of parallel Claudes](https://www.anthropic.com/engineering/building-c-compiler)
- [Orchestrate teams of Claude Code sessions - Claude Code Docs](https://code.claude.com/docs/en/agent-teams)
- [GitHub - anthropics/claudes-c-compiler](https://github.com/anthropics/claudes-c-compiler)

### Technical Deep Dives
- [Claude Code Swarm Orchestration Skill - Complete guide](https://gist.github.com/kieranklaassen/4f2aba89594a4aea4ad64d753984b2ea)
- [Claude Code's Hidden Multi-Agent System](https://paddo.dev/blog/claude-code-hidden-swarm/)
- [Production pain points and coordination patterns](https://gist.github.com/sigalovskinick/6cc1cef061f76b7edd198e0ebc863397)

### Community & Analysis
- [Hacker News: We tasked Opus 4.6 using agent teams to build a C Compiler](https://news.ycombinator.com/item?id=46903616)
- [Hacker News: How Anthropic teams use Claude Code](https://news.ycombinator.com/item?id=44678535)
- [Hacker News: Orchestrate teams of Claude Code sessions](https://news.ycombinator.com/item?id=46902368)

### Docker & Isolation
- [Running Claude Code Agents in Docker Containers for Complete Isolation](https://medium.com/@dan.avila7/running-claude-code-agents-in-docker-containers-for-complete-isolation-63036a2ef6f4)
- [Docker Sandboxes: Run Claude Code and More Safely](https://www.docker.com/blog/docker-sandboxes-run-claude-code-and-other-coding-agents-unsupervised-but-safely/)

### Git Worktrees & Parallel Sessions
- [Common workflows - Claude Code Docs](https://code.claude.com/docs/en/common-workflows)
- [Running Multiple Claude Code Sessions in Parallel with git worktree](https://dev.to/datadeer/part-2-running-multiple-claude-code-sessions-in-parallel-with-git-worktree-165i)
- [How we're shipping faster with Claude Code and Git Worktrees](https://incident.io/blog/shipping-faster-with-claude-code-and-git-worktrees)

### Task & Coordination Systems
- [Claude Code's 'Tasks' update lets agents work longer and coordinate across sessions](https://venturebeat.com/orchestration/claude-codes-tasks-update-lets-agents-work-longer-and-coordinate-across)
- [Claude Code Todos to Tasks](https://medium.com/@richardhightower/claude-code-todos-to-tasks-5a1b0e351a1c)
- [The Tasks System: Persistent State for Context Management](https://agentfactory.panaversity.org/docs/General-Agents-Foundations/context-engineering/tasks-system)

### News & Overview
- [Sixteen AI Agents Built a C Compiler From Scratch — And It Actually Works](https://www.webpronews.com/sixteen-ai-agents-built-a-c-compiler-from-scratch-and-it-actually-works/)
- [Anthropic's $20,000 Experiment: How 16 Parallel AI Agents Built a 100,000-Line C Compiler](https://www.webpronews.com/anthropics-20000-experiment-how-16-parallel-ai-agents-built-a-100000-line-c-compiler-from-scratch-in-rust/)
- [No Humans, Just 16 Claude AI Agents Built a Fully Functional C Compiler](https://www.gizmochina.com/2026/02/07/no-humans-just-16-claude-ai-agents-built-a-fully-functional-c-compiler-shocking-developers/)

---

**End of Report**
